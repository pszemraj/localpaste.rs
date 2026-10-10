#!/usr/bin/env python3
"""Drive persisted paste scenarios through the real macOS GUI and native keys.

Invoked by nav_probe_run_macos.sh. API calls only create fixtures and read back
state; all editing, selection, focus transfer and pasting use native events.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def wait_for(predicate, description, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {description}")


def main():
    def terminate(signum, _frame):
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

    def driver(command, *rest, pid=None):
        command_args = [str(args.driver), command]
        if pid is not None:
            command_args += ["--pid", str(pid)]
        return subprocess.check_output(command_args + list(map(str, rest)), env=env, text=True)

    def key(code, *modifiers, pid=None):
        driver("key", "--key-code", code, "--modifiers", ",".join(modifiers),
               "--key-delay-ms", args.key_delay_ms, pid=pid or process.pid)
        time.sleep(0.12)

    def native_text(text, pid=None):
        driver("type", "--text", text, pid=pid or process.pid)
        time.sleep(0.2)

    def api(path="/api/pastes", payload=None):
        data = json.dumps(payload).encode() if payload is not None else None
        request = urllib.request.Request(endpoint + path, data=data, headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=2) as response:
            return json.load(response)

    def ready():
        if process.poll() is not None:
            raise RuntimeError("GUI exited before API readiness")
        try:
            api()
            return True
        except (OSError, urllib.error.URLError):
            return False

    def all_pastes():
        return [api("/api/paste/" + row["id"]) for row in api()]

    def start(restart=False, clear_selection=False):
        nonlocal process
        launch_env = dict(env)
        if restart:
            launch_env["LOCALPASTE_NAV_PROBE_SCENARIO"] += "__restart"
        if clear_selection:
            launch_env["LOCALPASTE_NAV_PROBE_CLEAR_SELECTION"] = "1"
        with stdout_path.open("ab") as stdout:
            process = subprocess.Popen([str(args.exe)], env=launch_env, stdout=stdout, stderr=stdout)
        wait_for(ready, "isolated GUI API")
        def native_ready():
            return subprocess.run([str(args.driver), "activate", "--pid", str(process.pid)],
                                  env=env, capture_output=True).returncode == 0
        wait_for(native_ready, "native window activation")
        time.sleep(0.5)

    def stop():
        nonlocal process
        if process is not None:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
            process = None

    def frames():
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

    def snapshot():
        return json.loads(driver("snapshot", pid=process.pid))

    def latest():
        return frames()[-1]

    def capture_result(result):
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
        wait_for(lambda: any(row.get("title") == original["name"] for row in snapshot()), "picker fixture result")
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
                key(chord["key_code"], *chord["modifiers"])
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
        def stable(items):
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
