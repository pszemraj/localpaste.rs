#!/usr/bin/env python3
"""Drive persisted paste scenarios through the real macOS GUI and native keys.

Invoked by nav_probe_run_macos.sh. API calls only create fixtures and read back
state; all editing, selection, focus transfer and pasting use native events.
"""

from __future__ import annotations

import argparse
from collections.abc import Callable
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import time
from types import FrameType
from typing import Any
import urllib.error
import urllib.request


def wait_for(predicate: Callable[[], Any], description: str, timeout: float = 15) -> Any:
    """Poll predicate until it returns a truthy value, or raise RuntimeError after timeout."""
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {description}")


def main() -> None:
    """Run one native paste scenario against an isolated GUI and append its result to the log."""
    def terminate(signum: int, _frame: FrameType | None) -> None:
        """Exit with 128 plus the signal number so the cleanup in finally still runs."""
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, terminate)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", required=True)
    parser.add_argument("--spec", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--key-delay-ms", type=int, default=80)
    parser.add_argument("--capture", action="store_true")
    args = parser.parse_args()
    scenario = next(s for s in json.loads(args.spec.read_text())["scenarios"] if s["id"] == args.scenario)
    config = scenario["driver"]["macos"]["paste"]
    mode = config["mode"]
    clipboard = config["clipboard"]
    scratch = Path(tempfile.mkdtemp(prefix="localpaste-paste-"))
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    endpoint = f"http://127.0.0.1:{port}"
    env = dict(os.environ, DB_PATH=str(scratch / "db"), PORT=str(port), LP_SERVER=endpoint,
               LOCALPASTE_NAV_PROBE_LOG=str(args.log), LOCALPASTE_NAV_PROBE_SCENARIO=args.scenario)
    # A real persisted fixture must not be replaced by the navigation-only seed.
    for key in list(env):
        if key.startswith("LOCALPASTE_NAV_PROBE_SEED_") or key in (
                "LOCALPASTE_NAV_PROBE_FOCUS_EDITOR", "LOCALPASTE_NAV_PROBE_CLEAR_SELECTION"):
            env.pop(key)
    process = None
    safari_pid = None
    saved_clipboard = False
    backup = scratch / "clipboard.json"
    stdout_path = args.log.parent / f"{args.scenario}.stdout.log"

    def driver(command: str, *rest: object, pid: int | None = None) -> str:
        """Run a native driver subcommand, optionally targeting a pid, and return its stdout."""
        command_args = [str(args.driver), command]
        if pid is not None:
            command_args += ["--pid", str(pid)]
        return subprocess.check_output(command_args + list(map(str, rest)), env=env, text=True)

    def key(code: int, *modifiers: str, pid: int | None = None, delay_ms: int = 80) -> None:
        """Press a native chord, holding setup keys reliably unless testing a specific delay."""
        driver("key", "--key-code", code, "--modifiers", ",".join(modifiers),
               "--key-delay-ms", delay_ms, pid=pid or process.pid)
        time.sleep(0.12)

    def native_text(text: str, pid: int | None = None) -> None:
        """Type text natively into the target process (default: the GUI) and pause briefly."""
        driver("type", "--text", text, pid=pid or process.pid)
        time.sleep(0.2)

    def api(path: str = "/api/pastes", payload: dict[str, Any] | None = None) -> Any:
        """Call the isolated GUI API (POST when payload is given) and return the decoded JSON."""
        data = json.dumps(payload).encode() if payload is not None else None
        request = urllib.request.Request(endpoint + path, data=data, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=2) as response:
            return json.load(response)

    def ready() -> bool:
        """Return whether the GUI API answers, raising if the GUI process has already exited."""
        if process.poll() is not None:
            raise RuntimeError("GUI exited before API readiness")
        try:
            api()
            return True
        except (OSError, urllib.error.URLError):
            return False

    def all_pastes() -> list[Any]:
        """Fetch the full record of every stored paste through the API."""
        return [api("/api/paste/" + row["id"]) for row in api()]

    def start(restart: bool = False, clear_selection: bool = False) -> None:
        """Launch the GUI on the isolated database and wait for its API and native window.

        restart logs under the scenario id suffixed with __restart, and clear_selection starts the
        GUI with no paste selected.
        """
        nonlocal process
        launch_env = dict(env)
        if restart:
            launch_env["LOCALPASTE_NAV_PROBE_SCENARIO"] += "__restart"
        if clear_selection:
            launch_env["LOCALPASTE_NAV_PROBE_CLEAR_SELECTION"] = "1"
        with stdout_path.open("ab") as stdout:
            process = subprocess.Popen([str(args.exe)], env=launch_env, stdout=stdout, stderr=stdout)
        wait_for(ready, "isolated GUI API")
        def native_ready() -> bool:
            """Return whether the native driver can activate the GUI window."""
            return subprocess.run([str(args.driver), "activate", "--pid", str(process.pid)],
                                  env=env, capture_output=True).returncode == 0
        wait_for(native_ready, "native window activation")
        time.sleep(0.5)

    def stop() -> None:
        """Terminate the GUI process if one is running and wait for it to exit."""
        nonlocal process
        if process is not None:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
            process = None

    def frames() -> list[dict[str, Any]]:
        """Return the nav_probe_frame records logged for this scenario, skipping partial lines."""
        if not args.log.exists():
            return []
        records = []
        for line in args.log.read_text().splitlines():
            try:
                frame = json.loads(line)
            except json.JSONDecodeError:
                continue  # The GUI may currently be writing its last record.
            if frame.get("scenario") == args.scenario and frame.get("event") == "nav_probe_frame":
                records.append(frame)
        return records

    def snapshot() -> list[dict[str, Any]]:
        """Return the native accessibility snapshot rows of the GUI window."""
        return json.loads(driver("snapshot", pid=process.pid))

    def latest() -> dict[str, Any]:
        """Return the most recent logged frame for this scenario."""
        return frames()[-1]

    def capture_result(result: dict[str, Any]) -> None:
        """Under --capture, write result for a screenshot and wait for the resume marker."""
        if not args.capture:
            return
        capture = args.log.with_suffix(".capture.json")
        resume = args.log.with_suffix(".continue")
        resume.unlink(missing_ok=True)
        capture.write_text(json.dumps({"scenario": args.scenario, "pid": process.pid, **result}, indent=2))
        wait_for(resume.exists, "computer-use screenshot capture", timeout=180)
        capture.unlink()
        resume.unlink()

    try:
        start()
        original = api("/api/paste", {"content": scenario["seed"]["text"], "name": "Paste probe original",
                                      "language": "markdown", "language_is_manual": True, "tags": ["probe"]})
        # Refresh through the picker and open the actual stored body.
        key(40, "command", "shift")
        native_text(original["name"])
        # A picker row's accessible title joins the paste name and its language label.
        fixture_row = f'{original["name"]} markdown'
        wait_for(lambda: any(row.get("title") == fixture_row for row in snapshot()), "picker fixture result")
        key(36)
        wait_for(lambda: latest().get("focus", {}).get("virtual_editor"), "editor focus")
        key(126, "command")
        offset = 9 if mode == "markdown" and config["selection_chars"] == 24 else (0 if mode == "markdown" else 5)
        for _ in range(offset):
            key(124)
        if mode not in ("markdown", "no_open"):
            native_text("X")
        for _ in range(config["selection_chars"]):
            key(124, "shift")
        key(1, "command")  # Save the typed original before testing the paste.
        time.sleep(0.5)
        target = config["target"]
        if target == "title":
            driver("click", "--label", original["name"], pid=process.pid)
        elif target == "search":
            key(3, "command")
        elif target in ("palette", "picker", "help"):
            if target == "help":
                key(122)
            else:
                key(40, "command", *(("shift",) if target == "picker" else ()))
                key(0, "command")  # Replace a retained picker query.
                key(51)
        time.sleep(0.3)

        driver("clipboard-save", "--path", backup)
        saved_clipboard = True
        switched_away = switched_back = False
        if config["external_copy"]:
            safari_pid = int(subprocess.check_output(["pgrep", "-x", "Safari"], text=True).strip())
            driver("activate", pid=safari_pid)
            key(45, "command", pid=safari_pid)  # Own a new Safari window.
            driver("activate", pid=process.pid)
            key(48, "command")  # Cmd+Tab out; verify actual native foreground.
            driver("frontmost", pid=safari_pid)
            switched_away = True
            key(37, "command", pid=safari_pid)
            native_text(clipboard, pid=safari_pid)
            key(0, "command", pid=safari_pid)
            key(8, "command", pid=safari_pid)
            driver("clipboard-check", "--text", clipboard)
            key(48, "command", pid=safari_pid)
            driver("frontmost", pid=process.pid)
            switched_back = True
        else:
            driver("clipboard-set", "--text", clipboard)

        before = latest()
        precondition = None
        if mode == "no_open":
            # Startup normally opens the top row. The gated probe setup clears
            # that selection once, without changing native paste handling.
            stop()
            start(clear_selection=True)
            wait_for(lambda: latest()["app"]["selected_id"] is None
                     and any(row.get("value") == "Select a paste from the sidebar."
                             for row in snapshot()), "native no-open editor")
            before = latest()
            if before["app"]["selected_id"] is not None:
                precondition = "Native startup automatically opens the top sidebar paste; no deselect action is exposed."
            capture_result({"stage": "before_paste", "before_selected": before["app"]["selected_id"],
                            "db_path": env["DB_PATH"], "endpoint": endpoint, "native_ui": snapshot()})
        if precondition is None:
            for chord in scenario["driver"]["macos"]["keys"]:
                key(chord["key_code"], *chord["modifiers"], delay_ms=args.key_delay_ms)
            if mode in ("insert", "markdown", "no_open"):
                key(1, "command")
        time.sleep(1.5)
        ui = snapshot()
        observed = latest()
        rows = all_pastes()
        current_original = next(row for row in rows if row["id"] == original["id"])
        focused_fields = [row.get("value", "") for row in ui if row.get("role") == "AXTextField" and row.get("focused")]
        result = {
            "original_content": current_original["content"], "original_name": current_original["name"],
            "original_tags": current_original["tags"], "count": len(rows),
            "other_contents": sorted(row["content"] for row in rows if row["id"] != original["id"]),
            "editor_focused": observed["focus"]["virtual_editor"],
            "query": focused_fields[0] if focused_fields else None,
            "before_selected": before["app"]["selected_id"],
            "before_editor_focused": before["focus"]["virtual_editor"],
            "native_paste_seen": any(event.get("kind") == "paste" for frame in frames() for event in frame["raw_events"]),
            "switched_away_seen": switched_away, "switched_back_seen": switched_back,
            "precondition_failure": precondition, "db_path": env["DB_PATH"], "endpoint": endpoint,
            "key_delay_ms": args.key_delay_ms, "api_rows": rows, "native_ui": ui,
        }
        capture_result(result)
        if safari_pid is not None:
            key(53, pid=safari_pid)
            key(13, "command", pid=safari_pid)  # Close only the window created above.
            safari_pid = None
        driver("clipboard-restore", "--path", backup, "--text", clipboard)
        saved_clipboard = False
        result["clipboard_restored"] = True
        stop()
        start(restart=True)
        restored = all_pastes()
        # Compare every field except timestamps, which are not content semantics.
        def stable(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
            """Return items sorted by id with the created_at and updated_at timestamps removed."""
            return sorted([{k: v for k, v in item.items() if k not in ("updated_at", "created_at")} for item in items], key=lambda x: x["id"])
        result["restart_equal"] = stable(rows) == stable(restored)
        observed.update(event="nav_probe_paste_result", paste=result)
        with args.log.open("a") as log:
            log.write(json.dumps(observed) + "\n")
        print(f"paste probe: {args.scenario}; count={len(rows)}; restart_equal={result['restart_equal']}"
              + (f"; precondition_failure={precondition}" if precondition else ""), flush=True)
    finally:
        stop()
        if safari_pid is not None:
            key(53, pid=safari_pid)
            key(13, "command", pid=safari_pid)
        if saved_clipboard:
            driver("clipboard-restore", "--path", backup, "--text", clipboard)
        shutil.rmtree(scratch)


if __name__ == "__main__":
    main()
