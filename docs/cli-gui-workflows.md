# Using `lpaste` with the GUI

`lpaste` is the terminal-side companion to the desktop app. It talks to the same localhost API that the GUI exposes (the standalone server serves it too), so you can inspect, export, diff, or automate work without leaving the editor.

Install `lpaste` with the `cargo install` commands in the [README](../README.md#server-and-cli). [Release archives](release-gui.md#install-a-release) contain the GUI only. The examples below assume `lpaste` is on `PATH`.

## Connect to the running GUI

By default the GUI serves its API on `127.0.0.1:38411`, or on a free port if that one is taken, and writes the address it ended up with to a `.api-addr` file in its database directory. Unless told otherwise, `lpaste` reads that file from the directory named by its own `DB_PATH` (the default location when unset) and sends a probe request to the recorded address, giving up after 250 ms. It uses the address only if the answer identifies a LocalPaste API on a loopback host. A missing, stale, or unrelated entry falls back to the default endpoint, `http://localhost:38411`. The probe's exact checks are described in [architecture.md#10-discovery-and-trust](architecture.md#10-discovery-and-trust).

So when the GUI runs on the default `DB_PATH`, `lpaste` works without extra flags. If the GUI was started with a different `DB_PATH`, set the same value in the shell that runs `lpaste`:

```bash
lpaste list --limit 20
lpaste search-meta fsdp2
```

To pin the endpoint instead, use `--server` or `LP_SERVER`; either skips discovery and sends requests to the given address:

```bash
lpaste --server http://127.0.0.1:38411 list --limit 20
```

```powershell
$env:LP_SERVER = "http://127.0.0.1:38411"
lpaste list --limit 20
```

`--no-discovery` turns discovery off without choosing an endpoint: `lpaste` then uses `--server` or `LP_SERVER` if set and the default endpoint otherwise, and never reads `.api-addr` or probes a port.

```bash
lpaste --no-discovery list --limit 20
```

For terminal access to a GUI-owned store, follow [storage operational expectations](storage.md#operational-expectations).

## Useful complementary workflows

Replace quoted `<...>` placeholders with IDs from `lpaste list` or `lpaste versions`.

Search metadata only. This is usually the fastest way to find a paste from the terminal when you remember tags, language, or derived retrieval terms:

```bash
lpaste search-meta validation
lpaste search-meta cublaslt
```

Search inherits the server's default case behavior, including `LOCALPASTE_SEARCH_CASE_SENSITIVE`. Use an explicit flag when a script needs deterministic matching:

```bash
lpaste search --case-sensitive Needle
lpaste search-meta --case-insensitive needle
```

Fetch the current content of a paste into a local file:

```bash
lpaste get "<paste-id>" > recovered.txt
```

Inspect version history for a paste:

```bash
lpaste versions "<paste-id>" --limit 20
```

Fetch one stored historical version:

```bash
lpaste get-version "<paste-id>" "<version-id-ms>" > older-copy.txt
```

Diff two pastes, or diff two historical refs of the same paste:

```bash
lpaste diff "<left-id>" "<right-id>"
lpaste diff "<paste-id>" "<paste-id>" --left-version "<older-version-id-ms>" --right-version "<newer-version-id-ms>"
```

Duplicate a stored historical version into a new paste instead of resetting the current one:

```bash
lpaste duplicate-version "<paste-id>" "<version-id-ms>" --name "recovered-snapshot"
```

Reset a paste to a stored historical version:

```bash
lpaste reset-hard "<paste-id>" "<version-id-ms>" --yes
```

`reset-hard` is destructive: it rewrites the paste to the chosen snapshot and discards newer history for that paste.

## Scripted export of recent pastes

Export JSON to preserve content and metadata:

- use `lpaste --json list` to capture ids and names
- use `lpaste --json get <id>` to fetch the full paste payload
- write one JSON file per paste into a local directory

`list` defaults to 10 rows, and the API caps it at 100 regardless of a larger `--limit`. These scripts export up to the 100 most recent paste heads, excluding historical versions. They cannot enumerate a larger store because list pagination is unavailable. Use [database backups](deployment.md#backups) for a complete snapshot.

### PowerShell 5.1/7 example

This writes `index.json` plus one `<safe-name>--<id>.json` file per paste inside `localpaste-export/`.

```powershell
$outDir = Join-Path (Get-Location) "localpaste-export"
$limit = 100
$utf8Encoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = $utf8Encoding

New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$indexJson = lpaste --json list --limit $limit | Out-String
[IO.File]::WriteAllText((Join-Path $outDir "index.json"), $indexJson, $utf8Encoding)
$items = $indexJson | ConvertFrom-Json

foreach ($item in $items) {
    $id = [string]$item.id
    $name = [string]$item.name
    $safeName = ($name -replace '[^\w\.-]+', '-').Trim('-')
    if ([string]::IsNullOrWhiteSpace($safeName)) {
        $safeName = "paste"
    }

    $fileName = "{0}--{1}.json" -f $safeName.Substring(0, [Math]::Min($safeName.Length, 80)), $id
    $payload = lpaste --json get $id | Out-String
    [IO.File]::WriteAllText((Join-Path $outDir $fileName), $payload, $utf8Encoding)
}
```

The explicit encoding preserves native CLI Unicode output and writes UTF-8 without a BOM on both PowerShell versions. `index.json` remains an array for empty and single-paste stores.

### Bash example

This version uses `python3` only for JSON parsing and filename sanitization.

```bash
set -euo pipefail

out_dir="$PWD/localpaste-export"
limit=100

mkdir -p "$out_dir"
lpaste --json list --limit "$limit" > "$out_dir/index.json"

python3 - "$out_dir/index.json" <<'PY' | while IFS=$'\t' read -r paste_id safe_name; do
import json
import re
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    items = json.load(fh)

for item in items:
    name = str(item.get("name") or "paste")
    safe = re.sub(r"[^\w.\-]+", "-", name).strip("-") or "paste"
    print(f"{item['id']}\t{safe[:80]}")
PY
  lpaste --json get "$paste_id" > "$out_dir/${safe_name}--${paste_id}.json"
done
```

To export plain content instead of full JSON payloads, replace `lpaste --json get ...` with `lpaste get ...` and change the output extension.
