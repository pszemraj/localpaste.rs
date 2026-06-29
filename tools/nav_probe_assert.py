#!/usr/bin/env python3
"""Assert LocalPaste navigation-probe NDJSON against a compact JSON spec."""

import argparse
import json
import platform
import re
import sys
from pathlib import Path, PureWindowsPath

WINDOWS_SEND_KEY_NAMES = (
    "LEFT",
    "RIGHT",
    "UP",
    "DOWN",
    "HOME",
    "END",
    "PGUP",
    "PGDN",
    "BACKSPACE",
    "DEL",
)
WINDOWS_SEND_KEY_RE = re.compile(
    rf"^(?:\^)?(?:\+)?\{{({'|'.join(WINDOWS_SEND_KEY_NAMES)})\}}$"
)
WINDOWS_RUNNER_SWITCH_RE = re.compile(r'^\s*"([A-Z0-9_]+)"\s*\{', re.MULTILINE)


def host() -> str:
    return {"darwin": "macos"}.get(platform.system().lower(), platform.system().lower())


def dotted(value, path):
    cur = value
    for part in path.split("."):
        if not isinstance(cur, dict) or part not in cur:
            raise KeyError(path)
        cur = cur[part]
    return cur


def matches_subset(value, expected) -> bool:
    for key, expected_value in expected.items():
        try:
            actual = dotted(value, key)
        except KeyError:
            return False
        if actual != expected_value:
            return False
    return True


def selected_frames(matching):
    state_frame = matching[-1]
    applied = [frame for frame in matching if frame.get("applied_commands")]
    candidates = [
        frame
        for frame in matching
        if frame.get("candidate_commands_if_editor_focused")
    ]
    raw = [frame for frame in matching if frame.get("raw_events")]
    if applied:
        event_frame = applied[-1]
    elif candidates:
        event_frame = candidates[-1]
    elif raw:
        event_frame = raw[-1]
    else:
        event_frame = state_frame
    return event_frame, state_frame


def modifier_names(modifiers) -> list[str]:
    if not isinstance(modifiers, dict):
        return []
    names = []
    for key in ("ctrl", "shift", "alt", "command", "mac_cmd"):
        if modifiers.get(key):
            names.append(key)
    return names


def key_event_summary(frame) -> str:
    raw_events = frame.get("raw_events")
    if not isinstance(raw_events, list):
        return "event=?"
    pressed_keys = [
        event
        for event in raw_events
        if event.get("kind") == "key" and event.get("pressed") is True
    ]
    if not pressed_keys:
        return "event=?"
    parts = []
    for event in pressed_keys:
        modifiers = "+".join(modifier_names(event.get("modifiers")))
        key = event.get("key") or "?"
        parts.append(f"{modifiers}+{key}" if modifiers else str(key))
    return "event=" + ",".join(parts)


def focus_summary(frame) -> str:
    focus = frame.get("focus")
    if not isinstance(focus, dict):
        return "focus=?"
    owners = [
        key
        for key in (
            "virtual_editor",
            "sidebar_search",
            "editor_title",
            "command_palette_query",
            "properties_name",
            "properties_tags",
            "diff_query",
        )
        if focus.get(key)
    ]
    return "focus=" + ("|".join(owners) if owners else "none")


def cursor_summary(frame) -> str:
    cursor = frame.get("cursor")
    if not isinstance(cursor, dict):
        return "cursor=?"
    return "cursor={}:{}@{}/{}".format(
        cursor.get("line", "?"),
        cursor.get("col", "?"),
        cursor.get("char_index", "?"),
        cursor.get("buffer_len_chars", "?"),
    )


def selection_summary(frame) -> str:
    selection = frame.get("selection")
    if selection is None:
        return "selection=null"
    if not isinstance(selection, dict):
        return "selection=?"
    return "selection={}..{}".format(selection.get("start", "?"), selection.get("end", "?"))


