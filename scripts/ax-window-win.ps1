# Dump or resize one top-level window through UI Automation.
# Dump lines are `role|name`. Pass `frames` after the title to append
# `|x,y,w,h` on every line. `size` resizes the window and prints `x,y,w,h`.
# `press NAME` invokes the first button named NAME.
# `click -X X -Y Y` sends a real left click at that screen point and takes
# no -ProcessId or -Title.
# `menu -X X -Y Y` right-clicks that point, prints the names of the popup
# menu's items one per line, then presses Escape. It also takes no -ProcessId.
# `tray ROW` clicks the taskbar icon named fidget, invokes the menu row whose
# name starts with ROW, then waits for a window titled TITLE.
#
# EnumWindows plus FromHandle, not RootElement.FindFirst: a desktop-wide
# descendant search can hang on a multi-monitor session.
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('dump', 'size', 'press', 'click', 'tray', 'menu')]
    [string]$Command,
    [Parameter(Position = 1)]
    [int]$ProcessId,
    [Parameter(Position = 2)]
    [string]$Title = "",
    [Parameter(Position = 3)]
    [string]$Arg3 = "",
    [Parameter(Position = 4)]
    [string]$Arg4 = "",
    [int]$X,
    [int]$Y,
    [string]$TraceFile = "",
    [int]$SpriteW = 0,
    [int]$SpriteH = 0
)
$ErrorActionPreference = "Stop"
if ($Command -notin @("click", "menu") -and ($ProcessId -eq 0 -or -not $Title)) {
    Write-Error "$Command takes -ProcessId and -Title"
    exit 2
}
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
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, StringBuilder lp, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    // Every visible top-level window of class `cls`. FindWindow returns the
    // first `#32768` in z-order, which need not be the popup that is open.
    public static IntPtr[] Visible(string cls) { return OfClass(cls, true); }
    public static IntPtr[] OfClass(string cls, bool visibleOnly) {
        var found = new System.Collections.Generic.List<IntPtr>();
        EnumWindows((h, l) => {
            var sb = new StringBuilder(64);
            GetClassName(h, sb, sb.Capacity);
            if (sb.ToString() == cls && (!visibleOnly || IsWindowVisible(h))) found.Add(h);
            return true;
        }, IntPtr.Zero);
        return found.ToArray();
    }
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(800);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(80);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void RightClick(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(800);
        mouse_event(0x0008, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(400);
        mouse_event(0x0010, 0, 0, 0, UIntPtr.Zero);
    }
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    public static void Escape() {
        keybd_event(0x1B, 0, 0, UIntPtr.Zero);
        keybd_event(0x1B, 0, 0x0002, UIntPtr.Zero);
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr hMenu);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetMenuStringW(IntPtr hMenu, uint item, StringBuilder lpString, int maxCount, uint flags);
    public const uint MN_GETHMENU = 0x01E1;
    public const uint SMTO_ABORTIFHUNG = 0x0002;
    public const uint MF_BYPOSITION = 0x0400;
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
    if ($ct -eq [System.Windows.Automation.ControlType]::Edit) { return "entry" }
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

function Find-ByControlType($Root, $ControlType) {
    $cond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty, $ControlType)
    $Root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $cond)
}

function Get-PopupMenuItems {
    param([uint32]$FidgetPid = 0)
    $candidates = [FidgetWinEnum]::OfClass("#32768", $false)
    $results = @()
    foreach ($menu in $candidates) {
        $itemsPid = 0
        [FidgetWinEnum]::GetWindowThreadProcessId($menu, [ref]$itemsPid) | Out-Null
        if ($FidgetPid -ne 0 -and $itemsPid -ne $FidgetPid) { continue }
        try {
            $element = [System.Windows.Automation.AutomationElement]::FromHandle($menu)
            if ($null -eq $element) { continue }
            $items = Find-ByControlType $element ([System.Windows.Automation.ControlType]::MenuItem)
            if ($null -ne $items -and $items.Count -gt 0) {
                return @($items)
            }
            $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
            $child = $walker.GetFirstChild($element)
            while ($null -ne $child) {
                $nestedItems = Find-ByControlType $child ([System.Windows.Automation.ControlType]::MenuItem)
                if ($null -ne $nestedItems -and $nestedItems.Count -gt 0) {
                    return @($nestedItems)
                }
                $child = $walker.GetNextSibling($child)
            }
        } catch {
        }
        $hmenu = [IntPtr]::Zero
        $hmenuResult = [FidgetWinEnum]::SendMessageTimeout($menu, [FidgetWinEnum]::MN_GETHMENU, [IntPtr]::Zero, [IntPtr]::Zero, [FidgetWinEnum]::SMTO_ABORTIFHUNG, 500, [ref]$hmenu)
        if ($hmenu -ne [IntPtr]::Zero) {
            $count = [FidgetWinEnum]::GetMenuItemCount($hmenu)
            if ($count -gt 0) {
                $names = @()
                for ($i = 0; $i -lt $count; $i++) {
                    $sb = New-Object System.Text.StringBuilder 256
                    $len = [FidgetWinEnum]::GetMenuStringW($hmenu, $i, $sb, $sb.Capacity, [FidgetWinEnum]::MF_BYPOSITION)
                    if ($len -gt 0) {
                        $text = $sb.ToString()
                        $text = $text -replace '&', ''
                        if ($text.Contains("`t")) { $text = $text.Substring(0, $text.IndexOf("`t")) }
                        if ($text.Length -gt 0) { $names += $text }
                    }
                }
                if ($names.Count -gt 0) {
                    return $names
                }
            }
        }
    }
    return @()
}

