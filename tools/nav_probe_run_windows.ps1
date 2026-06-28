[CmdletBinding()]
param(
    [string]$Spec = "docs/dev/nav_contract.json",
    [string]$Log = "",
    [string[]]$Only = @(),
    [switch]$Build,
    [switch]$Assert,
    [switch]$UseLowLevelInput,
    [int]$LaunchPollMs = 50,
    [int]$LaunchPollCount = 800,
    [int]$AfterFocusMs = 25,
    [int]$BetweenKeysMs = 350,
    [int]$AfterScenarioMs = 900,
    [int]$AfterCloseMs = 500
)

$ErrorActionPreference = "Stop"
$repo = (Get-Location).Path
if ([string]::IsNullOrWhiteSpace($Log)) {
    $Log = "target/nav-probe-windows-$([guid]::NewGuid().ToString('N')).ndjson"
}
$specPath = [System.IO.Path]::GetFullPath((Join-Path $repo $Spec))
$logPath = [System.IO.Path]::GetFullPath((Join-Path $repo $Log))

function Assert-RepoPath {
    param([string]$Path, [string]$Label)
    if (-not $Path.StartsWith($repo, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label must stay inside repository root: $Path"
    }
}

function ConvertTo-SafeName {
    param([string]$Value)
    return ($Value -replace '[^A-Za-z0-9_.-]', '_')
}

function Test-ProbeSeeded {
    param([string]$Path, [string]$ScenarioId, [int]$ExpectedBufferLen)
    if (-not (Test-Path -LiteralPath $Path)) {
        return $false
    }
    $lines = @(Get-Content -LiteralPath $Path -Tail 80)
    for ($idx = $lines.Count - 1; $idx -ge 0; $idx--) {
        try {
            $frame = $lines[$idx] | ConvertFrom-Json
        }
        catch {
            continue
        }
        if ([string]$frame.scenario -ne $ScenarioId) {
            continue
        }
        $hasProbePaste = [string]$frame.app.selected_id -eq "__nav_probe__"
        $hasSeedBuffer = [int]$frame.cursor.buffer_len_chars -eq $ExpectedBufferLen
        return ($hasProbePaste -and $hasSeedBuffer)
    }
    return $false
}

Assert-RepoPath $specPath "Spec"
Assert-RepoPath $logPath "Log"

if ($Build) {
    cargo build -p localpaste_gui --bin localpaste-gui
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }
}

$exe = [System.IO.Path]::GetFullPath((Join-Path $repo "target/debug/localpaste-gui.exe"))
Assert-RepoPath $exe "Executable"
if (-not (Test-Path -LiteralPath $exe)) {
    throw "GUI executable not found: $exe. Run with -Build or build it first."
}