def command_summary(frame, key: str) -> str:
    commands = frame.get(key)
    if not isinstance(commands, list):
        return f"{key}=?"
    if not commands:
        return f"{key}=none"
    return f"{key}=" + ";".join(str(command) for command in commands)


def scenario_summary(scenario_id: str, event_frame, state_frame) -> str:
    return " | ".join(
        [
            scenario_id,
            key_event_summary(event_frame),
            command_summary(event_frame, "candidate_commands_if_editor_focused"),
            command_summary(event_frame, "applied_commands"),
            focus_summary(state_frame),
            cursor_summary(state_frame),
            selection_summary(state_frame),
        ]
    )


def parse_scenario_aliases(values: list[str]) -> dict[str, str]:
    aliases = {}
    for value in values:
        if "=" not in value:
            raise ValueError(f"scenario alias must use SPEC_ID=LOG_ID, got {value!r}")
        spec_id, log_id = value.split("=", 1)
        spec_id = spec_id.strip()
        log_id = log_id.strip()
        if not spec_id or not log_id:
            raise ValueError(f"scenario alias must use non-empty SPEC_ID=LOG_ID, got {value!r}")
        if spec_id in aliases:
            raise ValueError(f"duplicate scenario alias for {spec_id!r}")
        aliases[spec_id] = log_id
    return aliases


def windows_send_key_name(chord: str) -> str | None:
    match = WINDOWS_SEND_KEY_RE.fullmatch(chord)
    if match is None:
        return None
    return match.group(1)


def windows_runner_supported_keys(path: Path) -> set[str]:
    return set(WINDOWS_RUNNER_SWITCH_RE.findall(path.read_text("utf-8")))


def validate_spec(spec, windows_runner: Path | None = None) -> list[str]:
    failures = []
    seen = set()
    used_windows_keys = set()
    for index, scenario in enumerate(spec.get("scenarios", [])):
        scenario_id = scenario.get("id")
        if not scenario_id:
            failures.append(f"scenario[{index}]: missing id")
            continue
        if scenario_id in seen:
            failures.append(f"{scenario_id}: duplicate scenario id")
        seen.add(scenario_id)
        platforms = scenario.get("platforms")
        if not isinstance(platforms, list) or not platforms:
            failures.append(f"{scenario_id}: platforms must be a non-empty list")
        driver = scenario.get("driver", {})
        if "windows" in (platforms or []):
            send_keys = driver.get("windows", {}).get("send_keys")
            if not isinstance(send_keys, list) or not send_keys:
                failures.append(f"{scenario_id}: missing driver.windows.send_keys")
            else:
                for key in send_keys:
                    if not isinstance(key, str):
                        failures.append(f"{scenario_id}: unsupported Windows send_keys chord {key!r}")
                        continue
                    key_name = windows_send_key_name(key)
                    if key_name is None:
                        failures.append(f"{scenario_id}: unsupported Windows send_keys chord {key!r}")
                        continue
                    used_windows_keys.add(key_name)
        if "linux" in (platforms or []):
            linux_keys = driver.get("linux_x11", {}).get("keys")
            if not isinstance(linux_keys, list) or not linux_keys:
                failures.append(f"{scenario_id}: missing driver.linux_x11.keys")
    if windows_runner is not None:
        supported_windows_keys = windows_runner_supported_keys(windows_runner)
        missing = used_windows_keys - supported_windows_keys
        for key_name in sorted(missing):
            failures.append(
                f"{windows_runner}: Send-NavChord does not implement contract key {key_name}"
            )
    return failures


def validate_scenario_aliases(spec, aliases: dict[str, str]) -> list[str]:
    scenario_ids = {
        scenario.get("id")
        for scenario in spec.get("scenarios", [])
        if isinstance(scenario.get("id"), str)
    }
    return [
        f"scenario alias references unknown spec scenario {scenario_id!r}"
        for scenario_id in sorted(set(aliases) - scenario_ids)
    ]


