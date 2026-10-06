"""Regression coverage for cheap, current-commit workflow validation."""

from __future__ import annotations

import fnmatch
import unittest
from pathlib import Path

import yaml


class WorkflowLintTriggerTests(unittest.TestCase):
    def test_workflow_changes_are_checked_on_initial_and_updated_prs(self) -> None:
        path = Path(__file__).resolve().parents[1] / "workflows" / "workflow-lint.yml"
        # BaseLoader preserves GitHub's `on` key instead of treating it as YAML 1.1 bool.
        workflow = yaml.load(path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
        triggers = workflow["on"]
        pr = triggers["pull_request"]
        for action in ("opened", "reopened", "synchronize"):
            with self.subTest(action=action):
                self.assertIn(action, pr["types"])

        for changed_path, expected in (
            (".github/workflows/workflow-lint.yml", True),
            (".github/workflows/release-gui.yml", True),
            (".github/workflows/another-workflow.yaml", True),
            (".github/scripts/validate_workflow.py", True),
            (".github/scripts/test_workflow_lint.py", True),
            (".github/scripts/normalize_release_tag.sh", True),
            ("crates/localpaste_gui/src/main.rs", False),
            ("docs/dev/devlog.md", False),
        ):
            with self.subTest(changed_path=changed_path):
                self.assertEqual(
                    any(fnmatch.fnmatchcase(changed_path, pattern) for pattern in pr["paths"]),
                    expected,
                )

        self.assertIn("workflow_dispatch", triggers)
        self.assertNotIn("push", triggers)
        self.assertNotIn("review_requested", pr["types"])
        self.assertEqual(len(workflow["jobs"]), 1)

