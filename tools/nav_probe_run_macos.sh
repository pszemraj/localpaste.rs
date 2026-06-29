#!/usr/bin/env bash
set -euo pipefail

spec="docs/dev/nav_contract.json"
log=""
build=0
assert=0
list=0
summary=0
launch_poll_ms=100
launch_poll_count=400
after_focus_ms=150
between_keys_ms=350
after_scenario_ms=900
after_close_ms=500
only=()

usage() {
    cat <<'EOF'
Usage: tools/nav_probe_run_macos.sh [options]

Prerequisites:
  macOS with Accessibility permission granted to the terminal running this
  script. Native key injection uses osascript/System Events.

Options:
  --spec PATH                 Navigation contract JSON path
  --log PATH                  Probe NDJSON path; defaults to a unique target/ file
  --only SCENARIO             Run one scenario; may be repeated
  --list                      List selected macOS scenarios and exit
  --summary                   Print compact per-scenario evidence after assertions
  --build                     Build localpaste-gui before running
  --assert                    Run tools/nav_probe_assert.py after the probe run
  --launch-poll-ms MS         Poll interval while waiting for app/probe readiness
  --launch-poll-count COUNT   Poll count while waiting for app/probe readiness
  --after-focus-ms MS         Delay after final app activation before sending keys
  --between-keys-ms MS        Delay between keys inside a scenario
  --after-scenario-ms MS      Delay after keys before closing the app
  --after-close-ms MS         Delay between scenario processes

Environment:
  LOCALPASTE_NAV_PROBE_PYTHON  Python executable to use for JSON parsing; otherwise inherits an active venv/conda env, then falls back to python3/python
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --spec)
            spec="${2:?missing --spec value}"
            shift 2
            ;;
        --log)
            log="${2:?missing --log value}"
            shift 2
            ;;
        --only)
            only+=("${2:?missing --only value}")
            shift 2
            ;;
        --build)
            build=1
            shift
            ;;
        --list)
            list=1
            shift
            ;;
        --summary)
            summary=1
            shift
            ;;
        --assert)
            assert=1
            shift
            ;;
        --launch-poll-ms)
            launch_poll_ms="${2:?missing --launch-poll-ms value}"
            shift 2
            ;;
        --launch-poll-count)
            launch_poll_count="${2:?missing --launch-poll-count value}"
            shift 2
            ;;
        --after-focus-ms)
            after_focus_ms="${2:?missing --after-focus-ms value}"
            shift 2
            ;;
        --between-keys-ms)
            between_keys_ms="${2:?missing --between-keys-ms value}"
            shift 2
            ;;
        --after-scenario-ms)
            after_scenario_ms="${2:?missing --after-scenario-ms value}"
            shift 2
            ;;
        --after-close-ms)
            after_close_ms="${2:?missing --after-close-ms value}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

assert_positive_int() {
    local name="$1"
    local value="$2"
    if [[ ! "$value" =~ ^[0-9]+$ ]]; then
        echo "$name must be a positive integer; got $value" >&2
        exit 2
    fi
    if (( value < 1 )); then
        echo "$name must be a positive integer; got $value" >&2
        exit 2
    fi
}

assert_non_negative_int() {
    local name="$1"
    local value="$2"
    if [[ ! "$value" =~ ^[0-9]+$ ]]; then
        echo "$name must be a non-negative integer; got $value" >&2
        exit 2
    fi
}

assert_non_negative_int "--launch-poll-ms" "$launch_poll_ms"
assert_positive_int "--launch-poll-count" "$launch_poll_count"
assert_non_negative_int "--after-focus-ms" "$after_focus_ms"
assert_non_negative_int "--between-keys-ms" "$between_keys_ms"
assert_non_negative_int "--after-scenario-ms" "$after_scenario_ms"
assert_non_negative_int "--after-close-ms" "$after_close_ms"

repo="$(pwd -P)"

python_cmd=()
if [[ -n "${LOCALPASTE_NAV_PROBE_PYTHON:-}" ]]; then
    python_cmd=("$LOCALPASTE_NAV_PROBE_PYTHON")
elif [[ -n "${VIRTUAL_ENV:-}" && -x "${VIRTUAL_ENV}/bin/python" ]]; then
    python_cmd=("${VIRTUAL_ENV}/bin/python")
elif [[ -n "${CONDA_PREFIX:-}" && -x "${CONDA_PREFIX}/bin/python" ]]; then
    python_cmd=("${CONDA_PREFIX}/bin/python")
elif command -v python3 >/dev/null 2>&1; then
    python_cmd=(python3)