def validate_manifest(manifest) -> list[str]:
    failures = []
    if not isinstance(manifest, dict):
        return ["manifest must be a JSON object"]
    if manifest.get("event") != "nav_probe_windows_run":
        failures.append("manifest.event must be 'nav_probe_windows_run'")
    if manifest.get("platform") != "windows":
        failures.append("manifest.platform must be 'windows'")
    if manifest.get("status") != "assertions_passed":
        failures.append("manifest.status must be 'assertions_passed'")
    if manifest.get("assert_enabled") is not True:
        failures.append("manifest.assert_enabled must be true")
    shutdown_mode = manifest.get("shutdown_mode")
    if shutdown_mode is not None and shutdown_mode not in {"Kill", "CloseMainWindow"}:
        failures.append("manifest.shutdown_mode must be 'Kill' or 'CloseMainWindow'")
    repeat_count = manifest.get("repeat_count")
    if not isinstance(repeat_count, int) or repeat_count < 1:
        failures.append("manifest.repeat_count must be a positive integer")
    scenario_count = manifest.get("scenario_count")
    if not isinstance(scenario_count, int) or scenario_count < 1:
        failures.append("manifest.scenario_count must be a positive integer")
    runs = manifest.get("runs")
    if not isinstance(runs, list) or not runs:
        failures.append("manifest.runs must be a non-empty list")
        return failures
    run_count = manifest.get("run_count")
    if run_count != len(runs):
        failures.append(f"manifest.run_count expected {len(runs)!r}, got {run_count!r}")
    if isinstance(repeat_count, int) and isinstance(scenario_count, int):
        expected_run_count = repeat_count * scenario_count
        if len(runs) != expected_run_count:
            failures.append(
                f"manifest.runs expected {expected_run_count} entries, got {len(runs)}"
            )
    seen_log_ids = set()
    for index, run in enumerate(runs):
        if not isinstance(run, dict):
            failures.append(f"manifest.runs[{index}] must be an object")
            continue
        scenario_id = run.get("scenario_id")
        log_scenario_id = run.get("log_scenario_id")
        repeat_index = run.get("repeat_index")
        if not isinstance(scenario_id, str) or not scenario_id:
            failures.append(f"manifest.runs[{index}].scenario_id must be a non-empty string")
        if not isinstance(log_scenario_id, str) or not log_scenario_id:
            failures.append(f"manifest.runs[{index}].log_scenario_id must be a non-empty string")
        elif log_scenario_id in seen_log_ids:
            failures.append(f"manifest log scenario {log_scenario_id!r} is duplicated")
        else:
            seen_log_ids.add(log_scenario_id)
        if not isinstance(repeat_index, int) or repeat_index < 1:
            failures.append(f"manifest.runs[{index}].repeat_index must be a positive integer")
        elif isinstance(repeat_count, int) and repeat_index > repeat_count:
            failures.append(
                f"manifest.runs[{index}].repeat_index {repeat_index} exceeds repeat_count {repeat_count}"
            )
    if manifest.get("current_run") is not None:
        failures.append("manifest.current_run must be null for a successful run")
    completed_runs = manifest.get("completed_runs")
    if not isinstance(completed_runs, list):
        failures.append("manifest.completed_runs must be a list")
    else:
        expected = {
            (
                run.get("scenario_id"),
                run.get("log_scenario_id"),
                run.get("repeat_index"),
            )
            for run in runs
            if isinstance(run, dict)
        }
        actual = {
            (
                run.get("scenario_id"),
                run.get("log_scenario_id"),
                run.get("repeat_index"),
            )
            for run in completed_runs
            if isinstance(run, dict)
        }
        if len(completed_runs) != len(runs) or actual != expected:
            failures.append("manifest.completed_runs must exactly match manifest.runs")
    return failures


