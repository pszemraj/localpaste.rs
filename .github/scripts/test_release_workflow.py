"""Regression checks for release-only builds and tag source selection."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import validate_workflow


REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOW_PATH = REPO_ROOT / ".github/workflows/release-gui.yml"


def load_workflow():
    """Read the actual release workflow, preserving PyYAML's event-key handling."""
    data, errors = validate_workflow.load_yaml(WORKFLOW_PATH)
    if errors or data is None:
        raise AssertionError(errors)
    return data


class ReleaseTriggerTests(unittest.TestCase):
    def test_release_workflow_accepts_only_published_and_manual_triggers(self):
        data = load_workflow()
        self.assertEqual(
            set(validate_workflow.find_on_section(data)),
            {"release", "workflow_dispatch"},
        )
        self.assertEqual(
            validate_workflow.validate_release_trigger_rules(WORKFLOW_PATH, data), []
        )

    def test_validator_rejects_recurring_and_duplicate_build_triggers(self):
        for event in ("schedule", "push", "pull_request", "workflow_run"):
            with self.subTest(event=event):
                data = load_workflow()
                validate_workflow.find_on_section(data)[event] = {}
                errors = validate_workflow.validate_release_trigger_rules(WORKFLOW_PATH, data)
                self.assertTrue(any(f"must not define {event} trigger" in error for error in errors))

    def test_validator_rejects_draft_and_release_edit_triggers(self):
        for types in (None, ["created"], ["edited"], ["published", "edited"]):
            with self.subTest(types=types):
                data = load_workflow()
                validate_workflow.find_on_section(data)["release"] = {"types": types}
                errors = validate_workflow.validate_release_trigger_rules(WORKFLOW_PATH, data)
                self.assertTrue(any("release.types: [published]" in error for error in errors))

    def test_validator_requires_release_trigger(self):
        data = load_workflow()
        del validate_workflow.find_on_section(data)["release"]
        self.assertTrue(validate_workflow.validate_release_trigger_rules(WORKFLOW_PATH, data))


