"""Packaging regressions for installed dependencies and macOS signing order."""

from __future__ import annotations

import json
import os
import unittest
from pathlib import Path
from unittest import mock

import release_gui_prepare
import validate_workflow
from test_release_gui_versioning import create_packager_fixture, working_directory, workspace_tempdir


REPO_ROOT = Path(__file__).resolve().parents[2]


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