def manifest_path_candidates(raw_path: str, manifest_path: Path) -> list[Path]:
    candidates = [Path(raw_path)]
    for name in {Path(raw_path).name, PureWindowsPath(raw_path).name}:
        if name:
            candidates.append(manifest_path.parent / name)
    windows_parts = PureWindowsPath(raw_path).parts
    for marker in ("docs", "target"):
        if marker in windows_parts:
            candidates.append(Path(*windows_parts[windows_parts.index(marker):]))
    deduped = []
    seen = set()
    for candidate in candidates:
        key = str(candidate)
        if key not in seen:
            deduped.append(candidate)
            seen.add(key)
    return deduped


def resolve_manifest_path(manifest, key: str, manifest_path: Path, override: Path | None):
    if override is not None:
        return override, []
    raw_path = manifest.get(key)
    if not isinstance(raw_path, str) or not raw_path.strip():
        return None, [f"manifest.{key} must be a non-empty string"]
    candidates = manifest_path_candidates(raw_path, manifest_path)
    for candidate in candidates:
        if candidate.exists():
            return candidate, []
    joined = ", ".join(str(candidate) for candidate in candidates)
    return None, [f"manifest.{key} does not exist; tried {joined}"]


def assert_scenarios(
    frames,
    spec,
    host_platform: str,
    scenario_filter: set[str],
    scenario_aliases: dict[str, str],
    include_summary: bool,
):
    failures = []
    summaries = []
    matched_scenarios = set()
    for scenario in spec.get("scenarios", []):
        if scenario_filter and scenario.get("id") not in scenario_filter:
            continue
        platforms = scenario.get("platforms")
        if platforms and host_platform not in platforms:
            continue
        scenario_id = scenario["id"]
        matched_scenarios.add(scenario_id)
        log_scenario_id = scenario_aliases.get(scenario_id, scenario_id)
        matching = [frame for frame in frames if frame.get("scenario") == log_scenario_id]
        if not matching:
            failures.append(f"{scenario_id}: no frames for log scenario {log_scenario_id!r}")
            continue
        event_frame, state_frame = selected_frames(matching)
        if include_summary:
            summary_id = (
                scenario_id
                if log_scenario_id == scenario_id
                else f"{scenario_id} as {log_scenario_id}"
            )
            summaries.append(scenario_summary(summary_id, event_frame, state_frame))
        for key, expected in scenario.get("expect", {}).items():
            try:
                actual = dotted(state_frame, key)
            except KeyError:
                failures.append(f"{scenario_id}: missing {key}")
                continue
            if actual != expected:
                failures.append(f"{scenario_id}: {key} expected {expected!r}, got {actual!r}")
        for key, expected_part in scenario.get("expect_contains", {}).items():
            try:
                actual = dotted(event_frame, key)
            except KeyError:
                failures.append(f"{scenario_id}: missing {key}")
                continue
            if not (
                isinstance(actual, list)
                and any(str(expected_part) in str(item) for item in actual)
            ):
                failures.append(
                    f"{scenario_id}: {key} did not contain {expected_part!r}; got {actual!r}"
                )
        for expected_event in scenario.get("expect_events", []):
            actual_events = event_frame.get("raw_events")
            if not isinstance(actual_events, list):
                failures.append(f"{scenario_id}: raw_events is not a list")
                continue
            if not any(matches_subset(event, expected_event) for event in actual_events):
                failures.append(
                    f"{scenario_id}: raw_events did not contain event subset {expected_event!r}; got {actual_events!r}"
                )
    if scenario_filter and not matched_scenarios:
        requested = ", ".join(sorted(str(scenario_id) for scenario_id in scenario_filter))
        failures.append(f"no spec scenarios matched selection: {requested}")
    return failures, summaries


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=Path, nargs="?")
    parser.add_argument("spec", type=Path, nargs="?")
    parser.add_argument("--platform", default=None, help="Override detected host platform")
    parser.add_argument(
        "--scenario",
        action="append",
        default=[],
        help="Only assert the named scenario; may be repeated",
    )
    parser.add_argument(
        "--scenario-alias",
        action="append",
        default=[],
        help="Map a spec scenario id to a different log scenario label; format SPEC_ID=LOG_ID",
    )
    parser.add_argument(
        "--check-spec",
        action="store_true",
        help="Validate the contract spec without reading a probe log",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        help="Assert all runs listed in a Windows probe manifest",
    )
    parser.add_argument(
        "--summary",
        action="store_true",
        help="Print compact per-scenario evidence lines after selecting assertion frames",
    )
    parser.add_argument(
        "--windows-runner",
        type=Path,
        help="Validate Windows send_keys against a nav_probe_run_windows.ps1 Send-NavChord switch",
    )
    args = parser.parse_args()
    try:
        scenario_aliases = parse_scenario_aliases(args.scenario_alias)
    except ValueError as error:
        parser.error(str(error))

    if args.check_spec:
        spec_path = args.spec or args.log
        if spec_path is None:
            parser.error("--check-spec requires a spec path")
        spec = json.loads(spec_path.read_text("utf-8"))
        failures = validate_spec(spec, args.windows_runner)
        failures.extend(validate_scenario_aliases(spec, scenario_aliases))
        if failures:
            print("navigation probe spec validation failed:", file=sys.stderr)
            for failure in failures:
                print(f"- {failure}", file=sys.stderr)
            return 1
        print("navigation probe spec validation passed")
        return 0

    manifest = None
    manifest_failures = []
    if args.manifest is not None:
        manifest = json.loads(args.manifest.read_text("utf-8"))
        manifest_failures = validate_manifest(manifest)
        if not isinstance(manifest, dict):
            print("navigation probe manifest validation failed:", file=sys.stderr)
            for failure in manifest_failures:
                print(f"- {failure}", file=sys.stderr)
            return 1

    if manifest is None and (args.log is None or args.spec is None):
        parser.error("log and spec are required unless --check-spec is used")

    if manifest is not None:
        log_path, path_failures = resolve_manifest_path(manifest, "log_path", args.manifest, args.log)
        manifest_failures.extend(path_failures)
        spec_path, path_failures = resolve_manifest_path(manifest, "spec_path", args.manifest, args.spec)
        manifest_failures.extend(path_failures)
        if log_path is None or spec_path is None:
            print("navigation probe manifest validation failed:", file=sys.stderr)
            for failure in manifest_failures:
                print(f"- {failure}", file=sys.stderr)
            return 1
    else:
        log_path = args.log
        spec_path = args.spec

    frames = [
        json.loads(line)
        for line in log_path.read_text("utf-8").splitlines()
        if line.strip()
    ]
    spec = json.loads(spec_path.read_text("utf-8"))
    failures = validate_spec(spec, args.windows_runner)
    failures.extend(validate_scenario_aliases(spec, scenario_aliases))
    failures.extend(manifest_failures)
    summaries = []
    host_platform = args.platform or (manifest.get("platform") if manifest else None) or host()
    scenario_filter = set(args.scenario)

    if manifest is not None:
        selected_runs = [
            run
            for run in manifest.get("runs", [])
            if not scenario_filter
            or run.get("scenario_id") in scenario_filter
            or run.get("log_scenario_id") in scenario_filter
        ]
        if not selected_runs:
            failures.append("manifest selection matched no runs")
        for run in selected_runs:
            run_scenario_id = run.get("scenario_id")
            run_log_scenario_id = run.get("log_scenario_id")
            run_failures, run_summaries = assert_scenarios(
                frames,
                spec,
                host_platform,
                {run_scenario_id},
                {run_scenario_id: run_log_scenario_id},
                args.summary,
            )
            failures.extend(run_failures)
            summaries.extend(run_summaries)
    else:
        run_failures, run_summaries = assert_scenarios(
            frames,
            spec,
            host_platform,
            scenario_filter,
            scenario_aliases,
            args.summary,
        )
        failures.extend(run_failures)
        summaries.extend(run_summaries)

    if args.summary:
        print("navigation probe summary:")
        for summary in summaries:
            print(f"- {summary}")

    if failures:
        print("navigation probe assertions failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(f"navigation probe assertions passed for {host_platform}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
