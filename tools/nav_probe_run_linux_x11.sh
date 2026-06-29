#!/usr/bin/env bash
set -euo pipefail

spec="docs/dev/nav_contract.json"
log=""
build=0
assert=0
list=0
launch_poll_ms=100
launch_poll_count=400
after_focus_ms=50
between_keys_ms=350
after_scenario_ms=900
after_close_ms=500
only=()

usage() {
    cat <<'EOF'
Usage: tools/nav_probe_run_linux_x11.sh [options]

Prerequisites:
  Linux X11 session with xdotool installed. Wayland sessions must use the
  navigation probe manually because synthetic input is compositor-restricted.

Options:
  --spec PATH                 Navigation contract JSON path
  --log PATH                  Probe NDJSON path; defaults to a unique target/ file
  --only SCENARIO             Run one scenario; may be repeated
  --list                      List selected Linux X11 scenarios and exit
  --build                     Build localpaste-gui before running
  --assert                    Run tools/nav_probe_assert.py after the probe run
  --launch-poll-ms MS         Poll interval while waiting for app/probe readiness
  --launch-poll-count COUNT   Poll count while waiting for app/probe readiness
  --after-focus-ms MS         Delay after final window activation before sending keys
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

repo="$(pwd -P)"
if [[ -z "$log" ]]; then
    if [[ -r /proc/sys/kernel/random/uuid ]]; then
        run_id="$(tr -d '-' < /proc/sys/kernel/random/uuid)"
    else
        run_id="$(date +%s%N)"
    fi
    log="target/nav-probe-linux-x11-${run_id}.ndjson"
fi

absolute_path() {
    realpath -m "$1"
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
    sleep "$(awk "BEGIN { printf \"%.3f\", ${ms} / 1000 }")"
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "required command not found: $1" >&2
        exit 2
    fi
}

require_xdotool() {
    if ! command -v xdotool >/dev/null 2>&1; then
        cat >&2 <<'EOF'
required command not found: xdotool

Linux navigation automation is X11-only and requires xdotool for window focus
and native key injection. Install xdotool for automated contract runs, or run
the navigation probe manually on Wayland.
EOF
        exit 2
    fi
}

spec_path="$(assert_repo_path "$spec" "Spec")"
log_path="$(assert_repo_path "$log" "Log")"
exe="$(assert_repo_path "target/debug/localpaste-gui" "Executable")"

require_command awk

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

scenario_args=()
for scenario_id in "${only[@]}"; do
    scenario_args+=(--scenario "$scenario_id")
done

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
    if "linux" not in scenario.get("platforms", []):
        continue
    linux = scenario.get("driver", {}).get("linux_x11")
    if not linux:
        continue
    seed = scenario.get("seed", {})
    text = seed.get("text", "alpha beta\ngamma delta\nepsilon zeta\n")
    print(json.dumps({
        "id": scenario["id"],
        "seed_text": text,
        "seed_cursor": seed.get("cursor"),
        "seed_len": len(text),
        "keys": linux.get("keys", []),
    }))
PY
)

if [[ "${#scenarios[@]}" -eq 0 ]]; then
    echo "No Linux X11 scenarios found in $spec_path" >&2
    exit 2
fi

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

if [[ "${XDG_SESSION_TYPE:-x11}" == "wayland" ]]; then
    echo "Linux navigation automation is X11-only; run the probe manually on Wayland." >&2
    exit 2
fi
require_xdotool

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

probe_seeded() {
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
    if app.get("selected_id") == "__nav_probe__" and cursor.get("buffer_len_chars") == expected_len:
        raise SystemExit(0)
    raise SystemExit(1)
raise SystemExit(1)
PY
}

wait_for_window() {
    local pid="$1"
    local window_id=""
    for _ in $(seq 1 "$launch_poll_count"); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
            wait "$pid" || true
            echo "localpaste-gui exited before exposing a window" >&2
            return 1
        fi
        window_id="$(xdotool search --pid "$pid" --onlyvisible 2>/dev/null | head -n 1 || true)"
        if [[ -n "$window_id" ]]; then
            printf '%s\n' "$window_id"
            return 0
        fi
        ms_sleep "$launch_poll_ms"
    done
    echo "localpaste-gui did not expose a visible X11 window" >&2
    return 1
}

activate_window() {
    local window_id="$1"
    xdotool windowraise "$window_id" >/dev/null 2>&1 || true
    xdotool windowactivate --sync "$window_id" >/dev/null 2>&1 || true
}

close_app() {
    local pid="$1"
    local _window_id="${2:-}"
    if ! kill -0 "$pid" >/dev/null 2>&1; then
        wait "$pid" || true
        return 0
    fi
    # Probe runs use a disposable DB and need deterministic teardown. X11
    # window-close events can race with winit window geometry queries after the
    # evidence frame has been written, producing noisy shutdown panics.
    kill "$pid" >/dev/null 2>&1 || true
    for _ in $(seq 1 50); do
        if ! kill -0 "$pid" >/dev/null 2>&1; then
            wait "$pid" || true
            return 0
        fi
        sleep 0.1
    done
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" || true
}

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
        "LOCALPASTE_LINUX_DESKTOP_ENTRY=off"
        "LIBGL_ALWAYS_SOFTWARE=${LIBGL_ALWAYS_SOFTWARE:-1}"
        "WGPU_BACKEND=${WGPU_BACKEND:-gl}"
    )
    if [[ -n "$seed_cursor" ]]; then
        env_args+=("LOCALPASTE_NAV_PROBE_SEED_CURSOR=$seed_cursor")
    fi
    env "${env_args[@]}" "$exe" &
    pid="$!"
    window_id=""
    if ! window_id="$(wait_for_window "$pid")"; then
        close_app "$pid" "$window_id"
        exit 1
    fi

    seeded=0
    for _ in $(seq 1 "$launch_poll_count"); do
        activate_window "$window_id"
        if probe_seeded "$scenario_id" "$seed_len"; then
            seeded=1
            break
        fi
        ms_sleep "$launch_poll_ms"
    done
    if [[ "$seeded" -ne 1 ]]; then
        close_app "$pid" "$window_id"
        echo "navigation probe did not report seeded editor before input for $scenario_id" >&2
        exit 1
    fi

    activate_window "$window_id"
    ms_sleep "$after_focus_ms"
    for key in "${keys[@]}"; do
        activate_window "$window_id"
        xdotool key "$key"
        ms_sleep "$between_keys_ms"
    done
    ms_sleep "$after_scenario_ms"
    close_app "$pid" "$window_id"
    ms_sleep "$after_close_ms"
done

echo "nav probe log: $log_path"
if [[ "$assert" -eq 1 ]]; then
    run_python tools/nav_probe_assert.py "$log_path" "$spec_path" --platform linux "${scenario_args[@]}"
fi
