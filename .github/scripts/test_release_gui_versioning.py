from __future__ import annotations

import json
import os
import subprocess
import shutil
import tarfile
import unittest
import uuid
from contextlib import contextmanager
from pathlib import Path
from unittest import mock

import release_gui_collect
import release_gui_prepare
from release_versioning import (
    VersionValidationError,
    normalize_packaging_tag,
    normalize_packaging_version,
    packager_version_for_runner,
)


@contextmanager
def working_directory(path: Path):
    previous = Path.cwd()
    os.chdir(path)
    try:
        yield
    finally:
        os.chdir(previous)


@contextmanager
def workspace_tempdir():
    root = Path(".tmp") / "release-gui-tests"
    root.mkdir(parents=True, exist_ok=True)
    tempdir = root / uuid.uuid4().hex
    tempdir.mkdir()
    resolved = tempdir.resolve()
    try:
        yield resolved
    finally:
        shutil.rmtree(resolved, ignore_errors=True)


class ReleaseVersioningTests(unittest.TestCase):
    def test_normalize_packaging_version_accepts_prerelease(self) -> None:
        self.assertEqual(normalize_packaging_version("v0.5.0-beta.1"), "0.5.0-beta.1")

    def test_normalize_packaging_tag_accepts_prerelease(self) -> None:
        self.assertEqual(normalize_packaging_tag("0.5.0-beta.1"), "v0.5.0-beta.1")

    def test_normalize_packaging_tag_accepts_build_metadata(self) -> None:
        self.assertEqual(
            normalize_packaging_tag("v0.5.0-beta.1+sha.abc123"),
            "v0.5.0-beta.1+sha.abc123",
        )

    def test_normalize_packaging_tag_rejects_invalid_semver(self) -> None:
        with self.assertRaisesRegex(VersionValidationError, "Cargo semver"):
            normalize_packaging_tag("0.5")

    def test_packager_version_for_windows_strips_prerelease_and_build_metadata(self) -> None:
        self.assertEqual(
            packager_version_for_runner(
                "v0.5.0-beta.1+sha.abc123",
                runner_os="Windows",
            ),
            "0.5.0",
        )

    def test_packager_version_for_linux_preserves_prerelease(self) -> None:
        self.assertEqual(
            packager_version_for_runner("0.5.0-beta.1", runner_os="Linux"),
            "0.5.0-beta.1",
        )


