# GUI Perf Test Protocol (Rewrite)

This is a repeatable, low-friction protocol for validating editor performance changes in the rewrite. It isolates a temporary database, seeds deterministic test pastes, launches the GUI against that DB, and lists the manual checks.

## Goals

- Keep tests isolated from your real data.
- Create consistent paste sizes and languages for perf regression checks.
- Make it easy to repeat across machines/branches.

## Prereqs

- Build the server + GUI binaries:

```powershell
cargo build -p localpaste_server --bin localpaste --release
cargo build -p localpaste_gui --bin localpaste-gui --release
```

## 1) Seed a dedicated perf DB

```powershell
$ErrorActionPreference = "Stop"
$TestDb = Join-Path $env:TEMP ("lpaste-gui-perf-" + [guid]::NewGuid().ToString("N"))
$Port = 3055
$env:PORT = "$Port"
$env:DB_PATH = $TestDb
$env:RUST_LOG = "info"

$serverPid = Start-ServerProcess -ExePath .\target\release\localpaste.exe
Start-Sleep -Seconds 1
$Base = "http://127.0.0.1:$Port"

function Start-ServerProcess {
    param([string]$ExePath)
    if (-not ([System.Management.Automation.PSTypeName]'Localpaste.ProcessUtil').Type) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace Localpaste {
    public static class ProcessUtil {
        public const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
        public const uint CREATE_NO_WINDOW = 0x08000000;

        [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
        public struct STARTUPINFO {
            public int cb;
            public string lpReserved;
            public string lpDesktop;
            public string lpTitle;
            public int dwX;
            public int dwY;
            public int dwXSize;
            public int dwYSize;
            public int dwXCountChars;
            public int dwYCountChars;
            public int dwFillAttribute;
            public int dwFlags;
            public short wShowWindow;
            public short cbReserved2;
            public IntPtr lpReserved2;
            public IntPtr hStdInput;
            public IntPtr hStdOutput;
            public IntPtr hStdError;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct PROCESS_INFORMATION {
            public IntPtr hProcess;
            public IntPtr hThread;
            public uint dwProcessId;
            public uint dwThreadId;
        }

        [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
        public static extern bool CreateProcess(
            string lpApplicationName,
            string lpCommandLine,
            IntPtr lpProcessAttributes,
            IntPtr lpThreadAttributes,
            bool bInheritHandles,
            uint dwCreationFlags,
            IntPtr lpEnvironment,
            string lpCurrentDirectory,
            ref STARTUPINFO lpStartupInfo,
            out PROCESS_INFORMATION lpProcessInformation);

        [DllImport("kernel32.dll", SetLastError=true)]
        public static extern bool GenerateConsoleCtrlEvent(uint dwCtrlEvent, uint dwProcessGroupId);

        [DllImport("kernel32.dll", SetLastError=true)]
        public static extern bool CloseHandle(IntPtr hObject);
    }
}
'@
    }
    $si = New-Object Localpaste.ProcessUtil+STARTUPINFO
    $si.cb = [Runtime.InteropServices.Marshal]::SizeOf($si)
    $pi = New-Object Localpaste.ProcessUtil+PROCESS_INFORMATION
    $flags = [Localpaste.ProcessUtil]::CREATE_NEW_PROCESS_GROUP -bor [Localpaste.ProcessUtil]::CREATE_NO_WINDOW
    $ok = [Localpaste.ProcessUtil]::CreateProcess(
        $ExePath,
        "`"$ExePath`"",
        [IntPtr]::Zero,
        [IntPtr]::Zero,
        $false,
        $flags,
        [IntPtr]::Zero,
        (Split-Path $ExePath),
        [ref]$si,
        [ref]$pi
    )
    if (-not $ok) {
        $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw "CreateProcess failed with Win32 error $err"
    }
    [Localpaste.ProcessUtil]::CloseHandle($pi.hThread) | Out-Null
    [Localpaste.ProcessUtil]::CloseHandle($pi.hProcess) | Out-Null
    return [int]$pi.dwProcessId
}

function Stop-ServerGracefully {
    param([int]$ProcessId)
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if (-not $proc) { return }
    [Localpaste.ProcessUtil]::GenerateConsoleCtrlEvent(0, [uint32]$ProcessId) | Out-Null
    Start-Sleep -Milliseconds 300
    $proc = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($proc) { Stop-Process -Id $ProcessId -Force }
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
Stop-ServerGracefully -ProcessId $serverPid
```

## 2) Launch the GUI using the perf DB

```powershell
$env:DB_PATH = $TestDb
# Read-only virtual preview (diagnostic baseline)
# $env:LOCALPASTE_VIRTUAL_PREVIEW = "1"

# Editable rope-backed virtual editor (wins if both flags are set)
$env:LOCALPASTE_VIRTUAL_EDITOR = "1"

# Optional frame metrics log (avg FPS + p95 ms every ~2s)
# $env:LOCALPASTE_EDITOR_PERF_LOG = "1"

.\target\release\localpaste-gui.exe
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
   - Expect: no hitching when redraws happen; sustained smoothness target is >=45 FPS in release runs.

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

8. **Virtual editor parity (when `LOCALPASTE_VIRTUAL_EDITOR=1`)**
   - Verify `Ctrl/Cmd+A/C/X/V`, `Ctrl/Cmd+Z/Y`, Home/End, PageUp/PageDown, shift-selection.
   - Verify IME composition (`Enabled` -> `Preedit` -> `Commit`) does not lose caret/selection state.
   - Verify drag-selection behavior when crossing viewport edges (including auto-scroll behavior if implemented).

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
- Release gate for `perf-scroll-5k-lines`: average FPS `>= 45` and p95 frame time `<= 25 ms`.