elif command -v python >/dev/null 2>&1; then
    python_cmd=(python)
else
    echo "required command not found: python3 or python" >&2
    exit 2
fi

run_python() {
    "${python_cmd[@]}" "$@"
}

if [[ -z "$log" ]]; then
    if command -v uuidgen >/dev/null 2>&1; then
        run_id="$(uuidgen | tr '[:upper:]' '[:lower:]' | tr -d '-')"
    elif [[ -r /proc/sys/kernel/random/uuid ]]; then
        run_id="$(tr -d '-' < /proc/sys/kernel/random/uuid)"
    else
        run_id="$(date +%s%N)"
    fi
    log="target/nav-probe-macos-${run_id}.ndjson"
fi

absolute_path() {
    run_python - "$1" <<'PY'
import sys
from pathlib import Path

print(Path(sys.argv[1]).expanduser().resolve(strict=False))
PY
}

assert_repo_path() {
    local path="$1"
    local label="$2"
    local absolute
    absolute="$(absolute_path "$path")"
    if [[ "$absolute" != "$repo" && "$absolute" != "$repo"/* ]]; then
        echo "$label must stay inside repository root: $absolute" >&2
        exit 2
    fi
    printf '%s\n' "$absolute"
}

ms_sleep() {
    local ms="$1"
    run_python - "$ms" <<'PY'
import sys
import time

time.sleep(max(0, int(sys.argv[1])) / 1000.0)
PY
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "required command not found: $1" >&2
        exit 2
    fi
}

spec_path="$(assert_repo_path "$spec" "Spec")"
log_path="$(assert_repo_path "$log" "Log")"
exe="$(assert_repo_path "target/debug/localpaste-gui" "Executable")"

if [[ "${#only[@]}" -gt 0 ]]; then
    run_python - "$spec_path" "${only[@]}" <<'PY'
import json
import sys
from pathlib import Path

spec = json.loads(Path(sys.argv[1]).read_text("utf-8"))
only = set(sys.argv[2:])
known = {
    scenario.get("id")
    for scenario in spec.get("scenarios", [])
    if "macos" in scenario.get("platforms", [])
    and scenario.get("driver", {}).get("macos")
}
missing = sorted(only - known)
if missing:
    print(f"unknown macOS scenario(s): {', '.join(missing)}", file=sys.stderr)
    raise SystemExit(2)
PY
fi

mapfile -t scenarios < <(
    run_python - "$spec_path" "${only[@]}" <<'PY'
import json
import sys
from pathlib import Path

spec = json.loads(Path(sys.argv[1]).read_text("utf-8"))
only = set(sys.argv[2:])
for scenario in spec.get("scenarios", []):
    if only and scenario.get("id") not in only:
        continue
    if "macos" not in scenario.get("platforms", []):
        continue
    macos = scenario.get("driver", {}).get("macos")
    if not macos:
        continue
    seed = scenario.get("seed", {})
    text = seed.get("text", "alpha beta\ngamma delta\nepsilon zeta\n")
    print(json.dumps({
        "id": scenario["id"],
        "seed_text": text,
        "seed_cursor": seed.get("cursor"),
        "seed_len": len(text),
        "keys": macos.get("keys", []),
    }))
PY
)

if [[ "${#scenarios[@]}" -eq 0 ]]; then
    echo "No macOS scenarios found in $spec_path" >&2
    exit 2
fi

scenario_args=()
for scenario_json in "${scenarios[@]}"; do
    scenario_args+=(--scenario "$(run_python - "$scenario_json" <<'PY'
import json
import sys

print(json.loads(sys.argv[1])["id"])
PY
)")
done

if [[ "$list" -eq 1 ]]; then
    for scenario_json in "${scenarios[@]}"; do
        run_python - "$scenario_json" <<'PY'
import json
import sys

print(json.loads(sys.argv[1])["id"])
PY
    done
    exit 0
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "macOS navigation automation requires Darwin; use --list for scenario discovery on other hosts." >&2
    exit 2
fi
require_command osascript

if [[ "$build" -eq 1 ]]; then
    cargo build -p localpaste_gui --bin localpaste-gui
fi
if [[ ! -x "$exe" ]]; then
    echo "GUI executable not found: $exe. Run with --build or build it first." >&2
    exit 2
fi
mkdir -p "$(dirname "$log_path")"
rm -f "$log_path"

safe_name() {
    printf '%s' "$1" | sed -E 's/[^A-Za-z0-9_.-]/_/g'
}

json_field() {
    local json="$1"
    local field="$2"
    run_python - "$json" "$field" <<'PY'
import json
import sys

value = json.loads(sys.argv[1]).get(sys.argv[2])
if value is None:
    raise SystemExit(1)
if isinstance(value, list):
    for item in value:
        if isinstance(item, (dict, list)):
            print(json.dumps(item, separators=(",", ":")))
        else:
            print(item)
else:
    print(value)
PY
}

json_text_field() {
    local json="$1"
    local field="$2"
    run_python - "$json" "$field" <<'PY'
import json
import sys

value = json.loads(sys.argv[1]).get(sys.argv[2])
if value is None:
    raise SystemExit(1)
sys.stdout.write(str(value))
sys.stdout.write("__LOCALPASTE_NAV_PROBE_SENTINEL__")
PY
}

join_csv() {
    local joined=""
    local item
    for item in "$@"; do
        if [[ -n "$joined" ]]; then
            joined+=", "
        fi
        joined+="$item"
    done
    printf '%s' "$joined"
}

probe_ready() {
    local scenario_id="$1"
    local expected_len="$2"
    [[ -f "$log_path" ]] || return 1
    run_python - "$log_path" "$scenario_id" "$expected_len" <<'PY'
import json
import sys
from pathlib import Path

log = Path(sys.argv[1])
scenario = sys.argv[2]
expected_len = int(sys.argv[3])
try:
    lines = log.read_text("utf-8").splitlines()[-80:]
except FileNotFoundError:
    raise SystemExit(1)
for line in reversed(lines):
    if not line.strip():
        continue
    try:
        frame = json.loads(line)
    except json.JSONDecodeError:
        continue
    if frame.get("scenario") != scenario:
        continue
    app = frame.get("app", {})
    cursor = frame.get("cursor", {})
    focus = frame.get("focus", {})
    has_seed = app.get("selected_id") == "__nav_probe__" and cursor.get("buffer_len_chars") == expected_len
    has_keyboard_focus = focus.get("virtual_editor") is True and focus.get("wants_keyboard_input") is True
    if has_seed and has_keyboard_focus:
        raise SystemExit(0)
    raise SystemExit(1)
raise SystemExit(1)
PY
}

process_exists() {
    local pid="$1"
    osascript - "$pid" <<'APPLESCRIPT' >/dev/null
on run argv
    set targetPid to item 1 of argv as integer
    tell application "System Events"
        set matches to every process whose unix id is targetPid
        if (count of matches) is 0 then error "process not found"
    end tell
end run
APPLESCRIPT
}

activate_app() {
    local pid="$1"
    osascript - "$pid" <<'APPLESCRIPT' >/dev/null
on run argv
    set targetPid to item 1 of argv as integer
    tell application "System Events"
        set matches to every process whose unix id is targetPid
        if (count of matches) is 0 then error "process not found"
        set frontmost of item 1 of matches to true
    end tell
end run
APPLESCRIPT
}

wait_active_app() {
    local pid="$1"
    local scenario_id="$2"
    for _ in $(seq 1 "$launch_poll_count"); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
            wait "$pid" || true
            echo "localpaste-gui exited before activation for $scenario_id" >&2
            return 1
        fi
        if activate_app "$pid"; then
            return 0
        fi
        ms_sleep "$launch_poll_ms"
    done
    echo "localpaste-gui did not become activatable before input for $scenario_id" >&2
    return 1
}

send_macos_key() {
    local key_json="$1"
    local key_code
    key_code="$(json_field "$key_json" key_code)"
    if [[ ! "$key_code" =~ ^[0-9]+$ ]]; then
        echo "invalid macOS key_code: $key_code" >&2
        return 2
    fi
    mapfile -t modifiers < <(json_field "$key_json" modifiers || true)
    local clauses=()
    local modifier
    for modifier in "${modifiers[@]}"; do
        case "$modifier" in
            command) clauses+=("command down") ;;
            option) clauses+=("option down") ;;
            shift) clauses+=("shift down") ;;
            control) clauses+=("control down") ;;
            "") ;;
            *)
                echo "unsupported macOS modifier: $modifier" >&2
                return 2
                ;;
        esac
    done
    local using_clause=""
    if [[ "${#clauses[@]}" -gt 0 ]]; then
        local joined
        joined="$(join_csv "${clauses[@]}")"
        using_clause=" using {$joined}"
    fi
    osascript <<APPLESCRIPT >/dev/null
tell application "System Events"
    key code $key_code$using_clause
end tell
APPLESCRIPT
}

close_app() {
    local pid="$1"
    if ! kill -0 "$pid" >/dev/null 2>&1; then
        wait "$pid" || true
        return 0
    fi
    # Probe runs use a disposable DB and need deterministic teardown. Window
    # close events can race with winit after the evidence frame has been
    # written, so default to terminating only this scenario's child process.
    kill "$pid" >/dev/null 2>&1 || true
    for _ in $(seq 1 50); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
            wait "$pid" || true
            return 0
        fi
        sleep 0.1
    done
    kill -KILL "$pid" >/dev/null 2>&1 || true
    wait "$pid" || true
}

current_pid=""
cleanup_running_app() {
    if [[ -n "$current_pid" ]]; then
        close_app "$current_pid"
        current_pid=""
    fi
}
cleanup_and_exit() {
    local code="$1"
    cleanup_running_app
    trap - EXIT INT TERM
    exit "$code"
}
trap cleanup_running_app EXIT
trap 'cleanup_and_exit 130' INT
trap 'cleanup_and_exit 143' TERM

for scenario_json in "${scenarios[@]}"; do
    scenario_id="$(json_field "$scenario_json" id)"
    seed_text_raw="$(json_text_field "$scenario_json" seed_text)"
    seed_text="${seed_text_raw%__LOCALPASTE_NAV_PROBE_SENTINEL__}"
    seed_len="$(json_field "$scenario_json" seed_len)"
    seed_cursor="$(json_field "$scenario_json" seed_cursor || true)"
    mapfile -t keys < <(json_field "$scenario_json" keys)
    safe_scenario="$(safe_name "$scenario_id")"
    db_path="$(assert_repo_path "target/nav-probe-db-${safe_scenario}-$(date +%s%N)" "DB_PATH")"
    mkdir -p "$db_path"

    echo "nav probe: $scenario_id"
    env_args=(
        "DB_PATH=$db_path"
        "LOCALPASTE_NAV_PROBE_LOG=$log_path"
        "LOCALPASTE_NAV_PROBE_SCENARIO=$scenario_id"
        "LOCALPASTE_NAV_PROBE_SEED_TEXT=$seed_text"
        "LOCALPASTE_NAV_PROBE_SEED_NAME=nav-probe"
        "LOCALPASTE_NAV_PROBE_FOCUS_EDITOR=1"
    )
    if [[ -n "$seed_cursor" ]]; then
        env_args+=("LOCALPASTE_NAV_PROBE_SEED_CURSOR=$seed_cursor")
    fi
    env "${env_args[@]}" "$exe" &
    pid="$!"
    current_pid="$pid"

    process_visible=0
    for _ in $(seq 1 "$launch_poll_count"); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
            wait "$pid" || true
            echo "localpaste-gui exited before exposing a macOS process" >&2
            exit 1
        fi
        if process_exists "$pid"; then
            process_visible=1
            break
        fi
        ms_sleep "$launch_poll_ms"
    done
    if [[ "$process_visible" -ne 1 ]]; then
        close_app "$pid"
        current_pid=""
        echo "localpaste-gui did not expose a System Events process for $scenario_id" >&2
        exit 1
    fi

    ready=0
    for _ in $(seq 1 "$launch_poll_count"); do
        activate_app "$pid" || true
        if probe_ready "$scenario_id" "$seed_len"; then
            ready=1
            break
        fi
        ms_sleep "$launch_poll_ms"
    done
    if [[ "$ready" -ne 1 ]]; then
        close_app "$pid"
        current_pid=""
        echo "navigation probe did not report focused seeded editor before input for $scenario_id" >&2
        exit 1
    fi

    if ! wait_active_app "$pid" "$scenario_id"; then
        close_app "$pid"
        current_pid=""
        exit 1
    fi
    for key in "${keys[@]}"; do
        if ! wait_active_app "$pid" "$scenario_id"; then
            close_app "$pid"
            current_pid=""
            exit 1
        fi
        ms_sleep "$after_focus_ms"
        send_macos_key "$key"
        ms_sleep "$between_keys_ms"
    done
    ms_sleep "$after_scenario_ms"
    close_app "$pid"
    current_pid=""
    ms_sleep "$after_close_ms"
done

echo "nav probe log: $log_path"
if [[ "$assert" -eq 1 ]]; then
    summary_args=()
    if [[ "$summary" -eq 1 ]]; then
        summary_args+=(--summary)
    fi
    run_python tools/nav_probe_assert.py "$log_path" "$spec_path" --platform macos "${scenario_args[@]}" "${summary_args[@]}"
fi
