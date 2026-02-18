#!/usr/bin/env python3
"""Validate GitHub workflow files for YAML/shell/python-c issues."""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

import yaml

YAMLLINT_CONFIG = (
    "{extends: default, rules: {"
    "line-length: {max: 200}, "
    "truthy: disable, "
    "comments-indentation: disable, "
    "document-start: disable, "
    "indentation: {indent-sequences: whatever}"
    "}}"
)

GHA_EXPR_RE = re.compile(r"\$\{\{.*?\}\}", re.DOTALL)
PYTHON_C_RE = re.compile(r"""python3?\s+-c\s+(['"])(.*?)\1""", re.DOTALL)
BASH_READY: bool | None = None


def discover_workflow_paths(inputs: list[str]) -> list[Path]:
    paths: list[Path] = []
    for raw in inputs:
        candidate = Path(raw)
        if candidate.is_dir():
            paths.extend(sorted(candidate.glob("*.yml")))
            paths.extend(sorted(candidate.glob("*.yaml")))
        else:
            paths.append(candidate)

    unique: list[Path] = []
    seen: set[Path] = set()
    for item in paths:
        resolved = item.resolve()
        if resolved not in seen:
            unique.append(item)
            seen.add(resolved)
    return unique


def run_yamllint(paths: list[Path]) -> list[str]:
    if not paths:
        return ["no workflow files were provided to yamllint"]
    if shutil.which("yamllint") is None:
        return ["yamllint is not installed"]

    command = ["yamllint", "-d", YAMLLINT_CONFIG, *[str(path) for path in paths]]
    result = subprocess.run(command, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        output = result.stdout.strip() or result.stderr.strip()
        return [f"yamllint failed:\n{output}"]
    return []


def can_run_bash() -> bool:
    global BASH_READY
    if BASH_READY is not None:
        return BASH_READY

    bash_path = shutil.which("bash")
    if bash_path is None:
        BASH_READY = False
        return BASH_READY

    try:
        probe = subprocess.run(
            ["bash", "--version"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        BASH_READY = False
        return BASH_READY

    BASH_READY = probe.returncode == 0
    return BASH_READY


def load_yaml(path: Path) -> tuple[dict[str, Any] | None, list[str]]:
    if not path.exists():
        return None, [f"{path}: file does not exist"]

    try:
        data = yaml.safe_load(path.read_text(encoding="utf-8"))
    except yaml.YAMLError as exc:
        return None, [f"{path}: YAML parse error: {exc}"]

    if data is None:
        return {}, []
    if not isinstance(data, dict):
        return None, [f"{path}: expected mapping at root"]
    return data, []


def find_on_section(data: dict[str, Any]) -> Any:
    if "on" in data:
        return data["on"]
    if True in data:  # PyYAML can coerce "on" to bool.
        return data[True]
    return None


def extract_run_blocks(
    node: Any, path: str = "", inherited_shell: str = "bash"
) -> list[tuple[str, str, str]]:
    blocks: list[tuple[str, str, str]] = []

    if isinstance(node, dict):
        local_shell = inherited_shell
        defaults = node.get("defaults")
        if isinstance(defaults, dict):
            run_defaults = defaults.get("run")
            if isinstance(run_defaults, dict):
                run_shell = run_defaults.get("shell")
                if isinstance(run_shell, str):
                    local_shell = run_shell

        if "run" in node and isinstance(node["run"], str):
            shell_value = node.get("shell", local_shell)
            shell_name = shell_value if isinstance(shell_value, str) else local_shell
            blocks.append((path or "$", shell_name, node["run"]))

        for key, value in node.items():
            key_path = f"{path}.{key}" if path else str(key)
            blocks.extend(extract_run_blocks(value, key_path, local_shell))

    elif isinstance(node, list):
        for index, value in enumerate(node):
            blocks.extend(
                extract_run_blocks(value, f"{path}[{index}]", inherited_shell)
            )

    return blocks


def is_bash_shell(shell: str) -> bool:
    lowered = shell.lower()
    if "pwsh" in lowered or "powershell" in lowered:
        return False
    return lowered.strip() in {"bash", "sh"} or "bash" in lowered


def sanitize_for_bash(script: str) -> str:
    return GHA_EXPR_RE.sub("GHA_EXPR", script)


def check_bash_syntax(script: str, path: str) -> list[str]:
    if not can_run_bash():
        return []
    result = subprocess.run(
        ["bash", "-n"],
        input=sanitize_for_bash(script),
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip() or result.stdout.strip()
        return [f"{path}: bash syntax error:\n{stderr}"]
    return []


def check_python_c_snippets(script: str, path: str) -> list[str]:
    errors: list[str] = []
    for match in PYTHON_C_RE.finditer(script):
        code = match.group(2)
        try:
            compile(code, "<python-c>", "exec")
        except SyntaxError as exc:
            errors.append(
                f"{path}: invalid python -c snippet: {exc.msg} (line {exc.lineno}, col {exc.offset})"
            )
    return errors


def validate_release_trigger_rules(path: Path, data: dict[str, Any]) -> list[str]:
    if path.name != "release-gui.yml":
        return []

    errors: list[str] = []
    on_section = find_on_section(data)
    if not isinstance(on_section, dict):
        errors.append(f"{path}: expected 'on' to be a mapping")
        return errors

    if "pull_request" in on_section:
        errors.append(f"{path}: release workflow must not define pull_request trigger")

    if "push" not in on_section:
        errors.append(f"{path}: release workflow must define push tag trigger")
        return errors

    push_section = on_section.get("push")
    if not isinstance(push_section, dict):
        errors.append(f"{path}: release workflow push trigger must be a mapping")
        return errors

    tags = push_section.get("tags")
    if not isinstance(tags, list) or "v*" not in tags:
        errors.append(f"{path}: release workflow push.tags must include 'v*'")

    if "workflow_dispatch" not in on_section:
        errors.append(f"{path}: release workflow must define workflow_dispatch")

    return errors


def validate_workflow_file(path: Path) -> list[str]:
    data, errors = load_yaml(path)
    if data is None:
        return errors

    errors.extend(validate_release_trigger_rules(path, data))

    run_blocks = extract_run_blocks(data)
    for block_path, shell, script in run_blocks:
        if is_bash_shell(shell):
            errors.extend(check_bash_syntax(script, f"{path}:{block_path}"))
            errors.extend(check_python_c_snippets(script, f"{path}:{block_path}"))

    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate GitHub workflow YAML files.")
    parser.add_argument(
        "paths",
        nargs="*",
        default=[".github/workflows/release-gui.yml"],
        help="Workflow file(s) or directory containing workflow YAML files",
    )
    args = parser.parse_args()

    workflow_paths = discover_workflow_paths(args.paths)
    if not workflow_paths:
        print("No workflow files found to validate.", file=sys.stderr)
        return 2

    if not can_run_bash():
        print(
            "Warning: bash is unavailable; skipping bash syntax checks.",
            file=sys.stderr,
        )

    failures: list[str] = []
    failures.extend(run_yamllint(workflow_paths))
    for workflow_path in workflow_paths:
        failures.extend(validate_workflow_file(workflow_path))

    if failures:
        print("Workflow validation failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print(f"Workflow validation passed for {len(workflow_paths)} file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
