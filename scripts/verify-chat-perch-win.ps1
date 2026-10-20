#!/usr/bin/env pwsh
# Scenario: chat-perch (Windows), Chat as a Perch (#1502), twin of
#   scenarios/chat-perch.sh.
# On screen: one Fidget/BMO instance with Chat open and a fixture Harness.
#   Chat is focused and moved near the top of the primary display, the
#   Character is placed on its ledge, and the waiting question shows a bubble
#   over Chat. The bubble's chat button and the Chat composer each get one
#   real click; the composer gets typed text. Fidget quits at the end.
# Input: two real left clicks and a short typed marker, at points UI
#   Automation has confirmed belong to Fidget. The cursor is put back after.
# Duration: about 1 min, 5 min at most.
# Asserts: focused Chat has no WS_EX_TOPMOST; the Character is Perched on
#   Chat's top edge for 5 frames; the bubble's chat button sits inside Chat and
#   a click on it logs `bubble: open-chat invoked` for the asking Character;
#   a click on the composer focuses it and typed text lands in it.
# Isolation: FIDGET_DATA_DIR, HOME, APPDATA, LOCALAPPDATA, TEMP, TMP and
#   WEBVIEW2_USER_DATA_FOLDER all point under the evidence directory.
#
# Usage: verify-chat-perch-win.ps1 --go <fidget.exe> <fidget test exe> <evidence dir>
# Without --go it prints this header and exits 2.
# No param() block: Windows PowerShell binds `--go` as -Go (see question-bubble.win.ps1).
$Go = if ($args.Count -gt 0) { $args[0] } else { "" }
$ErrorActionPreference = "Stop"

# Pure checks, loaded on their own by tests/verify-chat-perch-win.test.js.
function Test-PerchedFrames([string[]]$Lines, [int]$LedgeY, [int]$Left, [int]$Width) {
    $frames = @($Lines | ForEach-Object {
        if ($_ -match '^\S+ frame: \d+ (\w+) pos\((-?\d+),(-?\d+)\)') {
            [pscustomobject]@{ State = $Matches[1]; X = [int]$Matches[2]; Y = [int]$Matches[3]; Line = $_ }
        }
    })
    if ($frames.Count -lt 5) { return $null }
    $tail = $frames[($frames.Count - 5)..($frames.Count - 1)]
    foreach ($f in $tail) {
        if ($f.State -ne "Perched" -or $f.X -lt $Left -or $f.X -ge ($Left + $Width) -or
            [Math]::Abs($f.Y - $LedgeY) -gt 2) { return $null }
    }
    return @($tail | ForEach-Object { $_.Line })
}

function Test-ClickTraced([string[]]$Lines, [string]$Instance) {
    [bool]($Lines | Where-Object { $_ -ceq "bubble: open-chat invoked instance=$Instance" })
}

function Get-AskInstance([string[]]$Lines) {
    $id = $null
    foreach ($line in $Lines) {
        if ($line -match 'showAsk instance=(\S+)') { $id = $Matches[1] }
    }
    return $id
}

function Get-ButtonRect([string[]]$Dump, [string]$Name) {
    foreach ($line in $Dump) {
        if ($line -match "^button\|$([regex]::Escape($Name))\|(-?\d+),(-?\d+),(\d+),(\d+)$") {
            return @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3], [int]$Matches[4])
        }
    }
    return $null
}

function Test-RectInside([int[]]$Inner, [int[]]$Outer) {
    $Inner[0] -ge $Outer[0] -and $Inner[1] -ge $Outer[1] -and
    ($Inner[0] + $Inner[2]) -le ($Outer[0] + $Outer[2]) -and
    ($Inner[1] + $Inner[3]) -le ($Outer[1] + $Outer[3])
}

if ($Go -ne "--go") {
    $header = @()
    foreach ($line in (Get-Content -LiteralPath $PSCommandPath | Select-Object -Skip 1)) {
        if (-not $line.StartsWith("#")) { break }
        $header += ($line -replace '^# ?', '')
    }
    $header -join "`n"
    exit 2
}
if ($args.Count -ne 4) {
    Write-Error "usage: verify-chat-perch-win.ps1 --go <fidget.exe> <fidget test exe> <evidence dir>"
    exit 1
}
$Bin = (Resolve-Path -LiteralPath $args[1]).Path
$TestBin = (Resolve-Path -LiteralPath $args[2]).Path
$VerifyBin = Join-Path (Split-Path -Parent $Bin) "fidget-verify.exe"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