class ReleaseGuiPrepareTests(unittest.TestCase):
    def test_probe_wix_version_accepts_nonzero_help_exit_when_output_has_version(self) -> None:
        with workspace_tempdir() as wix_bin:
            (wix_bin / "candle.exe").write_text("", encoding="utf-8")
            (wix_bin / "light.exe").write_text("", encoding="utf-8")

            side_effect = [
                subprocess.CompletedProcess(
                    [str(wix_bin / "candle.exe"), "-?"],
                    1,
                    stdout="WiX Toolset Compiler version 3.14.1.8722\n",
                    stderr="",
                ),
                subprocess.CompletedProcess(
                    [str(wix_bin / "light.exe"), "-?"],
                    2,
                    stdout="",
                    stderr="WiX Toolset Linker version 3.14.1.8722\n",
                ),
            ]

            with mock.patch.object(
                release_gui_prepare.subprocess,
                "run",
                side_effect=side_effect,
            ):
                self.assertEqual(
                    release_gui_prepare.probe_wix_version(wix_bin),
                    "3.14.1.8722",
                )

    def test_prepare_accepts_prerelease_workspace_version(self) -> None:
        with workspace_tempdir() as root:
            config_dir = root / "packaging" / "linux"
            config_dir.mkdir(parents=True)

            (root / "LICENSE").write_text("MIT\n", encoding="utf-8")
            target_dir = root / "target" / "x86_64-unknown-linux-gnu" / "release"
            target_dir.mkdir(parents=True)
            (target_dir / "localpaste-gui").write_text("binary\n", encoding="utf-8")

            icon_path = config_dir / "icon.png"
            icon_path.write_text("icon\n", encoding="utf-8")
            packager_config = config_dir / "packager.json"
            packager_config.write_text(
                json.dumps(
                    {
                        "formats": ["appimage"],
                        "icons": ["icon.png"],
                    }
                ),
                encoding="utf-8",
            )

            with working_directory(root):
                argv = [
                    "release_gui_prepare.py",
                    "--tag",
                    "0.5.0-beta.1",
                    "--target",
                    "x86_64-unknown-linux-gnu",
                    "--asset-suffix",
                    "linux-x86_64",
                    "--packager-config",
                    str(packager_config),
                    "--runner-os",
                    "Linux",
                ]
                with mock.patch("sys.argv", argv):
                    self.assertEqual(release_gui_prepare.main(), 0)

            effective_config = config_dir / "packager.effective.json"
            self.assertTrue(effective_config.is_file())
            self.assertEqual(
                json.loads(effective_config.read_text(encoding="utf-8"))["version"],
                "0.5.0-beta.1",
            )
            self.assertTrue((root / "dist" / "stage" / "linux-x86_64" / "localpaste").is_file())

    def test_prepare_windows_uses_numeric_version_for_prerelease_workspace_version(self) -> None:
        with workspace_tempdir() as root:
            config_dir = root / "packaging" / "windows"
            config_dir.mkdir(parents=True)

            (root / "LICENSE").write_text("MIT\n", encoding="utf-8")
            target_dir = root / "target" / "x86_64-pc-windows-msvc" / "release"
            target_dir.mkdir(parents=True)
            (target_dir / "localpaste-gui.exe").write_text("binary\n", encoding="utf-8")

            icon_path = config_dir / "localpaste.ico"
            icon_path.write_text("icon\n", encoding="utf-8")
            packager_config = config_dir / "packager.json"
            packager_config.write_text(
                json.dumps(
                    {
                        "formats": ["wix"],
                        "icons": ["localpaste.ico"],
                    }
                ),
                encoding="utf-8",
            )

            with working_directory(root):
                argv = [
                    "release_gui_prepare.py",
                    "--tag",
                    "0.5.0-beta.1",
                    "--target",
                    "x86_64-pc-windows-msvc",
                    "--asset-suffix",
                    "windows-x86_64",
                    "--packager-config",
                    str(packager_config),
                    "--runner-os",
                    "Windows",
                ]
                with (
                    mock.patch("sys.argv", argv),
                    mock.patch.object(
                        release_gui_prepare,
                        "discover_wix_bin",
                        return_value=(Path("C:/WiX/bin"), "3.14.1"),
                    ),
                ):
                    self.assertEqual(release_gui_prepare.main(), 0)

            effective_config = config_dir / "packager.effective.json"
            self.assertTrue(effective_config.is_file())
            self.assertEqual(
                json.loads(effective_config.read_text(encoding="utf-8"))["version"],
                "0.5.0",
            )
            self.assertTrue(
                (root / "dist" / "stage" / "windows-x86_64" / "localpaste.exe").is_file()
            )


class ReleaseGuiCollectTests(unittest.TestCase):
    def test_collect_accepts_prerelease_packaging_tag(self) -> None:
        with workspace_tempdir() as root:
            packager_out = root / "dist" / "packager" / "linux-x86_64"
            packager_out.mkdir(parents=True)
            (packager_out / "LocalPaste.AppImage").write_text("appimage\n", encoding="utf-8")

            stage_dir = root / "dist" / "stage" / "linux-x86_64"
            stage_dir.mkdir(parents=True)
            (stage_dir / "localpaste").write_text("binary\n", encoding="utf-8")
            (stage_dir / "LICENSE").write_text("MIT\n", encoding="utf-8")

            with working_directory(root):
                argv = [
                    "release_gui_collect.py",
                    "--tag",
                    "v0.5.0-beta.1",
                    "--asset-suffix",
                    "linux-x86_64",
                    "--runner-os",
                    "Linux",
                ]
                with mock.patch("sys.argv", argv):
                    self.assertEqual(release_gui_collect.main(), 0)

            release_dir = root / "dist" / "release" / "linux-x86_64"
            appimage = release_dir / "localpaste-v0.5.0-beta.1-linux-x86_64.AppImage"
            tarball = release_dir / "localpaste-v0.5.0-beta.1-linux-x86_64.tar.gz"
            manifest = release_dir / "manifest.json"

            self.assertTrue(appimage.is_file())
            self.assertTrue(tarball.is_file())
            self.assertTrue(manifest.is_file())
            self.assertEqual(
                json.loads(manifest.read_text(encoding="utf-8"))["tag"],
                "v0.5.0-beta.1",
            )

            with tarfile.open(tarball, "r:gz") as archive:
                names = sorted(archive.getnames())
            self.assertEqual(names, ["LICENSE", "localpaste"])
