# Using `lpaste` with the GUI

`lpaste` is the terminal-side companion to the desktop app. It talks to the same localhost API that the GUI exposes, so you can inspect, export, diff, or automate work without leaving the editor.

> [!IMPORTANT]
> GitHub Releases currently ship GUI assets only. To use `lpaste`, build it from source with Cargo, for example:
>
> ```bash
> cargo build -p localpaste_cli --bin lpaste
> ```
>
> or run it directly with:
>
> ```bash
> cargo run -p localpaste_cli --bin lpaste -- --help
> ```

## Connect to the running GUI

When the GUI is open, `lpaste` resolves its endpoint in this order:

1. `--server`
2. `LP_SERVER`
3. discovered `.api-addr` for the active `DB_PATH` (unless `--no-discovery`)
4. the default local endpoint

In practice, if the GUI is already running on the same `DB_PATH`, `lpaste` usually works without extra flags:

```bash
lpaste list --limit 20
lpaste search-meta fsdp2
```

If you want to pin the endpoint explicitly, use `--server` or `LP_SERVER`:

```bash
lpaste --server http://127.0.0.1:38411 list --limit 20
```

```powershell
$env:LP_SERVER = "http://127.0.0.1:38411"
lpaste list --limit 20
```

Do not run the standalone `localpaste` server and the GUI against the same `DB_PATH` at the same time. Use the GUI's embedded API when you want terminal access to the same local store.

## Useful complementary workflows

List recent paste names and ids:

```bash
lpaste list --limit 20
```

Search metadata only. This is usually the fastest way to find a paste from the terminal when you remember tags, language, or derived retrieval terms:

```bash
lpaste search-meta validation
lpaste search-meta cublaslt
```

Fetch the current content of a paste into a local file:

```bash
lpaste get <paste-id> > recovered.txt
```

Inspect version history for a paste:

```bash
lpaste versions <paste-id> --limit 20
```

Fetch one stored historical version:

```bash
lpaste get-version <paste-id> <version-id-ms> > older-copy.txt
```

Diff two pastes, or diff two historical refs of the same paste:

```bash
lpaste diff <left-id> <right-id>
lpaste diff <paste-id> <paste-id> --left-version <older-version-id-ms> --right-version <newer-version-id-ms>
```

Duplicate a stored historical version into a new paste instead of resetting the current one:

```bash
lpaste duplicate-version <paste-id> <version-id-ms> --name "recovered-snapshot"
```

Reset a paste to a stored historical version:

```bash
lpaste reset-hard <paste-id> <version-id-ms> --yes
```

`reset-hard` is destructive: it rewrites the paste to the chosen snapshot and discards newer history for that paste.

## Scripted export from the GUI-managed store

The simplest robust export is JSON-first:

- use `lpaste --json list` to capture ids and names
- use `lpaste --json get <id>` to fetch the full paste payload
- write one JSON file per paste into a local directory

That preserves content and metadata without guessing file extensions.

### PowerShell example: export all pastes to `.\localpaste-all-pastes`

This writes `index.json` plus one `<safe-name>--<id>.json` file per paste in the current working directory.

```powershell
$outDir = Join-Path (Get-Location) "localpaste-all-pastes"
$limit = 100000

New-Item -ItemType Directory -Force -Path $outDir | Out-Null

$items = lpaste --json list --limit $limit | ConvertFrom-Json
$items | ConvertTo-Json -Depth 8 | Set-Content -Path (Join-Path $outDir "index.json")

foreach ($item in $items) {
    $id = [string]$item.id
    $name = [string]$item.name
    $safeName = ($name -replace '[^\w\.-]+', '-').Trim('-')
    if ([string]::IsNullOrWhiteSpace($safeName)) {
        $safeName = "paste"
    }

    $fileName = "{0}--{1}.json" -f $safeName.Substring(0, [Math]::Min($safeName.Length, 80)), $id
    lpaste --json get $id | Set-Content -Path (Join-Path $outDir $fileName)
}
```

### Bash example: export all pastes to `./localpaste-all-pastes`

This version uses `python3` only for JSON parsing and filename sanitization.

```bash
set -euo pipefail

out_dir="$PWD/localpaste-all-pastes"
limit=100000

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

Notes:

- `list` defaults to `10`, so the export script must pass a larger `--limit`.
- If you have more than the chosen limit, raise it and rerun.
- To export plain content instead of full JSON payloads, replace `lpaste --json get ...` with `lpaste get ...` and change the output extension.
