"""Execute the actual release-note step against a fake GitHub CLI."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import validate_workflow
from test_release_workflow import WORKFLOW_PATH, load_workflow


START = "<!-- localpaste-macos-install:start -->"
END = "<!-- localpaste-macos-install:end -->"
LEGACY = (
    "macOS note: this release may include unsigned/unnotarized LocalPaste macOS artifacts. "
    "If Gatekeeper blocks LocalPaste, use Open Anyway in System Settings > Privacy & Security "
    "or run `xattr -cr /Applications/LocalPaste.app`."
)


class ReleaseNoteTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="localpaste release notes ")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.state_path = self.root / "state.json"
        self.gh = self.root / "gh"
        self.gh.write_text(f"#!{sys.executable}\n" + '''
import json
import os
import sys
from pathlib import Path
path = Path(os.environ["FAKE_GH_STATE"])
state = json.loads(path.read_text())
args = sys.argv[1:]
state["calls"].append(args)
path.write_text(json.dumps(state))
if args[:2] == ["release", "view"]:
    if state["failure"] == "read":
        print("simulated GitHub read failure", file=sys.stderr)
        sys.exit(1)
    print(json.dumps({"body": state["body"]}))
elif args[:2] == ["release", "edit"]:
    if state["failure"] == "write":
        print("simulated GitHub write failure", file=sys.stderr)
        sys.exit(1)
    state["body"] = Path(args[args.index("--notes-file") + 1]).read_text()
    path.write_text(json.dumps(state))
else:
    sys.exit("unexpected gh command")
''')
        self.gh.chmod(0o755)
        self.step = validate_workflow.find_step(load_workflow()["jobs"]["publish"], step_id="mac_gatekeeper")

    def run_step(self, body, *, failure="", calls=None):
        self.state_path.write_text(json.dumps({"body": body, "failure": failure, "calls": calls or []}))
        result = subprocess.run(["bash", "-c", self.step["run"]], cwd=self.root,
            env=os.environ | {"PATH": str(self.root) + os.pathsep + os.environ["PATH"],
                "FAKE_GH_STATE": str(self.state_path), "GH_REPO": "pszemraj/localpaste.rs",
                "RELEASE_TAG": "v0.5.0", "GH_TOKEN": "fixture-token"},
            capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result, json.loads(self.state_path.read_text())

    def test_empty_release_gets_tip_with_installed_app_and_actual_dmg_commands(self):
        result, state = self.run_step(None)
        self.assertNotIn("::warning::", result.stdout)
        self.assertIn("> [!TIP]\n", state["body"])
        self.assertIn('> xattr -dr com.apple.quarantine "/Applications/LocalPaste.app"', state["body"])
        self.assertIn('> xattr -d com.apple.quarantine "$HOME/Downloads/localpaste-v0.5.0-macos-aarch64.dmg"', state["body"])
        for call in state["calls"]:
            self.assertEqual(call[call.index("--repo") + 1], "pszemraj/localpaste.rs")

    def test_authored_notes_preserved_and_shell_syntax_remains_literal(self):
        body = '# Release notes\n\nUnicode: café. `touch backtick-executed` $(touch dollar-executed)\n'
        _, state = self.run_step(body)
        self.assertTrue(state["body"].startswith(body + "\n"))
        self.assertFalse((self.root / "backtick-executed").exists())
        self.assertFalse((self.root / "dollar-executed").exists())

    def test_rerun_does_not_duplicate_alert_or_edit_unchanged_notes(self):
        _, first = self.run_step("Authored notes")
        result, second = self.run_step(first["body"], calls=first["calls"])
        self.assertEqual(first["body"], second["body"])
        self.assertEqual(sum(call[:2] == ["release", "edit"] for call in second["calls"]), 1)
        self.assertEqual(second["body"].count(START), 1)
        self.assertIn("already up to date", result.stdout)

    def test_existing_managed_alert_is_updated_without_changing_surrounding_notes(self):
        before, after = "Authored before\n\n", "\n\nAuthored after\n"
        _, state = self.run_step(before + START + "\n> stale instructions\n" + END + after)
        self.assertTrue(state["body"].startswith(before + START))
        self.assertTrue(state["body"].endswith(END + after))
        self.assertNotIn("stale instructions", state["body"])

    def test_only_the_exact_legacy_generated_paragraph_is_replaced(self):
        authored = "Release changes\n\nmacOS note: maintainer-written text\n\n"
        _, state = self.run_step(authored + LEGACY + "\n")
        self.assertTrue(state["body"].startswith(authored))
        self.assertNotIn(LEGACY, state["body"])
        self.assertEqual(state["body"].count(START), 1)

    def test_read_and_write_errors_warn_without_failing_or_changing_notes(self):
        for failure in ("read", "write"):
            with self.subTest(failure=failure):
                result, state = self.run_step("Original notes", failure=failure)
                self.assertEqual(state["body"], "Original notes")
                self.assertIn("::warning::", result.stdout)
                self.assertIn("Release assets remain published", result.stdout)
                self.assertIn(f"simulated GitHub {failure} failure", result.stderr)
                self.assertEqual(len(state["calls"]), 1 if failure == "read" else 2)

    def test_incomplete_markers_leave_release_notes_untouched(self):
        for body in (START + "\nAuthored notes", END, END + START, START + START + END):
            with self.subTest(body=body):
                result, state = self.run_step(body)
                self.assertEqual(state["body"], body)
                self.assertEqual(len(state["calls"]), 1)
                self.assertIn("::warning::", result.stdout)

    def test_validator_keeps_notes_optional_and_asset_upload_required(self):
        data = load_workflow()
        self.assertEqual(validate_workflow.validate_release_note_rules(WORKFLOW_PATH, data), [])
        job = data["jobs"]["publish"]
        note = validate_workflow.find_step(job, step_id="mac_gatekeeper")
        note["continue-on-error"] = False
        self.assertTrue(validate_workflow.validate_release_note_rules(WORKFLOW_PATH, data))
        note["continue-on-error"] = True
        upload = validate_workflow.find_step(job, step_id="publish_release")
        upload["continue-on-error"] = True
        self.assertTrue(validate_workflow.validate_release_note_rules(WORKFLOW_PATH, data))
        del upload["continue-on-error"]
        job["continue-on-error"] = True
        self.assertTrue(validate_workflow.validate_release_note_rules(WORKFLOW_PATH, data))

    def test_validator_requires_note_update_after_successful_asset_upload(self):
        data = load_workflow()
        steps = data["jobs"]["publish"]["steps"]
        steps.insert(0, steps.pop())
        self.assertTrue(validate_workflow.validate_release_note_rules(WORKFLOW_PATH, data))

    def test_signed_builds_skip_unsigned_install_instructions(self):
        data = load_workflow()
        gate = validate_workflow.find_step(data["jobs"]["build_package"], step_id="mac_signing")
        signed = data["jobs"]["publish"]["env"]["MACOS_SIGNED"]
        self.assertTrue(gate["env"])
        for secret in gate["env"]:
            self.assertIn(f"secrets.{secret} != ''", signed)
        self.assertIn("env.MACOS_SIGNED != 'true'", self.step["if"])
