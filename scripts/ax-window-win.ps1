# Drive one window through UI Automation (dump, size, press/link NAME, click or
# menu at -X/-Y, tray ROW). FromHandle, not RootElement.FindFirst: a desktop-wide
# descendant search can hang on a multi-monitor session.
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('dump', 'size', 'press', 'link', 'click', 'tray', 'menu')]
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
. (Join-Path $PSScriptRoot "uia-signal-win.ps1")
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
    public static void Press() {
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);
        // A press and release in one tick read as no click; 80 ms spans a frame.
        System.Threading.Thread.Sleep(80);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);
    }
    public static void RightPress() {
        mouse_event(0x0008, 0, 0, 0, UIntPtr.Zero);
        // WebView2 opens its context menu on a held right press; 400 ms is held.
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

# Where to click: the sprite's middle once two frame lines in a row put it
# within 2 px, else -X/-Y.
function Get-ClickPoint {
    if (-not ($TraceFile -and (Test-Path -LiteralPath $TraceFile) -and $SpriteW -gt 0 -and $SpriteH -gt 0)) {
        return @($X, $Y)
    }
    $full = [System.IO.Path]::GetFullPath($TraceFile)
    $watcher = New-Object System.IO.FileSystemWatcher ([System.IO.Path]::GetDirectoryName($full)), ([System.IO.Path]::GetFileName($full))
    $watcher.NotifyFilter = [System.IO.NotifyFilters]'Size, LastWrite'
    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    try {
        while ($true) {
            $frames = @(Get-Content -LiteralPath $TraceFile |
                    Select-String -Pattern '^\d+\.?\d* frame: .* sprite\((-?\d+),(-?\d+)\) ' | Select-Object -Last 2)
            if ($frames.Count -eq 2) {
                $a = $frames[0].Matches[0].Groups; $b = $frames[1].Matches[0].Groups
                if ([Math]::Abs([int]$a[1].Value - [int]$b[1].Value) -le 2 -and [Math]::Abs([int]$a[2].Value - [int]$b[2].Value) -le 2) {
                    return @(([int]$b[1].Value + [int]($SpriteW / 2)), ([int]$b[2].Value + [int]($SpriteH / 2)))
                }
            }
            $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
            if ($left -le 0) { return @($X, $Y) }
            $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::Changed, $left) | Out-Null
        }
    } finally {
        $watcher.Dispose()
    }
}

# Move the cursor, then press once the overlay's clickable area follows it:
# with -TraceFile and FIDGET_TRACE_HITTEST=1, a new hit-test line says HIT with
# click-through OFF. Fails after 3 s; without -TraceFile, click at once.
function Invoke-Click([int]$ClickX, [int]$ClickY, [switch]$Right) {
    $start = if ($TraceFile -and (Test-Path -LiteralPath $TraceFile)) { (Get-Item -LiteralPath $TraceFile).Length } else { -1 }
    [FidgetWinEnum]::SetCursorPos($ClickX, $ClickY) | Out-Null
    if ($start -ge 0 -and -not (Wait-HitFollowed $start)) {
        Write-Error "the clickable area did not follow the cursor to $ClickX,$ClickY within 3s; is FIDGET_TRACE_HITTEST=1 set?"
        exit 1
    }
    if ($Right) { [FidgetWinEnum]::RightPress() } else { [FidgetWinEnum]::Press() }
}
function Wait-HitFollowed([long]$From) {
    $full = [System.IO.Path]::GetFullPath($TraceFile)
    $watcher = New-Object System.IO.FileSystemWatcher ([System.IO.Path]::GetDirectoryName($full)), ([System.IO.Path]::GetFileName($full))
    $watcher.NotifyFilter = [System.IO.NotifyFilters]'Size, LastWrite'
    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    try {
        while ($true) {
            $stream = [System.IO.File]::Open($full, 'Open', 'Read', 'ReadWrite')
            try {
                [void]$stream.Seek($From, 'Begin')
                $text = (New-Object System.IO.StreamReader $stream).ReadToEnd()
            } finally {
                $stream.Dispose()
            }
            if ($text -match 'hit-test: cursor\(.*\bHIT\s+click-through OFF') { return $true }
            $left = [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds
            if ($left -le 0) { return $false }
            $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::Changed, $left) | Out-Null
        }
    } finally {
        $watcher.Dispose()
    }
}

