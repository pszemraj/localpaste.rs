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
MATRIX_EXPR_RE = re.compile(
    r"^\$\{\{\s*matrix\.([A-Za-z_][A-Za-z0-9_]*)\s*\}\}$"
)
POWERSHELL_TESTPATH_AND_RE = re.compile(
    r"(?<!\()Test-Path\s+\([^)]*\)\s+-and\s+(?<!\()Test-Path\s+\(",
    re.IGNORECASE,
)
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


def normalize_shell_name(shell: str | None) -> str:
    if not isinstance(shell, str):
        return "unknown"
    lowered = shell.lower().strip()
    if "pwsh" in lowered or "powershell" in lowered:
        return "pwsh"
    if lowered in {"bash", "sh"} or "bash" in lowered:
        return "bash"
    return "unknown"


def read_run_default_shell(node: dict[str, Any]) -> str | None:
    defaults = node.get("defaults")
    if not isinstance(defaults, dict):
        return None
    run_defaults = defaults.get("run")
    if not isinstance(run_defaults, dict):
        return None
    shell = run_defaults.get("shell")
    if not isinstance(shell, str):
        return None
    return normalize_shell_name(shell)


def resolve_matrix_values(strategy: Any, key: str) -> list[str]:
    if not isinstance(strategy, dict):
        return []

    matrix = strategy.get("matrix")
    if not isinstance(matrix, dict):
        return []

    values: list[str] = []
    direct = matrix.get(key)
    if isinstance(direct, list):
        values.extend(str(value) for value in direct if isinstance(value, str))
    elif isinstance(direct, str):
        values.append(direct)

    include = matrix.get("include")
    if isinstance(include, list):
        for entry in include:
            if isinstance(entry, dict):
                included_value = entry.get(key)
                if isinstance(included_value, str):
                    values.append(included_value)

    unique: list[str] = []
    for value in values:
        if value not in unique:
            unique.append(value)
    return unique


def resolve_runs_on_labels(job: dict[str, Any]) -> list[str]:
    runs_on = job.get("runs-on")
    strategy = job.get("strategy")

    labels: list[str] = []

    def append_label(value: Any) -> None:
        if not isinstance(value, str):
            labels.append("unknown")
            return

        match = MATRIX_EXPR_RE.match(value.strip())
        if not match:
            labels.append(value)
            return

        matrix_key = match.group(1)
        matrix_values = resolve_matrix_values(strategy, matrix_key)
        if matrix_values:
            labels.extend(matrix_values)
        else:
            labels.append("unknown")

    if isinstance(runs_on, list):
        for value in runs_on:
            append_label(value)
    else:
        append_label(runs_on)

    if not labels:
        labels.append("unknown")

    unique: list[str] = []
    for label in labels:
        if label not in unique:
            unique.append(label)
    return unique


def infer_runner_default_shell(job: dict[str, Any]) -> str:
    inferred: set[str] = set()
    for label in resolve_runs_on_labels(job):
        lowered = label.lower()
        if "windows" in lowered:
            inferred.add("pwsh")
        elif "ubuntu" in lowered or "linux" in lowered or "macos" in lowered:
            inferred.add("bash")
        else:
            inferred.add("unknown")

    if len(inferred) == 1:
        return next(iter(inferred))

    # Mixed matrix runners (e.g. windows + linux) or unresolved values
    # should not be forced through bash syntax checks.
    return "unknown"


def extract_job_run_blocks(data: dict[str, Any]) -> list[tuple[str, str, str]]:
    blocks: list[tuple[str, str, str]] = []

    workflow_default_shell = read_run_default_shell(data)
    jobs = data.get("jobs")
    if not isinstance(jobs, dict):
        return blocks

    for job_name, job in jobs.items():
        if not isinstance(job, dict):
            continue

        default_shell = workflow_default_shell or infer_runner_default_shell(job)
        job_default_shell = read_run_default_shell(job)
        if job_default_shell is not None:
            default_shell = job_default_shell

        steps = job.get("steps")
        if not isinstance(steps, list):
            continue

        for index, step in enumerate(steps):
            if not isinstance(step, dict):
                continue
            run_script = step.get("run")
            if not isinstance(run_script, str):
                continue

            shell_name = default_shell
            if isinstance(step.get("shell"), str):
                shell_name = normalize_shell_name(step["shell"])

            blocks.append((f"jobs.{job_name}.steps[{index}]", shell_name, run_script))

    return blocks


def is_bash_shell(shell: str) -> bool:
    return normalize_shell_name(shell) == "bash"


def is_pwsh_shell(shell: str) -> bool:
    return normalize_shell_name(shell) == "pwsh"


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


def check_powershell_command_mode_traps(script: str, path: str) -> list[str]:
    errors: list[str] = []
    for index, line in enumerate(script.splitlines(), start=1):
        if POWERSHELL_TESTPATH_AND_RE.search(line):
            errors.append(
                f"{path}:line {index}: suspicious PowerShell Test-Path expression; "
                "wrap each Test-Path call in parentheses before using -and"
            )
    return errors


def normalize_needs(needs: Any) -> set[str]:
    if isinstance(needs, str):
        return {needs}
    if isinstance(needs, list):
        return {entry for entry in needs if isinstance(entry, str)}
    return set()


def has_uses_step(job: dict[str, Any], action_prefix: str) -> bool:
    steps = job.get("steps")
    if not isinstance(steps, list):
        return False
    lowered = action_prefix.lower()
    for step in steps:
        if not isinstance(step, dict):
            continue
        uses = step.get("uses")
        if isinstance(uses, str) and uses.lower().startswith(lowered):
            return True
    return False


