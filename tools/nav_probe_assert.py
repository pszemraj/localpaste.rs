#!/usr/bin/env python3
"""Assert LocalPaste navigation-probe NDJSON against a compact JSON spec."""

import argparse
import json
import platform
import sys
from pathlib import Path


def host() -> str:
    return {"darwin": "macos"}.get(platform.system().lower(), platform.system().lower())


def dotted(value, path):
    cur = value
    for part in path.split("."):
        if not isinstance(cur, dict) or part not in cur:
            raise KeyError(path)
        cur = cur[part]
    return cur


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=Path)
    parser.add_argument("spec", type=Path)
    args = parser.parse_args()

    frames = [
        json.loads(line)
        for line in args.log.read_text("utf-8").splitlines()
        if line.strip()
    ]
    spec = json.loads(args.spec.read_text("utf-8"))
    failures = []
    host_platform = host()

    for scenario in spec.get("scenarios", []):
        platforms = scenario.get("platforms")
        if platforms and host_platform not in platforms:
            continue
        scenario_id = scenario["id"]
        matching = [frame for frame in frames if frame.get("scenario") == scenario_id]
        if not matching:
            failures.append(f"{scenario_id}: no frames")
            continue
        applied = [frame for frame in matching if frame.get("applied_commands")]
        candidates = [
            frame
            for frame in matching
            if frame.get("candidate_commands_if_editor_focused")
        ]
        raw = [frame for frame in matching if frame.get("raw_events")]
        if applied:
            frame = applied[-1]
        elif candidates:
            frame = candidates[-1]
        elif raw:
            frame = raw[-1]
        else:
            frame = matching[-1]
        for key, expected in scenario.get("expect", {}).items():
            try:
                actual = dotted(frame, key)
            except KeyError:
                failures.append(f"{scenario_id}: missing {key}")
                continue
            if actual != expected:
                failures.append(f"{scenario_id}: {key} expected {expected!r}, got {actual!r}")
        for key, expected_part in scenario.get("expect_contains", {}).items():
            try:
                actual = dotted(frame, key)
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

    if failures:
        print("navigation probe assertions failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print(f"navigation probe assertions passed for {host_platform}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
