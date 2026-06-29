#!/usr/bin/env python3
"""Assert LocalPaste navigation-probe NDJSON against a compact JSON spec."""

import argparse
import json
import platform
import re
import sys
from pathlib import Path

WINDOWS_SEND_KEY_RE = re.compile(
    r"^(?:\^)?(?:\+)?\{(?:LEFT|RIGHT|UP|DOWN|HOME|END|PGUP|PGDN|BACKSPACE|DEL)\}$"
)


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


def validate_spec(spec) -> list[str]:
    failures = []
    seen = set()
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
                    if not isinstance(key, str) or not WINDOWS_SEND_KEY_RE.fullmatch(key):
                        failures.append(f"{scenario_id}: unsupported Windows send_keys chord {key!r}")
        if "linux" in (platforms or []):
            linux_keys = driver.get("linux_x11", {}).get("keys")
            if not isinstance(linux_keys, list) or not linux_keys:
                failures.append(f"{scenario_id}: missing driver.linux_x11.keys")
    return failures


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
        "--check-spec",
        action="store_true",
        help="Validate the contract spec without reading a probe log",
    )
    args = parser.parse_args()

    if args.check_spec:
        spec_path = args.spec or args.log
        if spec_path is None:
            parser.error("--check-spec requires a spec path")
        spec = json.loads(spec_path.read_text("utf-8"))
        failures = validate_spec(spec)
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
    failures = validate_spec(spec)
    host_platform = args.platform or host()
    scenario_filter = set(args.scenario)

    for scenario in spec.get("scenarios", []):
        if scenario_filter and scenario.get("id") not in scenario_filter:
            continue
        platforms = scenario.get("platforms")
        if platforms and host_platform not in platforms:
            continue
        scenario_id = scenario["id"]
        matching = [frame for frame in frames if frame.get("scenario") == scenario_id]
        if not matching:
            failures.append(f"{scenario_id}: no frames")
            continue
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

    if failures:
        print("navigation probe assertions failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(f"navigation probe assertions passed for {host_platform}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
