<!-- # GUI Perf Test Protocol (Rewrite) -->

This is a repeatable, low-friction protocol for validating editor performance
<!-- changes in the rewrite. It isolates a temporary database, seeds deterministic -->
test pastes, launches the GUI against that DB, and lists the manual checks.

## Goals

- Keep tests isolated from your real data.
- Create consistent paste sizes and languages for perf regression checks.
- Make it easy to repeat across machines/branches.

## Prereqs

- Build the server + GUI binaries:

```powershell
cargo build -p localpaste_server --bin localpaste
cargo build -p localpaste_gui --bin localpaste-gui
```

## 1) Seed a dedicated perf DB

```powershell
$ErrorActionPreference = "Stop"
$TestDb = Join-Path $env:TEMP ("lpaste-gui-perf-" + [guid]::NewGuid().ToString("N"))
$Port = 3055
$env:PORT = "$Port"
$env:DB_PATH = $TestDb
$env:RUST_LOG = "info"

$server = Start-Process -FilePath .\target\debug\localpaste.exe -NoNewWindow -PassThru
Start-Sleep -Seconds 1
$Base = "http://127.0.0.1:$Port"

function Stop-ServerGracefully {
    param([System.Diagnostics.Process]$Process)
    if (-not $Process -or $Process.HasExited) { return }
    if (-not ([System.Management.Automation.PSTypeName]'Localpaste.ConsoleControl').Type) {
        Add-Type -Namespace Localpaste -Name ConsoleControl -MemberDefinition @"
using System;
using System.Runtime.InteropServices;
public static class ConsoleControl {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool AttachConsole(uint dwProcessId);
    [DllImport("kernel32.dll", SetLastError=true, ExactSpelling=true)]
    public static extern bool FreeConsole();
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool GenerateConsoleCtrlEvent(uint dwCtrlEvent, uint dwProcessGroupId);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool SetConsoleCtrlHandler(IntPtr HandlerRoutine, bool Add);
}
"@
    }
    [Localpaste.ConsoleControl]::FreeConsole() | Out-Null
    if ([Localpaste.ConsoleControl]::AttachConsole([uint32]$Process.Id)) {
        [Localpaste.ConsoleControl]::SetConsoleCtrlHandler([IntPtr]::Zero, $true) | Out-Null
        [Localpaste.ConsoleControl]::GenerateConsoleCtrlEvent(0, 0) | Out-Null
        Start-Sleep -Milliseconds 200
        [Localpaste.ConsoleControl]::FreeConsole() | Out-Null
        [Localpaste.ConsoleControl]::AttachConsole(0xFFFFFFFF) | Out-Null
        [Localpaste.ConsoleControl]::SetConsoleCtrlHandler([IntPtr]::Zero, $false) | Out-Null
        if ($Process.WaitForExit(3000)) { return }
    }
    Stop-Process -Id $Process.Id -Force
}

function New-TestPaste {
    param(
        [string]$Name,
        [string]$Content,
        [string]$Language
    )
    $body = @{
        name = $Name
        content = $Content
        language = $Language
        language_is_manual = $true
    } | ConvertTo-Json
    $resp = Invoke-RestMethod -Method Post -Uri "$Base/api/paste" -Body $body -ContentType "application/json"
    [pscustomobject]@{ name = $resp.name; id = $resp.id; bytes = $resp.content.Length }
}

# Medium (10-50KB) python paste
$linePy = "def compute(value):`n    return value * 2`n`n"
$repeat = [math]::Ceiling(20000 / $linePy.Length)
$medium = ($linePy * $repeat).Substring(0, 20000)
$pasteMedium = New-TestPaste "perf-medium-python" $medium "python"

# ~100KB python paste
$linePy2 = "class DataProcessor:`n    def __init__(self, cfg):`n        self.cfg = cfg`n`n"
$repeat = [math]::Ceiling(100000 / $linePy2.Length)
$big = ($linePy2 * $repeat).Substring(0, 100000)
$paste100 = New-TestPaste "perf-100kb-python" $big "python"

# ~300KB rust paste (forces plain fallback)
$lineRs = 'fn main() { println!("hello"); }' + "`n"
$repeat = [math]::Ceiling(300000 / $lineRs.Length)
$huge = ($lineRs * $repeat).Substring(0, 300000)
$paste300 = New-TestPaste "perf-300kb-rust" $huge "rust"

# Scroll-heavy paste (~5k lines)
$lineScroll = "let x = 1 + 2; // scroll test`n"
$scroll = $lineScroll * 5000
$pasteScroll = New-TestPaste "perf-scroll-5k-lines" $scroll "rust"

# Show created IDs + sizes
$pasteMedium, $paste100, $paste300, $pasteScroll | Format-Table

# Stop server (DB is now populated)
Stop-ServerGracefully -Process $server
```

## 2) Launch the GUI using the perf DB

```powershell
$env:DB_PATH = $TestDb
.\target\debug\localpaste-gui.exe
```

## 3) Manual verification checklist

1. **Baseline responsiveness**
   - Open `perf-medium-python`.
   - Edit at start/middle/end.
   - Expect: caret stays responsive, highlight stays on.

2. **Async highlight behavior (~100KB)**
   - Open `perf-100kb-python`.
   - Type quickly for 2-3 seconds.
   - Expect: highlight stays visible (no rapid on/off flicker). Colors may lag
     briefly while typing, then refresh after ~150ms idle.

3. **Large paste fallback**
   - Open `perf-300kb-rust`.
   - Expect: label shows `(plain)`, no highlight, smooth scrolling.

4. **Scroll performance**
   - Open `perf-scroll-5k-lines`.
   - Rapidly scroll up/down.
   - Expect: no hitching when redraws happen.

5. **Wrap/reflow**
   - Resize the window width several times with a highlighted paste open.
   - Expect: wrapping recalculates; highlight persists without long plain-text
     gaps.

6. **Status bar counts**
   - Type in `perf-100kb-python`.
   - Expect: char count updates smoothly.

7. **Shortcut sanity**
   - `Ctrl/Cmd+N`, `Ctrl/Cmd+Delete`, `Ctrl/Cmd+V` (with no focus).
   - Expect: behavior unchanged, no noticeable lag.

## 4) Cleanup

```powershell
Remove-Item -Recurse -Force $TestDb
```

If PowerShell blocks the removal in your environment, use:

```powershell
cmd /c rmdir /s /q "$TestDb"
```

## Notes

- Keep the perf DB isolated (always set `DB_PATH`) to avoid polluting real data.
- If a perf regression is found, note which paste name, approximate size, and
  the exact interaction that triggers lag.