class ReleaseSourceTests(unittest.TestCase):
    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory(prefix="localpaste-release-test-")
        self.addCleanup(self.tempdir.cleanup)
        self.root = Path(self.tempdir.name)
        scripts = self.root / ".github/scripts"
        scripts.mkdir(parents=True)
        for name in (
            "normalize_release_tag.sh",
            "workspace_version.sh",
            "normalize_packaging_tag.py",
            "release_versioning.py",
        ):
            shutil.copy2(REPO_ROOT / ".github/scripts" / name, scripts / name)
        self.env = os.environ | {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
        self.git("init", "--quiet")
        manifest = self.root / "Cargo.toml"
        manifest.write_text('[workspace.package]\nversion = "0.5.0"\n', encoding="utf-8")
        (self.root / "Cargo.lock").write_text("# release lockfile\n", encoding="utf-8")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "release")
        self.release_sha = self.git("rev-parse", "HEAD")
        self.git("tag", "v0.5.0")
        manifest.write_text('[workspace.package]\nversion = "0.5.1"\n', encoding="utf-8")
        self.git("add", "Cargo.toml")
        self.git("commit", "--quiet", "-m", "next development version")
        self.current_sha = self.git("rev-parse", "HEAD")
        self.git("remote", "add", "origin", str(self.root))

    def git(self, *args):
        """Run Git only in the disposable fixture, with no signing or hooks."""
        return subprocess.run(
            [
                "git", "-c", "user.name=Release test", "-c", "user.email=release@example.invalid",
                "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", *args,
            ],
            cwd=self.root, env=self.env, check=True, capture_output=True, text=True,
        ).stdout.strip()

    def resolve(self, **inputs):
        """Execute the workflow's source resolver against the fixture repository."""
        step = validate_workflow.find_step(load_workflow()["jobs"]["resolve_tag"], step_id="resolve")
        self.assertIsNotNone(step)
        output = self.root / "outputs"
        env = self.env | {
            "GITHUB_OUTPUT": str(output), "GITHUB_STEP_SUMMARY": str(self.root / "summary"),
            "GITHUB_SHA": self.current_sha, "GITHUB_REF_NAME": "main",
            "INPUT_SOURCE_MODE": "", "INPUT_TAG": "", "RELEASE_TAG": "", **inputs,
        }
        result = subprocess.run(
            ["bash", "-c", step["run"]], cwd=self.root, env=env,
            capture_output=True, text=True,
        )
        outputs = {}
        if output.exists():
            outputs = dict(line.split("=", 1) for line in output.read_text().splitlines())
        return result, outputs

    def test_published_release_builds_tag_commit_even_when_main_has_advanced(self):
        result, outputs = self.resolve(GITHUB_EVENT_NAME="release", RELEASE_TAG="v0.5.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(outputs, {
            "source_mode": "release_tag", "resolved_tag": "v0.5.0", "source_ref": self.release_sha,
        })
        self.assertNotEqual(outputs["source_ref"], self.current_sha)

    def test_manual_release_retry_still_resolves_existing_tag(self):
        result, outputs = self.resolve(
            GITHUB_EVENT_NAME="workflow_dispatch", INPUT_SOURCE_MODE="release_tag", INPUT_TAG="0.5.0",
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(outputs["source_ref"], self.release_sha)

    def test_manual_rehearsal_defaults_to_current_ref(self):
        result, outputs = self.resolve(GITHUB_EVENT_NAME="workflow_dispatch")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(outputs, {
            "source_mode": "current_ref", "resolved_tag": "v0.5.1", "source_ref": self.current_sha,
        })

    def test_missing_release_tag_fails_before_building(self):
        result, outputs = self.resolve(GITHUB_EVENT_NAME="release", RELEASE_TAG="v0.9.9")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("release tag does not exist: v0.9.9", result.stderr)
        self.assertEqual(outputs, {})

    def test_manual_retry_uses_workflow_helpers_without_changing_tagged_source(self):
        helper = self.root / ".github/scripts/release_gui_prepare.py"
        helper.write_text("# corrected packaging helper\n")
        self.git("add", str(helper))
        self.git("commit", "--quiet", "-m", "fix packaging")
        workflow_sha = self.git("rev-parse", "HEAD")
        self.git("checkout", "--quiet", "v0.5.0")
        self.assertFalse(helper.exists())
        runner_temp = self.root / "runner-temp"
        runner_temp.mkdir()
        github_env = self.root / "github-env"
        step = validate_workflow.find_step(load_workflow()["jobs"]["build_package"],
            step_name="Load packaging helpers from workflow commit")
        result = subprocess.run(["bash", "-c", step["run"]], cwd=self.root,
            env=self.env | {"RUNNER_TEMP": str(runner_temp), "WORKFLOW_SHA": workflow_sha,
                            "GITHUB_ENV": str(github_env)}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        scripts = Path(github_env.read_text().strip().split("=", 1)[1])
        self.assertEqual((scripts / helper.name).read_text(), "# corrected packaging helper\n")
        self.assertFalse(helper.exists())
        self.assertEqual(self.git("rev-parse", "HEAD"), self.release_sha)

    def test_local_mac_build_uses_tagged_source_with_checkout_cache_and_config(self):
        """Exercise the real source/cache/config shell logic without a native Mac build."""
        manifest = self.root / "Cargo.toml"
        lockfile = self.root / "Cargo.lock"
        manifest.write_text("[workspace\n# unfinished local edit\n")
        lockfile.write_text("# unfinished local lockfile\n")
        before = (manifest.read_bytes(), lockfile.read_bytes(), self.git("rev-parse", "HEAD"))
        tagged_manifest = self.git("show", "v0.5.0:Cargo.toml") + "\n"
        tagged_lockfile = self.git("show", "v0.5.0:Cargo.lock") + "\n"

        script = (REPO_ROOT / "tools/build-macos-app.sh").read_text()
        prefix, marker, _ = script.partition("\npackage_id=")
        self.assertTrue(marker, "could not locate the boundary before the native build")
        tools = self.root / "tools"
        tools.mkdir()
        probe = tools / "cache-probe.sh"
        # Only native platform detection and JSON decoding are substituted.
        probe.write_text(prefix.replace("/usr/bin/plutil", "plutil") +
                         '\nprintf "%s\\n" "$target_dir"\n'
                         'if [[ -f .cargo/config.toml ]]; then cat .cargo/config.toml; fi\n')
        stubs = self.root / "test-bin"
        stubs.mkdir()
        commands = {
            "uname": "#!/bin/sh\nprintf 'Darwin\\n'\n",
            "plutil": f"#!{sys.executable}\nimport json, sys\n"
                      "print(json.load(sys.stdin)['target_directory'])\n",
            "cargo": f"#!{sys.executable}\n" + """import json, os, sys
from pathlib import Path
args = sys.argv[1:]
assert args[0] == "metadata" and "--locked" in args and "--no-deps" in args
manifest = (Path(args[args.index("--manifest-path") + 1])
            if "--manifest-path" in args else Path.cwd() / "Cargo.toml")
assert manifest.read_text() == os.environ["EXPECTED_TAG_MANIFEST"], "dirty workspace evaluated"
assert (manifest.parent / "Cargo.lock").read_text() == os.environ["EXPECTED_TAG_LOCKFILE"]
configured = os.environ.get("CARGO_TARGET_DIR")
target = Path(configured) if configured else manifest.parent / "target"
target = target if target.is_absolute() else Path.cwd() / target
Path(os.environ["METADATA_LOG"]).write_text(json.dumps({
    "manifest": str(manifest), "cwd": str(Path.cwd()),
}))
print(json.dumps({"target_directory": str(target.resolve())}))
""",
        }
        for name, content in commands.items():
            command = stubs / name
            command.write_text(content)
            command.chmod(0o755)
        metadata_log = self.root / "metadata-log.json"
        env = self.env | {
            "PATH": str(stubs) + os.pathsep + os.environ["PATH"],
            "EXPECTED_TAG_MANIFEST": tagged_manifest, "EXPECTED_TAG_LOCKFILE": tagged_lockfile,
            "METADATA_LOG": str(metadata_log),
        }
        env.pop("CARGO_TARGET_DIR", None)
        local_config = self.root / ".cargo/config.toml"
        cases = [(configured, cargo_config)
                 for configured in (None, str(self.root / "cache with spaces"), "relative cache")
                 for cargo_config in (None, "[build]\njobs = 3\n")]
        for configured, cargo_config in cases:
            with self.subTest(configured=configured, cargo_config=cargo_config):
                if cargo_config:
                    local_config.parent.mkdir(exist_ok=True)
                    local_config.write_text(cargo_config)
                elif local_config.exists():
                    local_config.unlink()
                case_env = env | ({"CARGO_TARGET_DIR": configured} if configured else {})
                result = subprocess.run(
                    ["/bin/bash", str(probe), "v0.5.0"], cwd=self.root,
                    env=case_env, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                expected = Path(configured) if configured else self.root / "target"
                if not expected.is_absolute():
                    expected = self.root / expected
                target, _, build_config = result.stdout.partition("\n")
                self.assertEqual(Path(target).resolve(), expected.resolve())
                # The untracked repo-local Cargo config reaches the build's working directory.
                self.assertEqual(build_config, cargo_config or "")
                metadata = json.loads(metadata_log.read_text())
                self.assertEqual(Path(metadata["cwd"]), self.root.resolve())
                self.assertNotEqual(Path(metadata["manifest"]), manifest.resolve())
                self.assertEqual(
                    (manifest.read_bytes(), lockfile.read_bytes(), self.git("rev-parse", "HEAD")),
                    before,
                )


if __name__ == "__main__":
    unittest.main()
