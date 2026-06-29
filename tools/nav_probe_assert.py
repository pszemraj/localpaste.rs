#!/usr/bin/env python3
"""Assert LocalPaste navigation-probe NDJSON against a compact JSON spec."""

import argparse
import json
import platform
import re
import sys
from pathlib import Path

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

    if args.log is None or args.spec is None:
        parser.error("log and spec are required unless --check-spec is used")

    frames = [
        json.loads(line)
        for line in args.log.read_text("utf-8").splitlines()
        if line.strip()
    ]
    spec = json.loads(args.spec.read_text("utf-8"))
    failures = validate_spec(spec, args.windows_runner)
    failures.extend(validate_scenario_aliases(spec, scenario_aliases))
    summaries = []
    host_platform = args.platform or host()
    scenario_filter = set(args.scenario)

    for scenario in spec.get("scenarios", []):
        if scenario_filter and scenario.get("id") not in scenario_filter:
            continue
        platforms = scenario.get("platforms")
        if platforms and host_platform not in platforms:
            continue
        scenario_id = scenario["id"]
        log_scenario_id = scenario_aliases.get(scenario_id, scenario_id)
        matching = [frame for frame in frames if frame.get("scenario") == log_scenario_id]
        if not matching:
            failures.append(f"{scenario_id}: no frames for log scenario {log_scenario_id!r}")
            continue
        event_frame, state_frame = selected_frames(matching)
        if args.summary:
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