$out = Join-Path $args[3] ("chat-perch-win-" + (Get-Date -Format "yyyyMMdd-HHmmss"))
$data = Join-Path $out "data"
$homeDir = Join-Path $out "home"
foreach ($d in $data, "$homeDir\AppData\Roaming", "$homeDir\AppData\Local", "$out\tmp", "$out\webview2") {
    New-Item -ItemType Directory -Force -Path $d | Out-Null
}
$result = Join-Path $out "result.txt"
$marks = Join-Path $out "harness.log"
$trace = Join-Path $data "process.log"
$stdout = Join-Path $out "app.log"
$stderr = Join-Path $out "app.err"
Set-Content -LiteralPath $marks -Value "" -Encoding ascii
Set-Content -LiteralPath $result -Value "" -Encoding ascii
$proc = $null

function Fail([string]$Message) {
    Add-Content -LiteralPath $result -Value "FAIL: $Message"
    [Console]::Error.WriteLine("FAIL: $Message")
    [Console]::Error.WriteLine("Evidence: $out")
    exit 1
}
function Pass([string]$Check, [string]$Detail) {
    Add-Content -LiteralPath $result -Value "$Check=pass $Detail"
    Write-Output "PASS $Check $Detail"
}
foreach ($f in $Bin, $TestBin, $VerifyBin) { if (-not (Test-Path -LiteralPath $f)) { Fail "missing $f" } }

$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $data "settings.json"),
    '{"hide_in_fullscreen": false, "first_run_tour_shown": true}', $utf8)

$harness = "$root\scripts\scenarios\fixture-harness.cmd $TestBin script=scenario-asking count=$marks"
if (@($harness -split '\s+').Count -ne 4) { Fail "a Harness path holds a space: $harness" }

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class ChatPerchWin {
    public delegate bool Callback(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb, IntPtr l);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
    [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr h, uint flags);
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    public static IntPtr Find(uint pid, string title, int minWidth) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            if (owner != pid || !IsWindowVisible(h)) return true;
            var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
            RECT r; GetWindowRect(h, out r);
            if (sb.ToString() == title && r.R - r.L >= minWidth) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    // One overlay per display, all titled Fidget: take the one holding (x, y).
    public static IntPtr FindAt(uint pid, string title, int x, int y) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint owner; GetWindowThreadProcessId(h, out owner);
            if (owner != pid || !IsWindowVisible(h)) return true;
            var sb = new StringBuilder(256); GetWindowText(h, sb, 256);
            RECT r; GetWindowRect(h, out r);
            if (sb.ToString() == title && x >= r.L && x < r.R && y >= r.T && y < r.B) { found = h; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    public static IntPtr TopAt(int x, int y) {
        POINT p; p.X = x; p.Y = y;
        return GetAncestor(WindowFromPoint(p), 2);
    }
    public static void Click(int x, int y) {
        POINT back; GetCursorPos(out back);
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(300);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(60);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(300);
        SetCursorPos(back.X, back.Y);
    }
}
"@
[ChatPerchWin]::SetProcessDPIAware() | Out-Null
$UIA = [System.Windows.Automation.AutomationElement]

function Wait-For([int]$Seconds, [scriptblock]$Probe) {
    for ($n = $Seconds * 4; $n -gt 0; $n--) {
        if (& $Probe) { return $true }
        if ($proc.HasExited) { Fail "Fidget exited early; see $stderr" }
        Start-Sleep -Milliseconds 250
    }
    return $false
}
function Lines([string]$File) { if (Test-Path -LiteralPath $File) { @(Get-Content -LiteralPath $File) } else { @() } }
function Rect([IntPtr]$Hwnd) {
    $r = New-Object ChatPerchWin+RECT
    [ChatPerchWin]::GetWindowRect($Hwnd, [ref]$r) | Out-Null
    @($r.L, $r.T, ($r.R - $r.L), ($r.B - $r.T))
}
function Topmost([IntPtr]$Hwnd) { ([ChatPerchWin]::GetWindowLong($Hwnd, -20) -band 0x8) -ne 0 }
function Chat-State {
    $fg = [ChatPerchWin]::GetForegroundWindow()
    "foreground_is_chat=$($fg -eq $chat) topmost=$(Topmost $chat) exstyle=0x{0:X8} rect={1}" -f `
        [ChatPerchWin]::GetWindowLong($chat, -20), ((Rect $chat) -join ",")
}
# UI Automation straight from the HWND: the overlays share one title, so a
# title lookup can dump the wrong display's overlay.
function Dump([IntPtr]$Hwnd, [string]$Label) {
    $all = $UIA::FromHandle($Hwnd).FindAll([System.Windows.Automation.TreeScope]::Subtree,
        [System.Windows.Automation.Condition]::TrueCondition)
    $lines = @(foreach ($el in $all) {
        try {
            $b = $el.Current.BoundingRectangle
            $frame = if ($b.IsEmpty) { "" } else { "{0},{1},{2},{3}" -f [int]$b.X, [int]$b.Y, [int]$b.Width, [int]$b.Height }
            $role = switch ($el.Current.ControlType.ProgrammaticName) {
                "ControlType.Button" { "button" } "ControlType.Edit" { "entry" } default { "other" }
            }
            "$role|$($el.Current.Name -replace "`n", '\n')|$frame"
        } catch { }
    })
    $lines | Set-Content -LiteralPath (Join-Path $out "$Label.ax.txt") -Encoding utf8
    $lines
}
# The point must be Fidget's: Win32 names the top-level window there and UI
# Automation names the element, both without moving the cursor.
function Target-At([int]$X, [int]$Y) {
    $top = [ChatPerchWin]::TopAt($X, $Y)
    $el = $UIA::FromPoint((New-Object System.Windows.Point $X, $Y))
    $path = @()
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    for ($i = 0; $i -lt 4 -and $null -ne $el; $i++) {
        $path += "$($el.Current.ControlType.ProgrammaticName)|$($el.Current.Name)"
        $el = $walker.GetParent($el)
    }
    $name = $path -join " < "
    $type = $path[0]
    $line = "point=$X,$Y hwnd=$top element=$name"
    Add-Content -LiteralPath (Join-Path $out "target-checks.txt") -Value $line
    [pscustomobject]@{ Hwnd = $top; Name = $name; Type = $type; Line = $line }
}