def has_named_step(job: dict[str, Any], step_name: str) -> bool:
    steps = job.get("steps")
    if not isinstance(steps, list):
        return False
    for step in steps:
        if not isinstance(step, dict):
            continue
        if step.get("name") == step_name:
            return True
    return False


def find_step(job: dict[str, Any], *, step_id: str | None = None, step_name: str | None = None) -> dict[str, Any] | None:
    steps = job.get("steps")
    if not isinstance(steps, list):
        return None
    for step in steps:
        if not isinstance(step, dict):
            continue
        if step_id is not None and step.get("id") == step_id:
            return step
        if step_name is not None and step.get("name") == step_name:
            return step
    return None


def validate_release_job_shape(path: Path, data: dict[str, Any]) -> list[str]:
    if path.name != "release-gui.yml":
        return []

    errors: list[str] = []
    jobs = data.get("jobs")
    if not isinstance(jobs, dict):
        return [f"{path}: expected jobs to be a mapping"]

    required_jobs = {"resolve_tag", "smoke", "build_package", "publish"}
    missing = sorted(required_jobs - set(jobs))
    if missing:
        errors.append(f"{path}: missing required release jobs: {', '.join(missing)}")
        return errors

    smoke_job = jobs.get("smoke")
    build_job = jobs.get("build_package")
    publish_job = jobs.get("publish")

    if isinstance(smoke_job, dict):
        smoke_needs = normalize_needs(smoke_job.get("needs"))
        if "resolve_tag" not in smoke_needs:
            errors.append(f"{path}: smoke job must need resolve_tag")
    else:
        errors.append(f"{path}: smoke job must be a mapping")

    if isinstance(build_job, dict):
        build_needs = normalize_needs(build_job.get("needs"))
        required_build_needs = {"resolve_tag", "smoke"}
        if not required_build_needs.issubset(build_needs):
            errors.append(f"{path}: build_package job must need resolve_tag and smoke")
        strategy = build_job.get("strategy")
        matrix = strategy.get("matrix") if isinstance(strategy, dict) else None
        include = matrix.get("include") if isinstance(matrix, dict) else None
        expected_matrix = {
            ("windows-latest", "x86_64-pc-windows-msvc"),
            ("ubuntu-22.04", "x86_64-unknown-linux-gnu"),
            ("macos-14", "aarch64-apple-darwin"),
        }
        actual_matrix: set[tuple[str, str]] = set()
        if isinstance(include, list):
            for entry in include:
                if not isinstance(entry, dict):
                    continue
                os_name = entry.get("os")
                target = entry.get("target")
                if isinstance(os_name, str) and isinstance(target, str):
                    actual_matrix.add((os_name, target))
        missing_targets = sorted(expected_matrix - actual_matrix)
        if missing_targets:
            errors.append(
                f"{path}: build_package matrix is missing expected targets: {missing_targets}"
            )
    else:
        errors.append(f"{path}: build_package job must be a mapping")

    if isinstance(publish_job, dict):
        publish_needs = normalize_needs(publish_job.get("needs"))
        required_publish_needs = {"resolve_tag", "build_package"}
        if not required_publish_needs.issubset(publish_needs):
            errors.append(f"{path}: publish job must need resolve_tag and build_package")
        if not has_uses_step(publish_job, "softprops/action-gh-release"):
            errors.append(
                f"{path}: publish job must include softprops/action-gh-release upload step"
            )
    else:
        errors.append(f"{path}: publish job must be a mapping")

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


def validate_release_macos_signing_rules(path: Path, data: dict[str, Any]) -> list[str]:
    if path.name != "release-gui.yml":
        return []

    errors: list[str] = []
    jobs = data.get("jobs")
    if not isinstance(jobs, dict):
        return errors

    build_job = jobs.get("build_package")
    if not isinstance(build_job, dict):
        return errors

    mac_gate_step = find_step(build_job, step_id="mac_signing")
    if mac_gate_step is None:
        errors.append(
            f"{path}: build_package must define a mac signing gate step with id 'mac_signing'"
        )
        return errors

    sign_step = find_step(build_job, step_name="Sign, notarize, and verify macOS artifacts")
    if sign_step is None:
        errors.append(
            f"{path}: build_package must include 'Sign, notarize, and verify macOS artifacts' step"
        )
    else:
        sign_if = sign_step.get("if")
        if not isinstance(sign_if, str) or "steps.mac_signing.outputs.enabled == 'true'" not in sign_if:
            errors.append(
                f"{path}: mac signing step must be gated by steps.mac_signing.outputs.enabled == 'true'"
            )

    for step_name in ("Collect release assets", "Upload release assets"):
        step = find_step(build_job, step_name=step_name)
        if step is None:
            errors.append(f"{path}: missing '{step_name}' step in build_package")
            continue
        condition = step.get("if")
        if not isinstance(condition, str) or "steps.mac_signing.outputs.enabled == 'true'" not in condition:
            errors.append(
                f"{path}: '{step_name}' must be gated by steps.mac_signing.outputs.enabled == 'true'"
            )

    return errors


def validate_workflow_file(path: Path) -> list[str]:
    data, errors = load_yaml(path)
    if data is None:
        return errors

    errors.extend(validate_release_trigger_rules(path, data))
    errors.extend(validate_release_job_shape(path, data))
    errors.extend(validate_release_macos_signing_rules(path, data))

    run_blocks = extract_job_run_blocks(data)
    for block_path, shell, script in run_blocks:
        if is_bash_shell(shell):
            errors.extend(check_bash_syntax(script, f"{path}:{block_path}"))
            errors.extend(check_python_c_snippets(script, f"{path}:{block_path}"))
        if is_pwsh_shell(shell):
            errors.extend(
                check_powershell_command_mode_traps(script, f"{path}:{block_path}")
            )

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