$logParent = Split-Path -Parent $logPath
New-Item -ItemType Directory -Force -Path $logParent | Out-Null
if (Test-Path -LiteralPath $logPath) {
    Remove-Item -LiteralPath $logPath -Force
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class LocalPasteNavProbeWindow {
    const int SW_RESTORE = 9;
    const int MOUSEEVENTF_LEFTDOWN = 0x0002;
    const int MOUSEEVENTF_LEFTUP = 0x0004;
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }
    [StructLayout(LayoutKind.Sequential)]
    public struct POINT {
        public int X;
        public int Y;
    }
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")]
    static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")]
    static extern bool GetCursorPos(out POINT point);
    [DllImport("user32.dll")]
    static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    static extern void mouse_event(int dwFlags, int dx, int dy, int dwData, UIntPtr dwExtraInfo);
    public static void RestoreAndForeground(IntPtr hWnd) {
        ShowWindow(hWnd, SW_RESTORE);
        SetForegroundWindow(hWnd);
    }
    public static void ClickTitleBar(IntPtr hWnd) {
        RECT rect;
        if (!GetWindowRect(hWnd, out rect)) {
            return;
        }
        POINT prior;
        bool restoreCursor = GetCursorPos(out prior);
        int x = rect.Left + Math.Max(48, (rect.Right - rect.Left) / 2);
        int y = rect.Top + 10;
        SetCursorPos(x, y);
        mouse_event(MOUSEEVENTF_LEFTDOWN, x, y, 0, UIntPtr.Zero);
        mouse_event(MOUSEEVENTF_LEFTUP, x, y, 0, UIntPtr.Zero);
        if (restoreCursor) {
            SetCursorPos(prior.X, prior.Y);
        }
    }
}
public static class LocalPasteNavProbeInput {
    const int KEYEVENTF_KEYUP = 0x0002;
    const byte VK_CONTROL = 0x11;
    const byte VK_SHIFT = 0x10;
    const byte VK_MENU = 0x12;
    [DllImport("user32.dll")]
    static extern void keybd_event(byte bVk, byte bScan, int dwFlags, UIntPtr dwExtraInfo);
    static void Down(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); }
    static void Up(byte vk) { keybd_event(vk, 0, KEYEVENTF_KEYUP, UIntPtr.Zero); }
    public static void SendChord(byte key, bool ctrl, bool shift) {
        try {
            if (ctrl) { Down(VK_CONTROL); }
            if (shift) { Down(VK_SHIFT); }
            Down(key);
            Up(key);
        } finally {
            if (shift) { Up(VK_SHIFT); }
            if (ctrl) { Up(VK_CONTROL); }
        }
    }
    public static void ReleaseModifiers() {
        Up(VK_MENU);
        Up(VK_SHIFT);
        Up(VK_CONTROL);
    }
}
'@

function Send-NavChord {
    param([string]$Chord)
    $ctrl = $Chord.Contains("^")
    $shift = $Chord.Contains("+")
    $name = $Chord
    if ($Chord -match '\{([^}]+)\}') {
        $name = $Matches[1]
    }
    $vk = switch ($name.ToUpperInvariant()) {
        "LEFT" { 0x25 }
        "RIGHT" { 0x27 }
        "UP" { 0x26 }
        "DOWN" { 0x28 }
        "HOME" { 0x24 }
        "END" { 0x23 }
        "PGUP" { 0x21 }
        "PGDN" { 0x22 }
        "BACKSPACE" { 0x08 }
        "DEL" { 0x2E }
        default { throw "Unsupported navigation chord: $Chord" }
    }
    [LocalPasteNavProbeInput]::SendChord([byte]$vk, [bool]$ctrl, [bool]$shift)
}

function Release-NavModifiers {
    [LocalPasteNavProbeInput]::ReleaseModifiers()
}

function Set-NavProbeForeground {
    param([System.Diagnostics.Process]$Process, [IntPtr]$Handle)
    $Process.Refresh()
    if (-not [string]::IsNullOrWhiteSpace($Process.MainWindowTitle)) {
        $null = $wscript.AppActivate($Process.MainWindowTitle)
    }
    $null = $wscript.AppActivate($Process.Id)
    [LocalPasteNavProbeWindow]::RestoreAndForeground($Handle)
}

$specJson = Get-Content -LiteralPath $specPath -Raw | ConvertFrom-Json
$wscript = New-Object -ComObject WScript.Shell
$scenarios = @(
    $specJson.scenarios | Where-Object {
        $_.platforms -contains "windows" -and $_.driver -and $_.driver.windows
    }
)
if ($Only.Count -gt 0) {
    $allowed = @{}
    foreach ($scenarioId in $Only) {
        $allowed[[string]$scenarioId] = $true
    }
    $scenarios = @($scenarios | Where-Object { $allowed.ContainsKey([string]$_.id) })
}
if ($scenarios.Count -eq 0) {
    throw "No Windows scenarios found in $specPath"
}