if ($Command -eq "click") {
    $point = Get-ClickPoint
    Invoke-Click $point[0] $point[1]
    exit 0
}

if ($Command -eq "menu") {
    $point = Get-ClickPoint
    $pidArg = if ($ProcessId -gt 0) { $ProcessId } else { 0 }
    # Listen before the click so a fast MenuOpened is not missed.
    $signal = [FidgetUiaSignal]::new(0)
    try {
        Invoke-Click $point[0] $point[1] -Right
        $deadline = [DateTime]::UtcNow.AddSeconds(3)
        $items = @(Get-PopupMenuItems -FidgetPid $pidArg)
        while ($items.Count -eq 0 -and $signal.Wait([int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)) {
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
        $deadline = [DateTime]::UtcNow.AddSeconds(2)
        while (@(Get-PopupMenuItems -FidgetPid $pidArg).Count -gt 0 -and
            $signal.Wait([int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)) {
        }
    } finally {
        $signal.Dispose()
    }
    exit 0
}

if ($Command -eq "tray") {
    # tray-icon attaches its menu on the click itself, so a real click, not Invoke.
    $taskbar = [FidgetWinEnum]::FindWindow("Shell_TrayWnd", $null)
    $icon = Wait-UiaChange 0 3 {
        $buttons = Find-ByControlType ([System.Windows.Automation.AutomationElement]::FromHandle($taskbar)) ([System.Windows.Automation.ControlType]::Button)
        $buttons | Where-Object { $_.Current.Name -like "fidget*" } | Select-Object -First 1
    }
    if ($null -eq $icon) {
        Write-Error "no fidget icon on the taskbar after 3s; is it in the hidden-icons overflow?"
        exit 1
    }
    $r = $icon.Current.BoundingRectangle
    $signal = [FidgetUiaSignal]::new(0)
    try {
        [FidgetWinEnum]::SetCursorPos([int]($r.X + $r.Width / 2), [int]($r.Y + $r.Height / 2)) | Out-Null
        [FidgetWinEnum]::Press()
        $deadline = [DateTime]::UtcNow.AddSeconds(3)
        $findRow = { Get-PopupMenuItems | Where-Object { $_ -isnot [string] -and $_.Current.Name.StartsWith($Arg3) } | Select-Object -First 1 }
        $row = & $findRow
        while ($null -eq $row -and $signal.Wait([int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)) {
            $row = & $findRow
        }
    } finally {
        $signal.Dispose()
    }
    if ($null -eq $row) {
        Write-Error "the tray menu has no $Arg3 row"
        exit 1
    }
    $signal = [FidgetUiaSignal]::new($ProcessId)
    try {
        Invoke-Element $row
        $deadline = [DateTime]::UtcNow.AddSeconds(3)
        while ($true) {
            if ([FidgetWinEnum]::Find([uint32]$ProcessId, $Title) -ne [IntPtr]::Zero) { exit 0 }
            if (-not $signal.Wait([int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)) { break }
        }
    } finally {
        $signal.Dispose()
    }
    Write-Error "$Title did not open within 3s"
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
# A link through its Invoke pattern: no cursor move, no real click.
if ($Command -eq "link") {
    $found = Find-ByControlType $window ([System.Windows.Automation.ControlType]::Hyperlink) |
        Where-Object { $_.Current.Name -eq $Arg3 } | Select-Object -First 1
    if ($null -eq $found) {
        Write-Error "no link $Arg3"
        exit 1
    }
    Invoke-Element $found
    exit 0
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