$env:FIDGET_DATA_DIR = $data
$env:HOME = $homeDir
$env:USERPROFILE = $homeDir
$env:APPDATA = "$homeDir\AppData\Roaming"
$env:LOCALAPPDATA = "$homeDir\AppData\Local"
$env:TEMP = "$out\tmp"
$env:TMP = "$out\tmp"
$env:WEBVIEW2_USER_DATA_FOLDER = "$out\webview2"
$env:FIDGET_HARNESS = $harness
$env:FIDGET_OPEN_CHAT = "1"
$env:FIDGET_DEBUG_IPC = "1"
$env:FIDGET_DIRECTOR_WAKE_SECS = "6"
$env:FIDGET_DIRECTOR_API_KEY = "x"
$env:FIDGET_CAPTURABLE = "1"
$env:FIDGET_TRACE_FRAMES = "1"
$env:FIDGET_TRACE_BUBBLE = "1"
$env:FIDGET_CHARACTER = "bmo"
$env:FIDGET_CHARACTERS = Join-Path $root "characters"

$proc = Start-Process -FilePath $Bin -WorkingDirectory $out -PassThru -WindowStyle Hidden `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
@("pid=$($proc.Id)", "commit=$(git -C $root rev-parse HEAD)", "bin=$Bin") |
    Set-Content -LiteralPath (Join-Path $out "run.txt")
try {
    $script:chat = [IntPtr]::Zero
    if (-not (Wait-For 30 { $script:chat = [ChatPerchWin]::Find([uint32]$proc.Id, "BMO", 300); $script:chat -ne [IntPtr]::Zero })) {
        Fail "Chat window did not open"
    }
    $chat = $script:chat
    if (-not (Wait-For 30 { (Lines $marks) -contains "asked" })) { Fail "fixture did not ask its question" }

    $work = [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea
    $r = Rect $chat
    [ChatPerchWin]::SetWindowPos($chat, [IntPtr]::Zero, $work.Left + [int](($work.Width - $r[2]) / 2),
        $work.Top + 160, 0, 0, 0x0015) | Out-Null
    # SetForegroundWindow is refused while another app holds the foreground,
    # so a click on Chat's title bar focuses it, as a user would.
    [ChatPerchWin]::SetForegroundWindow($chat) | Out-Null
    if (-not (Wait-For 1 { [ChatPerchWin]::GetForegroundWindow() -eq $chat })) {
        $r = Rect $chat
        [ChatPerchWin]::Click($r[0] + [int]($r[2] / 2), $r[1] + 12)
    }
    if (-not (Wait-For 5 { [ChatPerchWin]::GetForegroundWindow() -eq $chat })) {
        Fail "Chat did not take the foreground: $(Chat-State)"
    }
    Start-Sleep -Milliseconds 500
    $state = Chat-State
    Set-Content -LiteralPath (Join-Path $out "initial-chat-state.txt") -Value $state
    if (Topmost $chat) { Fail "focused Chat is WS_EX_TOPMOST: $state" }
    Pass "normal_level" $state

    $chatRect = Rect $chat
    Add-Content -LiteralPath (Join-Path $out "run.txt") -Value "chat=$($chatRect -join ',')"
    $ledgeX = $chatRect[0] + [int]($chatRect[2] / 2)
    $ledgeY = $chatRect[1]
    $before = (Lines $trace).Count
    & $VerifyBin --evidence-dir (Join-Path $out "verify") place --x $ledgeX --y ($ledgeY - 60) *>> (Join-Path $out "place.txt")
    if ($LASTEXITCODE -ne 0) { Fail "fidget-verify place failed; see $out\place.txt" }
    $script:perched = $null
    if (-not (Wait-For 10 { $script:perched = Test-PerchedFrames (Lines $trace | Select-Object -Skip $before) $ledgeY $chatRect[0] $chatRect[2]; $null -ne $script:perched })) {
        Fail "Character did not settle Perched on Chat at y=$ledgeY; see $trace"
    }
    $script:perched | Set-Content -LiteralPath (Join-Path $out "ledge-frames.txt")
    if (Topmost $chat) { Fail "Chat turned WS_EX_TOPMOST after the perch: $(Chat-State)" }
    Pass "perch" ($script:perched[-1])

    $overlay = [ChatPerchWin]::FindAt([uint32]$proc.Id, "Fidget", $ledgeX, $ledgeY)
    if ($overlay -eq [IntPtr]::Zero) { Fail "no overlay window titled Fidget over Chat's ledge" }
    $script:button = $null
    if (-not (Wait-For 10 { $script:button = Get-ButtonRect (Dump $overlay "bubble") "chat"; $null -ne $script:button })) {
        Fail "question bubble has no chat button; see $out\bubble.ax.txt"
    }
    $button = $script:button
    if (-not (Test-RectInside $button $chatRect)) {
        Fail "chat button $($button -join ',') is not inside Chat $($chatRect -join ',')"
    }
    $bx = $button[0] + [int]($button[2] / 2)
    $by = $button[1] + [int]($button[3] / 2)
    $hit = Target-At $bx $by
    # Recorded, not asserted: WindowFromPoint skips the overlay's layered
    # window and both pages are titled Fidget, so neither names the button.
    # The open-chat trace after the click is the proof the button took it.
    $ask = Get-AskInstance (Lines $stderr)
    if (-not $ask) { Fail "no showAsk trace names the asking Character; see $stderr" }
    $beforeClick = (Lines $stderr).Count
    [ChatPerchWin]::Click($bx, $by)
    if (-not (Wait-For 5 { Test-ClickTraced (Lines $stderr | Select-Object -Skip $beforeClick) $ask })) {
        Fail "chat button click did not invoke open Chat for $ask; see $stderr"
    }
    if (-not (Wait-For 5 { [ChatPerchWin]::GetForegroundWindow() -eq $chat -and -not (Topmost $chat) })) {
        Fail "Chat was not focused at normal level after the bubble click: $(Chat-State)"
    }
    Pass "bubble_button_click" "button=$($button -join ',') instance=$ask"

    $edits = @(Dump $chat "chat-before" | Where-Object { $_ -match '^entry\|' })
    if ($edits.Count -eq 0) { Fail "no composer entry in Chat; see $out\chat-before.ax.txt" }
    if (-not ($edits[-1] -match '\|(-?\d+),(-?\d+),(\d+),(\d+)$')) { Fail "composer has no frame: $($edits[-1])" }
    $cx = [int]$Matches[1] + [int]([int]$Matches[3] / 2)
    $cy = [int]$Matches[2] + [int]([int]$Matches[4] / 2)
    $hit = Target-At $cx $cy
    if (-not ($hit.Type -like "ControlType.Edit|Ask *")) { Fail "top element at the composer is not Chat's composer: $($hit.Line)" }
    [ChatPerchWin]::Click($cx, $cy)
    $focus = $UIA::FocusedElement
    $focusLine = "focused=$($focus.Current.ControlType.ProgrammaticName)|$($focus.Current.Name) pid=$($focus.Current.ProcessId)"
    Set-Content -LiteralPath (Join-Path $out "composer-focus.txt") -Value $focusLine
    # msedgewebview2.exe owns the page, so the element pid is not Fidget's.
    if ($focus.Current.ControlType -ne [System.Windows.Automation.ControlType]::Edit -or
        -not $focus.Current.Name.StartsWith("Ask ") -or [ChatPerchWin]::GetForegroundWindow() -ne $chat) {
        Fail "composer did not take keyboard focus: $focusLine"
    }
    $marker = "Windows Chat perch"
    [System.Windows.Forms.SendKeys]::SendWait($marker)
    Start-Sleep -Milliseconds 500
    $typed = $focus.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
    Set-Content -LiteralPath (Join-Path $out "composer-value.txt") -Value $typed
    if ($typed -ne $marker) { Fail "composer holds '$typed', expected '$marker'" }
    Pass "composer_click_and_type" "$focusLine value=$typed"

    Write-Output "PASS: all four Windows checks; evidence in $out"
} finally {
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -like "*count=$marks*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
