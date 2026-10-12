"""Packaging regressions for installed dependencies and macOS signing order."""

from __future__ import annotations

import json
import os
import subprocess
import unittest
import zipfile
from pathlib import Path
from unittest import mock

import release_gui_prepare
import release_gui_collect
import release_windows_runtime
import validate_workflow
from test_release_gui_versioning import create_packager_fixture, working_directory, workspace_tempdir


REPO_ROOT = Path(__file__).resolve().parents[2]


def create_crt(root):
    """Create a redist layout with the required DLLs and an additional CRT DLL."""
    directory = root / "x64/Microsoft.VC143.CRT"
    directory.mkdir(parents=True)
    for name in (*release_windows_runtime.REQUIRED_DLLS, "concrt140.dll"):
        (directory / name).write_bytes(f"fixture {name}".encode())
    return directory


class WindowsPackagingTests(unittest.TestCase):
    def test_active_redist_is_used_without_querying_other_toolchains(self):
        with workspace_tempdir() as root:
            crt = create_crt(root / "active")
            with mock.patch.dict(os.environ, {"VCToolsRedistDir": str(root / "active")}), \
                    mock.patch.object(release_windows_runtime.subprocess, "run") as run:
                self.assertEqual(release_windows_runtime.discover_vc_runtime(), crt)
                run.assert_not_called()

    def test_vswhere_discovery_selects_newest_complete_numeric_version(self):
        with workspace_tempdir() as root:
            vswhere = root / "program-files/Microsoft Visual Studio/Installer/vswhere.exe"
            vswhere.parent.mkdir(parents=True)
            vswhere.touch()
            redist = root / "visual-studio/VC/Redist/MSVC"
            create_crt(redist / "14.9.1")
            expected = create_crt(redist / "14.10.1")
            incomplete = create_crt(redist / "14.11.1")
            (incomplete / "vcruntime140_1.dll").unlink()
            result = subprocess.CompletedProcess([], 0, stdout=str(root / "visual-studio") + "\n")
            with mock.patch.dict(os.environ, {"ProgramFiles(x86)": str(root / "program-files")}, clear=True), \
                    mock.patch.object(release_windows_runtime.subprocess, "run", return_value=result) as run:
                self.assertEqual(release_windows_runtime.discover_vc_runtime(), expected)
                self.assertIn("Microsoft.VisualStudio.Component.VC.Tools.x86.x64", run.call_args.args[0])

    def test_discovery_rejects_incomplete_redist(self):
        with workspace_tempdir() as root:
            crt = create_crt(root)
            (crt / "msvcp140.dll").unlink()
            with mock.patch.dict(os.environ, {"VCToolsRedistDir": str(root)}, clear=True):
                with self.assertRaisesRegex(ValueError, "complete x64 Microsoft C\\+\\+ redistributable"):
                    release_windows_runtime.discover_vc_runtime()

    def test_prepare_and_zip_include_runtime_dlls_beside_executable(self):
        with workspace_tempdir() as root:
            target = "x86_64-pc-windows-msvc"
            config = create_packager_fixture(root, "windows", target)
            crt = create_crt(root / "redist")
            argv = ["release_gui_prepare.py", "--tag", "v0.5.0", "--target", target,
                    "--asset-suffix", "windows-x86_64", "--packager-config", str(config),
                    "--runner-os", "Windows"]
            with working_directory(root), mock.patch("sys.argv", argv), \
                    mock.patch.dict(os.environ, {"VCToolsRedistDir": str(root / "redist")}), \
                    mock.patch.object(release_gui_prepare, "discover_wix_bin", return_value=(root / "wix/bin", "3.14.1")):
                self.assertEqual(release_gui_prepare.main(), 0)
            stage = root / "dist/stage/windows-x86_64"
            expected = {path.name: path.read_bytes() for path in crt.iterdir()}
            effective = json.loads(config.with_name("packager.effective.json").read_text())
            self.assertEqual({item["target"] for item in effective["resources"]}, set(expected))
            for resource in effective["resources"]:
                self.assertEqual(Path(resource["src"]), stage / resource["target"])
                self.assertEqual(Path(resource["src"]).read_bytes(), expected[resource["target"]])
            packages = root / "packages"
            packages.mkdir()
            (packages / "LocalPaste.msi").write_bytes(b"installer fixture")
            release = root / "release"
            release.mkdir()
            names = release_gui_collect.collect_windows("v0.5.0", "windows-x86_64", packages, release, stage)
            with zipfile.ZipFile(release / names[1]) as archive:
                self.assertEqual(set(archive.namelist()), {"localpaste.exe", "LICENSE", *expected})
                self.assertEqual(archive.read("localpaste.exe"), (stage / "localpaste.exe").read_bytes())
                for name, payload in expected.items():
                    self.assertEqual(archive.read(name), payload)

    def test_zip_rejects_missing_or_empty_runtime_instead_of_publishing_broken_payload(self):
        with workspace_tempdir() as root:
            stage = create_crt(root)
            (stage / "localpaste.exe").write_bytes(b"executable fixture")
            (stage / "LICENSE").write_text("MIT")
            packages, release = root / "packages", root / "release"
            packages.mkdir()
            release.mkdir()
            (packages / "LocalPaste.msi").write_bytes(b"installer fixture")
            for name in release_windows_runtime.REQUIRED_DLLS:
                for missing in (True, False):
                    with self.subTest(name=name, missing=missing):
                        path = stage / name
                        if missing:
                            path.unlink()
                        else:
                            path.write_bytes(b"")
                        with self.assertRaises(SystemExit):
                            release_gui_collect.collect_windows("v0.5.0", "windows-x86_64", packages, release, stage)
                        self.assertFalse(list(release.glob("*.zip")))
                        path.write_bytes(b"runtime fixture")