foreach ($scenario in $scenarios) {
    $scenarioId = [string]$scenario.id
    $safeScenario = ConvertTo-SafeName $scenarioId
    $dbPath = [System.IO.Path]::GetFullPath((Join-Path $repo "target/nav-probe-db-$safeScenario-$([guid]::NewGuid().ToString('N'))"))
    Assert-RepoPath $dbPath "DB_PATH"
    New-Item -ItemType Directory -Force -Path $dbPath | Out-Null

    $seedText = "alpha beta`ngamma delta`nepsilon zeta`n"
    if ($scenario.seed -and $scenario.seed.text) {
        $seedText = [string]$scenario.seed.text
    }
    $expectedBufferLen = $seedText.Length

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $exe
    $psi.WorkingDirectory = $repo
    $psi.UseShellExecute = $false
    $psi.Environment["DB_PATH"] = $dbPath
    $psi.Environment["LOCALPASTE_NAV_PROBE_LOG"] = $logPath
    $psi.Environment["LOCALPASTE_NAV_PROBE_SCENARIO"] = $scenarioId
    $psi.Environment["LOCALPASTE_NAV_PROBE_SEED_TEXT"] = $seedText
    $psi.Environment["LOCALPASTE_NAV_PROBE_SEED_NAME"] = "nav-probe"
    $psi.Environment["LOCALPASTE_NAV_PROBE_FOCUS_EDITOR"] = "1"
    if ($scenario.seed -and $scenario.seed.cursor) {
        $psi.Environment["LOCALPASTE_NAV_PROBE_SEED_CURSOR"] = [string]$scenario.seed.cursor
    }

    Write-Host "nav probe: $scenarioId"
    $proc = [System.Diagnostics.Process]::Start($psi)
    try {
        Release-NavModifiers
        $handle = [IntPtr]::Zero
        for ($idx = 0; $idx -lt $LaunchPollCount; $idx++) {
            Start-Sleep -Milliseconds $LaunchPollMs
            $proc.Refresh()
            if ($proc.HasExited) {
                throw "localpaste-gui exited early for $scenarioId with code $($proc.ExitCode)"
            }
            if ($proc.MainWindowHandle -ne [IntPtr]::Zero) {
                $handle = $proc.MainWindowHandle
                break
            }
        }
        if ($handle -eq [IntPtr]::Zero) {
            throw "localpaste-gui did not expose a main window for $scenarioId"
        }
        [LocalPasteNavProbeWindow]::ClickTitleBar($handle)
        Start-Sleep -Milliseconds 100
        $seeded = $false
        for ($idx = 0; $idx -lt $LaunchPollCount; $idx++) {
            Set-NavProbeForeground $proc $handle
            Start-Sleep -Milliseconds $LaunchPollMs
            if (Test-ProbeSeeded $logPath $scenarioId $expectedBufferLen) {
                $seeded = $true
                break
            }
        }
        if (-not $seeded) {
            throw "navigation probe did not report seeded editor before input for $scenarioId"
        }
        [LocalPasteNavProbeWindow]::ClickTitleBar($handle)
        Set-NavProbeForeground $proc $handle
        Start-Sleep -Milliseconds $AfterFocusMs
        foreach ($key in $scenario.driver.windows.send_keys) {
            try {
                if ($UseLowLevelInput) {
                    Send-NavChord ([string]$key)
                }
                else {
                    $wscript.SendKeys([string]$key)
                }
            }
            finally {
                Release-NavModifiers
            }
            Start-Sleep -Milliseconds $BetweenKeysMs
        }
        Start-Sleep -Milliseconds $AfterScenarioMs
    }
    finally {
        Release-NavModifiers
        if (-not $proc.HasExited) {
            $proc.CloseMainWindow() | Out-Null
            if (-not $proc.WaitForExit(5000)) {
                $proc.Kill()
                $proc.WaitForExit()
            }
        }
    }
    Start-Sleep -Milliseconds $AfterCloseMs
}

Write-Host "nav probe log: $logPath"
if ($Assert) {
    $assertArgs = @("tools/nav_probe_assert.py", $logPath, $specPath, "--platform", "windows")
    foreach ($scenarioId in $Only) {
        $assertArgs += @("--scenario", [string]$scenarioId)
    }
    conda run -n misc python @assertArgs
    if ($LASTEXITCODE -ne 0) {
        throw "navigation probe assertions failed with exit code $LASTEXITCODE"
    }
}