if ($Command -eq "click") {
    $clickX = $X
    $clickY = $Y
    if ($TraceFile -and (Test-Path -LiteralPath $TraceFile) -and $SpriteW -gt 0 -and $SpriteH -gt 0) {
        $settled = $false
        $lastX = $null
        $lastY = $null
        $firstSeenAt = $null
        $deadline = (Get-Date).AddSeconds(3)
        while ((Get-Date) -lt $deadline -and -not $settled) {
            $trace = Get-Content -LiteralPath $TraceFile
            $recent = $trace | Select-String -Pattern '^\d+\.?\d* frame: .* sprite\((-?\d+),(-?\d+)\) ' | Select-Object -Last 1
            if (-not $recent) {
                Start-Sleep -Milliseconds 150
                continue
            }
            $sx = [int]$recent.Matches[0].Groups[1].Value
            $sy = [int]$recent.Matches[0].Groups[2].Value
            if ($null -eq $lastX) {
                $lastX = $sx
                $lastY = $sy
                $firstSeenAt = Get-Date
                Start-Sleep -Milliseconds 150
                continue
            }
            $dx = [Math]::Abs($sx - $lastX)
            $dy = [Math]::Abs($sy - $lastY)
            if ($dx -le 2 -and $dy -le 2) {
                $clickX = $sx + [int]($SpriteW / 2)
                $clickY = $sy + [int]($SpriteH / 2)
                $settled = $true
            } else {
                $lastX = $sx
                $lastY = $sy
                $firstSeenAt = Get-Date
                Start-Sleep -Milliseconds 150
            }
        }
    } else {
        $clickX = $X
        $clickY = $Y
    }
    [FidgetWinEnum]::Click($clickX, $clickY)
    exit 0
}

if ($Command -eq "menu") {
    $clickX = $X
    $clickY = $Y
    if ($TraceFile -and (Test-Path -LiteralPath $TraceFile) -and $SpriteW -gt 0 -and $SpriteH -gt 0) {
        $settled = $false
        $lastX = $null
        $lastY = $null
        $firstSeenAt = $null
        $deadline = (Get-Date).AddSeconds(3)
        while ((Get-Date) -lt $deadline -and -not $settled) {
            $trace = Get-Content -LiteralPath $TraceFile
            $recent = $trace | Select-String -Pattern '^\d+\.?\d* frame: .* sprite\((-?\d+),(-?\d+)\) ' | Select-Object -Last 1
            if (-not $recent) {
                Start-Sleep -Milliseconds 150
                continue
            }
            $sx = [int]$recent.Matches[0].Groups[1].Value
            $sy = [int]$recent.Matches[0].Groups[2].Value
            if ($null -eq $lastX) {
                $lastX = $sx
                $lastY = $sy
                $firstSeenAt = Get-Date
                Start-Sleep -Milliseconds 150
                continue
            }
            $dx = [Math]::Abs($sx - $lastX)
            $dy = [Math]::Abs($sy - $lastY)
            if ($dx -le 2 -and $dy -le 2) {
                $clickX = $sx + [int]($SpriteW / 2)
                $clickY = $sy + [int]($SpriteH / 2)
                $settled = $true
            } else {
                $lastX = $sx
                $lastY = $sy
                $firstSeenAt = Get-Date
                Start-Sleep -Milliseconds 150
            }
        }
    } else {
        $clickX = $X
        $clickY = $Y
    }
    [FidgetWinEnum]::RightClick($clickX, $clickY)
    Start-Sleep -Milliseconds 300
    $items = @()
    $pidArg = if ($ProcessId -gt 0) { $ProcessId } else { 0 }
    for ($n = 0; $n -lt 32 -and $items.Count -eq 0; $n++) {
        Start-Sleep -Milliseconds 150
        $items = @(Get-PopupMenuItems -FidgetPid $pidArg)
    }
    if ($items.Count -eq 0) {
        $allMenus = [FidgetWinEnum]::OfClass("#32768", $false)
        Write-Error ("no menu items found via UIA or Win32; {0} #32768 windows exist" -f $allMenus.Count)
        exit 1
    }
    if ($items[0] -is [string]) {
        $items | ForEach-Object { Write-Output $_ }
    } else {
        $items | ForEach-Object { Write-Output $_.Current.Name }
    }
    [FidgetWinEnum]::Escape()
    $closedDeadline = (Get-Date).AddSeconds(1)
    while ((Get-Date) -lt $closedDeadline) {
        $remaining = @(Get-PopupMenuItems -FidgetPid $pidArg)
        if ($remaining.Count -eq 0) { break }
        Start-Sleep -Milliseconds 50
    }
    Start-Sleep -Milliseconds 500
    exit 0
}

if ($Command -eq "tray") {
    # tray-icon attaches its menu on the click itself, so a real click, not Invoke.
    $taskbar = [FidgetWinEnum]::FindWindow("Shell_TrayWnd", $null)
    $icon = $null
    for ($n = 0; $n -lt 80 -and $null -eq $icon; $n++) {
        $buttons = Find-ByControlType ([System.Windows.Automation.AutomationElement]::FromHandle($taskbar)) ([System.Windows.Automation.ControlType]::Button)
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
        $row = Get-PopupMenuItems | Where-Object { $_.Current.Name.StartsWith($Arg3) } | Select-Object -First 1
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
    $found = Find-ByControlType $window ([System.Windows.Automation.ControlType]::Button) |
        Where-Object { $_.Current.Name -eq $Arg3 } | Select-Object -First 1
    if ($null -eq $found) {
        Write-Error "no button $Arg3"
        exit 1
    }
    Invoke-Element $found
    exit 0
}
Write-Node -Element $window -Depth 0 -WithFrames $withFrames
exit 0