class MacosPackagingTests(unittest.TestCase):
    def prepare(self, root, config, env):
        argv = [
            "release_gui_prepare.py", "--tag", "v0.5.0", "--target", "aarch64-apple-darwin",
            "--asset-suffix", "macos-aarch64", "--packager-config", str(config), "--runner-os", "macOS",
        ]
        with working_directory(root), mock.patch.dict(os.environ, env), mock.patch("sys.argv", argv):
            self.assertEqual(release_gui_prepare.main(), 0)
        return json.loads(config.with_name("packager.effective.json").read_text())

    def test_signed_config_enables_packager_signing_before_dmg_assembly(self):
        with workspace_tempdir() as root:
            config = create_packager_fixture(root, "macos", "aarch64-apple-darwin")
            result = self.prepare(root, config, {
                "MACOS_SIGNING_ENABLED": "true", "APPLE_SIGNING_IDENTITY": "Developer ID Application: Test",
            })
            self.assertEqual(result["macos"]["signingIdentity"], "Developer ID Application: Test")
            self.assertEqual(result["formats"], ["dmg", "app"])

    def test_unsigned_config_ignores_partial_credentials_and_removes_stale_identity(self):
        with workspace_tempdir() as root:
            config = create_packager_fixture(root, "macos", "aarch64-apple-darwin")
            data = json.loads(config.read_text())
            data["macos"] = {"signingIdentity": "stale", "minimumSystemVersion": "11.0"}
            config.write_text(json.dumps(data))
            result = self.prepare(root, config, {
                "MACOS_SIGNING_ENABLED": "false", "APPLE_SIGNING_IDENTITY": "partial credentials",
            })
            self.assertEqual(result["macos"], {"minimumSystemVersion": "11.0"})

    def test_enabled_signing_requires_identity(self):
        with workspace_tempdir() as root:
            config = create_packager_fixture(root, "macos", "aarch64-apple-darwin")
            with self.assertRaises(SystemExit):
                self.prepare(root, config, {"MACOS_SIGNING_ENABLED": "true", "APPLE_SIGNING_IDENTITY": ""})
            self.assertFalse(config.with_name("packager.effective.json").exists())

    def test_both_workflows_supply_native_signing_credentials_during_packaging(self):
        credentials = {
            "APPLE_CERTIFICATE": "APPLE_SIGNING_CERT_BASE64",
            "APPLE_CERTIFICATE_PASSWORD": "APPLE_SIGNING_CERT_PASSWORD",
            "APPLE_ID": "APPLE_ID", "APPLE_PASSWORD": "APPLE_APP_SPECIFIC_PASSWORD",
            "APPLE_TEAM_ID": "APPLE_TEAM_ID",
        }
        for workflow, job_name in (
            ("release-gui.yml", "build_package"), ("verify-gui-packaging.yml", "macos-package-verify"),
        ):
            with self.subTest(workflow=workflow):
                path = REPO_ROOT / ".github/workflows" / workflow
                data, errors = validate_workflow.load_yaml(path)
                self.assertEqual(errors, [])
                job = data["jobs"][job_name]
                prepare = validate_workflow.find_step(job, step_name="Prepare packager config and staged runtime")
                self.assertEqual(prepare["env"]["MACOS_SIGNING_ENABLED"], "${{ steps.mac_signing.outputs.enabled }}")
                self.assertEqual(prepare["env"]["APPLE_SIGNING_IDENTITY"], "${{ secrets.APPLE_SIGNING_IDENTITY }}")
                package = validate_workflow.find_step(job, step_name="Build installer packages")
                for name, secret in credentials.items():
                    self.assertEqual(package["env"][name],
                        "${{ steps.mac_signing.outputs.enabled == 'true' && secrets." + secret + " || '' }}")
                notarize = validate_workflow.find_step(job, step_name="Notarize DMG and verify macOS artifacts")
                self.assertLess(job["steps"].index(package), job["steps"].index(notarize))
                self.assertNotIn("codesign --force", notarize["run"])
                self.assertIn('xcrun stapler validate "${APP_BUNDLE}"', notarize["run"])
                self.assertEqual(validate_workflow.validate_release_macos_signing_rules(path, data), [])

    def test_manual_rehearsal_uses_committed_lockfile(self):
        data, errors = validate_workflow.load_yaml(REPO_ROOT / ".github/workflows/verify-gui-packaging.yml")
        self.assertEqual(errors, [])
        job = data["jobs"]["macos-package-verify"]
        for name in ("Workspace check", "GUI test suite", "Build GUI release binary"):
            self.assertIn("--locked", validate_workflow.find_step(job, step_name=name)["run"])
