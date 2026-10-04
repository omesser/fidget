# Dump or resize one top-level window through UI Automation.
# Dump lines are `role|name`. Pass `frames` after the title to append
# `|x,y,w,h` on every line. `size` resizes the window and prints `x,y,w,h`.
# `press NAME [INDEX]` invokes the INDEX-th button named NAME (1-based).
# `click X Y` sends a real left click at that screen point.
# `tray ROW` clicks the taskbar icon named fidget, invokes the menu row whose
# name starts with ROW, then waits for a window titled TITLE.
#
# EnumWindows plus FromHandle, not RootElement.FindFirst: a desktop-wide
# descendant search can hang on a multi-monitor session.
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('dump', 'size', 'press', 'click', 'tray')]
    [string]$Command,
    [Parameter(Mandatory = $true, Position = 1)]
    [int]$ProcessId,
    [Parameter(Mandatory = $true, Position = 2)]
    [string]$Title,
    [Parameter(Position = 3)]
    [string]$Arg3 = "",
    [Parameter(Position = 4)]
    [string]$Arg4 = ""
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class FidgetWinEnum {
    public delegate bool Callback(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(Callback cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder lp, int n);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindow(string cls, string name);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public const uint SWP_NOZORDER = 0x0004;
    public const uint SWP_NOACTIVATE = 0x0010;
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
    public static IntPtr Find(uint pid, string title) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((h, l) => {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            if (owner != pid) return true;
            var sb = new StringBuilder(512);
            GetWindowText(h, sb, sb.Capacity);
            if (sb.ToString().IndexOf(title, StringComparison.OrdinalIgnoreCase) >= 0) {
                found = h;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
"@

function Format-Frame([System.Windows.Rect]$Rect) {
    "{0},{1},{2},{3}" -f [int]$Rect.X, [int]$Rect.Y, [int]$Rect.Width, [int]$Rect.Height
}

function Short-Role($Element) {
    $ct = $Element.Current.ControlType
    if ($ct -eq [System.Windows.Automation.ControlType]::Button) { return "button" }
    if ($ct -eq [System.Windows.Automation.ControlType]::Window) { return "frame" }
    if ($ct -eq [System.Windows.Automation.ControlType]::Text) { return "label" }
    if ($ct -eq [System.Windows.Automation.ControlType]::Hyperlink) { return "link" }
    if ($ct -eq [System.Windows.Automation.ControlType]::Document) { return "web-area" }
    if ($ct -eq [System.Windows.Automation.ControlType]::Pane) {
        try {
            $scroll = $Element.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
            if ($null -ne $scroll) { return "scroll-area" }
        } catch { }
        return "pane"
    }
    return "other"
}

function Write-Node {
    param($Element, [int]$Depth, [bool]$WithFrames)
    if ($null -eq $Element -or $Depth -gt 30) { return }
    $role = "other"
    $name = ""
    try {
        $name = ($Element.Current.Name -replace "`n", "\n")
        $role = Short-Role $Element
    } catch { return }
    $line = "{0}|{1}" -f $role, $name
    if ($WithFrames) {
        $frame = ""
        try { $frame = Format-Frame $Element.Current.BoundingRectangle } catch { }
        $line = "{0}|{1}" -f $line, $frame
    }
    Write-Output $line
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $child = $walker.GetFirstChild($Element)
    while ($null -ne $child) {
        Write-Node -Element $child -Depth ($Depth + 1) -WithFrames $WithFrames
        $child = $walker.GetNextSibling($child)
    }
}

function Invoke-Element($Element) {
    $Element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}

function Find-Named($Root, $ControlType) {
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty, $ControlType)
    $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
}

if ($Command -eq "click") {
    [FidgetWinEnum]::Click([int]$Arg3, [int]$Arg4)
    exit 0
}

if ($Command -eq "tray") {
    # tray-icon attaches its menu on the click itself, so a real click, not Invoke.
    $taskbar = [FidgetWinEnum]::FindWindow("Shell_TrayWnd", $null)
    $icon = $null
    for ($n = 0; $n -lt 80 -and $null -eq $icon; $n++) {
        $buttons = Find-Named ([System.Windows.Automation.AutomationElement]::FromHandle($taskbar)) ([System.Windows.Automation.ControlType]::Button)
        $icon = $buttons | Where-Object { $_.Current.Name -like "fidget*" } | Select-Object -First 1
        if ($null -eq $icon) { Start-Sleep -Milliseconds 250 }
    }
    if ($null -eq $icon) {
        Write-Error "no fidget icon on the taskbar after 20s; is it in the hidden-icons overflow?"
        exit 1
    }
    $r = $icon.Current.BoundingRectangle
    [FidgetWinEnum]::Click([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2))
    $row = $null
    for ($n = 0; $n -lt 20 -and $null -eq $row; $n++) {
        Start-Sleep -Milliseconds 250
        $menu = [FidgetWinEnum]::FindWindow("#32768", $null)
        if ($menu -eq [IntPtr]::Zero) { continue }
        $items = Find-Named ([System.Windows.Automation.AutomationElement]::FromHandle($menu)) ([System.Windows.Automation.ControlType]::MenuItem)
        $row = $items | Where-Object { $_.Current.Name.StartsWith($Arg3) } | Select-Object -First 1
    }
    if ($null -eq $row) {
        Write-Error "the tray menu has no $Arg3 row"
        exit 1
    }
    Invoke-Element $row
    for ($n = 0; $n -lt 40; $n++) {
        if ([FidgetWinEnum]::Find([uint32]$ProcessId, $Title) -ne [IntPtr]::Zero) { exit 0 }
        Start-Sleep -Milliseconds 250
    }
    Write-Error "$Title did not open within 10s"
    exit 1
}

$hwnd = [FidgetWinEnum]::Find([uint32]$ProcessId, $Title)
if ($hwnd -eq [IntPtr]::Zero) {
    Write-Error "no window titled $Title for pid $ProcessId"
    exit 1
}

if ($Command -eq "size") {
    if (-not $Arg3 -or -not $Arg4) {
        Write-Error "usage: ax-window-win.ps1 size -ProcessId PID -Title TITLE WIDTH HEIGHT"
        exit 2
    }
    $width = [int]$Arg3
    $height = [int]$Arg4
    $rect = New-Object FidgetWinEnum+RECT
    if (-not [FidgetWinEnum]::GetWindowRect($hwnd, [ref]$rect)) {
        Write-Error "could not read frame for $Title"
        exit 1
    }
    $ok = [FidgetWinEnum]::SetWindowPos(
        $hwnd,
        [IntPtr]::Zero,
        $rect.Left,
        $rect.Top,
        $width,
        $height,
        [FidgetWinEnum]::SWP_NOZORDER -bor [FidgetWinEnum]::SWP_NOACTIVATE
    )
    if (-not $ok) {
        Write-Error "could not resize $Title"
        exit 1
    }
    if (-not [FidgetWinEnum]::GetWindowRect($hwnd, [ref]$rect)) {
        Write-Error "could not read frame for $Title after resize"
        exit 1
    }
    Write-Output ("{0},{1},{2},{3}" -f $rect.Left, $rect.Top, ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top))
    exit 0
}

$withFrames = ($Arg3 -eq "frames")
$window = [System.Windows.Automation.AutomationElement]::FromHandle($hwnd)
if ($null -eq $window) {
    Write-Error "FromHandle returned nothing for $hwnd"
    exit 1
}
if ($Command -eq "press") {
    $index = if ($Arg4) { [int]$Arg4 } else { 1 }
    $found = @(Find-Named $window ([System.Windows.Automation.ControlType]::Button) | Where-Object { $_.Current.Name -eq $Arg3 })
    if ($index -lt 1 -or $index -gt $found.Count) {
        Write-Error "no button $Arg3 at index $index ($($found.Count) found)"
        exit 1
    }
    Invoke-Element $found[$index - 1]
    exit 0
}
Write-Node -Element $window -Depth 0 -WithFrames $withFrames
exit 0
