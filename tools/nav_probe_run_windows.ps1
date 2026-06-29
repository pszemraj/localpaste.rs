[CmdletBinding()]
param(
    [string]$Spec = "docs/dev/nav_contract.json",
    [string]$Log = "",
    [string[]]$Only = @(),
    [switch]$Build,
    [switch]$Assert,
    [switch]$List,
    [switch]$CtrlOnly,
    [switch]$Summary,
    [switch]$UseLowLevelInput,
    [switch]$UseSendKeys,
    [int]$LaunchPollMs = 50,
    [int]$LaunchPollCount = 800,
    [int]$ForegroundPollMs = 25,
    [int]$ForegroundPollCount = 80,
    [int]$AfterFocusMs = 150,
    [int]$BetweenKeysMs = 350,
    [int]$AfterScenarioMs = 900,
    [int]$AfterCloseMs = 500,
    [int]$RepeatCount = 1
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
    $separators = [char[]]@([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    $repoRoot = [System.IO.Path]::GetFullPath($repo).TrimEnd($separators)
    $fullPath = [System.IO.Path]::GetFullPath($Path).TrimEnd($separators)
    $repoPrefix = $repoRoot + [System.IO.Path]::DirectorySeparatorChar
    if (-not ($fullPath.Equals($repoRoot, [System.StringComparison]::OrdinalIgnoreCase) -or $fullPath.StartsWith($repoPrefix, [System.StringComparison]::OrdinalIgnoreCase))) {
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

function Resolve-NavProbePython {
    if (-not [string]::IsNullOrWhiteSpace($env:LOCALPASTE_NAV_PROBE_PYTHON)) {
        return @($env:LOCALPASTE_NAV_PROBE_PYTHON)
    }
    if (-not [string]::IsNullOrWhiteSpace($env:VIRTUAL_ENV)) {
        $candidate = Join-Path $env:VIRTUAL_ENV "Scripts\python.exe"
        if (Test-Path -LiteralPath $candidate) {
            return @($candidate)
        }
    }
    if (-not [string]::IsNullOrWhiteSpace($env:CONDA_PREFIX)) {
        $candidate = Join-Path $env:CONDA_PREFIX "python.exe"
        if (Test-Path -LiteralPath $candidate) {
            return @($candidate)
        }
    }
    $python = Get-Command python -ErrorAction SilentlyContinue
    if ($python) {
        return @($python.Source)
    }
    $python3 = Get-Command python3 -ErrorAction SilentlyContinue
    if ($python3) {
        return @($python3.Source)
    }
    $conda = Get-Command conda -ErrorAction SilentlyContinue
    if ($conda) {
        return @($conda.Source, "run", "-n", "misc", "python")
    }
    throw "required command not found: python, python3, conda, or LOCALPASTE_NAV_PROBE_PYTHON"
}

function Invoke-NavProbePython {
    param([string[]]$Arguments)
    $pythonCommand = @($script:NavProbePython)
    $cmd = $pythonCommand[0]
    $prefixArgs = @()
    if ($pythonCommand.Count -gt 1) {
        $prefixArgs = $pythonCommand[1..($pythonCommand.Count - 1)]
    }
    & $cmd @prefixArgs @Arguments
}

function Set-NavProbeProcessEnv {
    param([System.Diagnostics.ProcessStartInfo]$ProcessStartInfo, [string]$Name, [string]$Value)
    $ProcessStartInfo.EnvironmentVariables[$Name] = $Value
}

function Invoke-NavProbeAssertions {
    param([string[]]$ScenarioIds, [bool]$IncludeSummary)
    $assertArgs = @("tools/nav_probe_assert.py", $logPath, $specPath, "--platform", "windows")
    foreach ($scenarioId in $ScenarioIds) {
        $assertArgs += @("--scenario", [string]$scenarioId)
    }
    if ($IncludeSummary) {
        $assertArgs += @("--summary")
    }
    Invoke-NavProbePython $assertArgs
    if ($LASTEXITCODE -ne 0) {
        throw "navigation probe assertions failed with exit code $LASTEXITCODE"
    }
}

Assert-RepoPath $specPath "Spec"
Assert-RepoPath $logPath "Log"
if ($RepeatCount -lt 1) {
    throw "RepeatCount must be at least 1"
}

$specJson = Get-Content -LiteralPath $specPath -Raw | ConvertFrom-Json
$scenarios = @(
    $specJson.scenarios | Where-Object {
        $_.platforms -contains "windows" -and $_.driver -and $_.driver.windows
    }
)
if ($CtrlOnly) {
    $scenarios = @($scenarios | Where-Object { [string]$_.id -like "ctrl_*" })
}
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
if ($List) {
    foreach ($scenario in $scenarios) {
        Write-Output ([string]$scenario.id)
    }
    exit 0
}

Add-Type @'
using System;
using System.ComponentModel;
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
    public static extern IntPtr GetForegroundWindow();
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
    public static bool IsForegroundWindow(IntPtr hWnd) {
        return GetForegroundWindow() == hWnd;
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
    const uint INPUT_KEYBOARD = 1;
    const uint KEYEVENTF_EXTENDEDKEY = 0x0001;
    const uint KEYEVENTF_KEYUP = 0x0002;
    const ushort VK_CONTROL = 0x11;
    const ushort VK_SHIFT = 0x10;
    const ushort VK_MENU = 0x12;
    [StructLayout(LayoutKind.Sequential)]
    struct INPUT {
        public uint type;
        public InputUnion input;
    }
    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion {
        [FieldOffset(0)]
        public KEYBDINPUT keyboard;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct KEYBDINPUT {
        public ushort wVk;
        public ushort wScan;
        public uint dwFlags;
        public uint time;
        public UIntPtr dwExtraInfo;
    }
    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint nInputs, INPUT[] pInputs, int cbSize);
    static void SendKey(ushort vk, bool keyUp, bool extended) {
        INPUT input = new INPUT();
        input.type = INPUT_KEYBOARD;
        input.input.keyboard.wVk = vk;
        input.input.keyboard.wScan = 0;
        input.input.keyboard.dwFlags = (keyUp ? KEYEVENTF_KEYUP : 0) | (extended ? KEYEVENTF_EXTENDEDKEY : 0);
        input.input.keyboard.time = 0;
        input.input.keyboard.dwExtraInfo = UIntPtr.Zero;
        INPUT[] inputs = new INPUT[] { input };
        uint sent = SendInput(1, inputs, Marshal.SizeOf(typeof(INPUT)));
        if (sent != 1) {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "SendInput failed");
        }
    }
    static void Down(ushort vk, bool extended) { SendKey(vk, false, extended); }
    static void Up(ushort vk, bool extended) { SendKey(vk, true, extended); }
    public static void SendChord(ushort key, bool ctrl, bool shift, bool extended) {
        try {
            if (ctrl) { Down(VK_CONTROL, false); }
            if (shift) { Down(VK_SHIFT, false); }
            Down(key, extended);
            Up(key, extended);
        } finally {
            if (shift) { Up(VK_SHIFT, false); }
            if (ctrl) { Up(VK_CONTROL, false); }
        }
    }
    public static void ReleaseModifiers() {
        Up(VK_MENU, false);
        Up(VK_SHIFT, false);
        Up(VK_CONTROL, false);
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
    $extended = $true
    $vk = switch ($name.ToUpperInvariant()) {
        "LEFT" { 0x25 }
        "RIGHT" { 0x27 }
        "UP" { 0x26 }
        "DOWN" { 0x28 }
        "HOME" { 0x24 }
        "END" { 0x23 }
        "PGUP" { 0x21 }
        "PGDN" { 0x22 }
        "BACKSPACE" { $extended = $false; 0x08 }
        "DEL" { 0x2E }
        default { throw "Unsupported navigation chord: $Chord" }
    }
    [LocalPasteNavProbeInput]::SendChord([UInt16]$vk, [bool]$ctrl, [bool]$shift, [bool]$extended)
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

function Wait-NavProbeForeground {
    param([System.Diagnostics.Process]$Process, [IntPtr]$Handle, [string]$ScenarioId)
    for ($idx = 0; $idx -lt $ForegroundPollCount; $idx++) {
        Set-NavProbeForeground $Process $Handle
        if ([LocalPasteNavProbeWindow]::IsForegroundWindow($Handle)) {
            return
        }
        Start-Sleep -Milliseconds $ForegroundPollMs
    }
    throw "localpaste-gui did not become foreground window before input for $ScenarioId"
}

function Stop-NavProbeProcess {
    param([System.Diagnostics.Process]$Process)
    if ($null -eq $Process) {
        return
    }
    try {
        $Process.Refresh()
        if ($Process.HasExited) {
            return
        }
        $Process.CloseMainWindow() | Out-Null
        if ($Process.WaitForExit(5000)) {
            return
        }
        try {
            $Process.Kill()
        }
        catch {
            Write-Warning "failed to kill nav probe process $($Process.Id): $_"
            return
        }
        if (-not $Process.WaitForExit(5000)) {
            Write-Warning "nav probe process $($Process.Id) did not exit after Kill()"
        }
    }
    finally {
        $Process.Dispose()
    }
}

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

$wscript = New-Object -ComObject WScript.Shell
$sendWithLowLevelInput = (-not $UseSendKeys) -or $UseLowLevelInput
if ($sendWithLowLevelInput) {
    Write-Host "nav probe input driver: SendInput"
}
else {
    Write-Host "nav probe input driver: WScript.SendKeys"
}
if ($Assert) {
    $script:NavProbePython = @(Resolve-NavProbePython)
}

for ($repeatIndex = 1; $repeatIndex -le $RepeatCount; $repeatIndex++) {
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

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.WorkingDirectory = $repo
    $psi.UseShellExecute = $false
    Set-NavProbeProcessEnv $psi "DB_PATH" $dbPath
    Set-NavProbeProcessEnv $psi "LOCALPASTE_NAV_PROBE_LOG" $logPath
    Set-NavProbeProcessEnv $psi "LOCALPASTE_NAV_PROBE_SCENARIO" $scenarioId
    Set-NavProbeProcessEnv $psi "LOCALPASTE_NAV_PROBE_SEED_TEXT" $seedText
    Set-NavProbeProcessEnv $psi "LOCALPASTE_NAV_PROBE_SEED_NAME" "nav-probe"
    Set-NavProbeProcessEnv $psi "LOCALPASTE_NAV_PROBE_FOCUS_EDITOR" "1"
    if ($scenario.seed -and $scenario.seed.cursor) {
        Set-NavProbeProcessEnv $psi "LOCALPASTE_NAV_PROBE_SEED_CURSOR" ([string]$scenario.seed.cursor)
    }

    if ($RepeatCount -gt 1) {
        Write-Host "nav probe: $scenarioId ($repeatIndex/$RepeatCount)"
    }
    else {
        Write-Host "nav probe: $scenarioId"
    }
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
        Wait-NavProbeForeground $proc $handle $scenarioId
        foreach ($key in $scenario.driver.windows.send_keys) {
            try {
                Wait-NavProbeForeground $proc $handle $scenarioId
                Start-Sleep -Milliseconds $AfterFocusMs
                if ($sendWithLowLevelInput) {
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
        Stop-NavProbeProcess $proc
    }
    Start-Sleep -Milliseconds $AfterCloseMs
    if ($Assert -and $RepeatCount -gt 1) {
        Invoke-NavProbeAssertions -ScenarioIds @($scenarioId) -IncludeSummary $false
    }
}
}

Write-Host "nav probe log: $logPath"
if ($Assert) {
    $scenarioIds = @($scenarios | ForEach-Object { [string]$_.id })
    Invoke-NavProbeAssertions -ScenarioIds $scenarioIds -IncludeSummary ([bool]$Summary)
}
