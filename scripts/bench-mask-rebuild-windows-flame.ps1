# Record a WPR CPU trace, with stacks, while fidget rebuilds its click-through
# region (#428). Windows only, from an elevated PowerShell: WPR needs admin.
# Usage: scripts\bench-mask-rebuild-windows-flame.ps1 -Bin PATH\fidget.exe [-Out DIR] [-Target N] [-Seconds N]
#          [-HalfWidth N -HalfHeight N] [-Instances NAME] [-Scale N] [-Check]
#   FIDGET_BENCH_GREEN_LIGHT=1: the operator agreed to fidget on the desktop
#   and to this script moving the pointer. Hands off the mouse while it runs.
#
# The pointer is held on the sprite's centre, which is what gates a rebuild,
# until Target region rebuilds land or Seconds pass. Target defaults to 100.
# Seconds defaults to Target / 2 + 15, which allows 2 rebuilds a second, half
# the rate seen on X11 (#428). The pointer goes to the sprite's top-left from
# each frame line plus half the sprite size from the "overlay: ... sprite WxH"
# line. HalfWidth and HalfHeight override that, and BMO's 63x64 is the
# fallback when the line is missing. The pointer moves once a display refresh,
# to the newest frame line (scripts\bench-mask-rebuild-windows-steer.cs says why).
#
# Check steers without WPR or admin, for Seconds (default 20), and reports how
# old each frame line was when the pointer moved and how many rebuilds landed.
# Rebuilds near the frame rate mean the pointer stayed on the sprite.
#
# Open trace.etl in WPA: CPU Usage (Sampled), process fidget.exe, Flame by
# Stack, and search for SetWindowRgn. Release builds can inline
# overlay::apply_input_mask into overlay::update_input_region, or further into
# the frame_loop closure that calls it, so read the frames above SetWindowRgn
# and CombineRgn rather than looking for apply_input_mask by name.
# Build with CARGO_PROFILE_RELEASE_DEBUG=line-tables-only so the PDB has frames.
# scripts\export-mask-flame-windows.ps1 turns trace.etl into a flamegraph
# without WPA's UI or admin.
#
# Instances names a shipped package under characters\ (BMO, the large one at
# 126x128; Nim, the small one at 32x32 drawn 4x). Scale draws a copy of it at
# that scale instead, with the copy under Out\characters.

param(
    [Parameter(Mandatory = $true)]
    [string]$Bin,
    [string]$Out = "",
    [int]$Target = 100,
    # 0: Target / 2 + 15.
    [int]$Seconds = 0,
    # 0: half the size on the overlay line.
    [int]$HalfWidth = 0,
    [int]$HalfHeight = 0,
    [string]$Instances = "BMO",
    # 0: the manifest's scale.
    [int]$Scale = 0,
    [switch]$Check
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path -Parent $PSScriptRoot)

if ($env:FIDGET_BENCH_GREEN_LIGHT -ne '1') {
    [Console]::Error.WriteLine("This launches fidget on the live desktop and moves the pointer. Set FIDGET_BENCH_GREEN_LIGHT=1 once the operator has agreed.")
    exit 2
}
if ($env:OS -ne 'Windows_NT') {
    [Console]::Error.WriteLine("WPR is Windows only. OS is '$env:OS', not Windows_NT.")
    exit 2
}
if (-not $Check) {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        [Console]::Error.WriteLine("WPR needs an elevated PowerShell. Run this again as administrator.")
        exit 2
    }
    if (-not (Get-Command wpr.exe -ErrorAction SilentlyContinue)) {
        [Console]::Error.WriteLine("wpr.exe is not on PATH; it ships with Windows 10 and later in System32.")
        exit 2
}
}
if (-not (Test-Path -LiteralPath $Bin)) {
    [Console]::Error.WriteLine("no $Bin - build: `$env:CARGO_PROFILE_RELEASE_DEBUG='line-tables-only'; cargo build --release -p fidget")
    exit 2
}
$Bin = (Resolve-Path -LiteralPath $Bin).Path
$pdb = [System.IO.Path]::ChangeExtension($Bin, ".pdb")
if (-not (Test-Path -LiteralPath $pdb)) {
    [Console]::Error.WriteLine("warning: no $pdb, so fidget frames will show as addresses")
}

# A WPR session already running is someone else's; this script does not cancel it.
if (-not $Check) {
    & wpr.exe -status | Out-String | Set-Variable status
    if ($status -match 'WPR is recording') {
        [Console]::Error.WriteLine("WPR is already recording. Stop that session first (wpr -cancel or wpr -stop).")
        exit 2
    }
}

if ($Seconds -le 0) { $Seconds = if ($Check) { 20 } else { [int][math]::Ceiling($Target / 2) + 15 } }
if (-not ('FidgetBench.LogFollower' -as [type])) {
    Add-Type -Path (Join-Path $PSScriptRoot "bench-mask-rebuild-windows-steer.cs")
}

if (-not $Out) {
    $Out = Join-Path "E:\workspace\fidget-428-flame" (Get-Date -Format "yyyyMMdd-HHmmss")
}
$null = New-Item -ItemType Directory -Force -Path $Out
$Out = (Resolve-Path -LiteralPath $Out).Path
$scratch = Join-Path $Out "scratch"
$dataDir = Join-Path $Out "data"
$null = New-Item -ItemType Directory -Force -Path $scratch, $dataDir
$processLog = Join-Path $dataDir "process.log"
$etl = Join-Path $Out "trace.etl"

$characters = Join-Path (Get-Location).Path "characters"
if ($Scale -gt 0) {
    $source = Join-Path $characters $Instances.ToLowerInvariant()
    $manifest = Join-Path $source "character.manifest"
    if (-not (Test-Path -LiteralPath $manifest)) {
        [Console]::Error.WriteLine("no $manifest to draw at ${Scale}x")
        exit 2
    }
    $characters = Join-Path $Out "characters"
    $copy = Join-Path $characters (Split-Path -Leaf $source)
    $null = New-Item -ItemType Directory -Force -Path $characters
    Copy-Item -Recurse -LiteralPath $source -Destination $copy
    # scale is a top-level key, so it goes above the first table.
    $body = @(Get-Content -Encoding UTF8 -LiteralPath (Join-Path $copy "character.manifest") | Where-Object { $_ -notmatch '^scale\s*=' })
    # WriteAllLines leaves out the BOM that Windows PowerShell 5.1 adds.
    [System.IO.File]::WriteAllLines((Join-Path $copy "character.manifest"), [string[]](@("scale = $Scale") + $body))
}

# Scratch profile, as scripts\bench-frame-cadence-windows.ps1 does, plus
# FIDGET_DATA_DIR so release's process.log lands where this script reads it.
$launchEnv = @{
    APPDATA = $scratch; LOCALAPPDATA = $scratch; USERPROFILE = $scratch; HOME = $scratch
    FIDGET_DATA_DIR = $dataDir
    FIDGET_DIRECTOR = "0"
    FIDGET_DIRECTOR_API_KEY = "bench-placeholder" # pragma: allowlist secret
    FIDGET_TRACE_FRAMES = "1"
    FIDGET_TRACE_MASK_REBUILD = "1"
    FIDGET_INSTANCES = $Instances
    FIDGET_CHARACTERS = $characters
}
$saved = @{}
foreach ($k in $launchEnv.Keys) {
    $saved[$k] = [Environment]::GetEnvironmentVariable($k)
    [Environment]::SetEnvironmentVariable($k, $launchEnv[$k])
}
try {
    $app = Start-Process -FilePath $Bin -PassThru -WindowStyle Hidden `
        -RedirectStandardOutput (Join-Path $Out "stdout.log") -RedirectStandardError (Join-Path $Out "stderr.log")
} finally {
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
}

$recording = $false
$vblank = $null
function Stop-All {
    if ($script:recording) {
        & wpr.exe -cancel 2>$null | Out-Null
        $script:recording = $false
    }
    if ($script:vblank) { $script:vblank.Dispose(); $script:vblank = $null }
    # taskkill /T also stops the WebView2 processes this fidget spawned.
    & taskkill.exe /PID $app.Id /T /F 2>$null | Out-Null
}
trap { Stop-All; break }

# Each pass waits for the next display refresh, then reads what fidget logged
# since the last one. Returns the follower once it has seen Frames frame lines
# and Rebuilds rebuild lines, or when Seconds pass or fidget exits.
$vblank = New-Object FidgetBench.VBlank
function Read-Log([long]$Since, [int]$Hw, [int]$Hh, [double]$Scale, [int]$Frames, [int]$Rebuilds, [int]$Seconds, [bool]$Steer) {
    $follower = New-Object FidgetBench.LogFollower $processLog, $Since, $Hw, $Hh, $Scale
    $clock = [Diagnostics.Stopwatch]::StartNew()
    try {
        while ($follower.Frames -lt $Frames -or $follower.Rebuilds.Count -lt $Rebuilds) {
            if ($clock.Elapsed.TotalSeconds -ge $Seconds) { break }
            if ($app.HasExited) { [Console]::Error.WriteLine("fidget exited; see $processLog"); break }
            $script:vblank.Wait()
            if ($follower.Step() -and $Steer) { [void][FidgetBench.Screen]::MoveTo($follower.AimX, $follower.AimY) }
        }
    } finally { $follower.Dispose() }
    return $follower
}

if ((Read-Log 0 0 0 1 1 0 60 $false).Frames -eq 0) {
    Stop-All
    [Console]::Error.WriteLine("no frame line within 60s; see $processLog")
    exit 1
}

# The overlay line comes before the first frame line. Select-String reads the
# log the app still has open.
if ($HalfWidth -le 0 -or $HalfHeight -le 0) {
    $overlay = Select-String -LiteralPath $processLog -Pattern 'overlay: \d+ display\(s\); sprite (\d+)x(\d+)' | Select-Object -Last 1
    if ($overlay -and [int]$overlay.Matches[0].Groups[1].Value -gt 0) {
        $w = [int]$overlay.Matches[0].Groups[1].Value
        $h = [int]$overlay.Matches[0].Groups[2].Value
        if ($HalfWidth -le 0) { $HalfWidth = [int]($w / 2) }
        if ($HalfHeight -le 0) { $HalfHeight = [int]($h / 2) }
    } else {
        [Console]::Error.WriteLine("warning: no sprite size on an overlay line; using BMO's 63x64 half size")
        if ($HalfWidth -le 0) { $HalfWidth = 63 }
        if ($HalfHeight -le 0) { $HalfHeight = 64 }
    }
}
$scale = [FidgetBench.Screen]::PrimaryScale()
if ($scale -le 0) { Stop-All; [Console]::Error.WriteLine("no DPI for the primary display"); exit 1 }

if (-not $Check) {
    & wpr.exe -start CPU -filemode
    if ($LASTEXITCODE -ne 0) { Stop-All; [Console]::Error.WriteLine("wpr -start CPU failed ($LASTEXITCODE)"); exit 1 }
    $recording = $true
}
$since = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$drawn = if ($Scale -gt 0) { "$Instances at ${Scale}x" } else { $Instances }
$doing = if ($Check) { "checking" } else { "recording" }
Write-Host "$doing; holding the pointer at sprite + ($HalfWidth,$HalfHeight) points x $scale on $drawn until $Target region rebuilds or ${Seconds}s"

$followed = Read-Log $since $HalfWidth $HalfHeight $scale 0 $Target $Seconds $true
$elapsed = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds() - $since
$lines = @($followed.Rebuilds)

$stopCode = 0
if (-not $Check) {
    Write-Host "stopping WPR (merging the trace takes a while)"
    & wpr.exe -stop $etl "fidget #428 mask rebuild"
    $stopCode = $LASTEXITCODE
    $recording = $false
}
Stop-All

if (-not $Check) {
    # Next to the trace so WPA finds fidget's symbols with the ETL alone.
    Copy-Item -LiteralPath $Bin -Destination $Out
    if (Test-Path -LiteralPath $pdb) { Copy-Item -LiteralPath $pdb -Destination $Out }
}

$ms = @($lines | ForEach-Object { if ($_ -match ', ([\d.]+) ms$') { [double]$Matches[1] } } | Sort-Object)
$rects = @($lines | ForEach-Object { if ($_ -match 'region rebuild (\d+) rects') { [int]$Matches[1] } } | Sort-Object)
$lags = @($followed.LagsMs | Sort-Object)
$summary = "character: $drawn`nrebuilds: $($ms.Count) in ${elapsed}s (target $Target)`nframes: $($followed.Frames)"
if ($lags.Count -gt 0) {
    $summary += "`nframe line age when the pointer moved, ms: median $($lags[[math]::Floor($lags.Count / 2)]) max $($lags[-1])"
}
if ($ms.Count -gt 0) {
    $summary += "`nrebuild ms: min $($ms[0]) median $($ms[[math]::Floor($ms.Count / 2)]) max $($ms[-1])"
    $summary += "`nrects: min $($rects[0]) median $($rects[[math]::Floor($rects.Count / 2)]) max $($rects[-1])"
}
if (-not $Check) {
    $summary += "`nwpr -stop exit: $stopCode`ntrace: $etl`nsymbols: $Out (fidget), srv*E:\workspace\symbols*https://msdl.microsoft.com/download/symbols (Windows)"
}
$summary | Set-Content -LiteralPath (Join-Path $Out "summary.txt") -Encoding UTF8
$lines | Set-Content -LiteralPath (Join-Path $Out "rebuilds.log") -Encoding UTF8
Write-Host $summary
if ($Check) {
    Write-Host "output: $Out (summary.txt, rebuilds.log, process log in data\)"
    exit 0
}
Write-Host "output: $Out (summary.txt, rebuilds.log, trace.etl, process log in data\)"
Write-Host "flamegraph, from a non-elevated PowerShell: scripts\export-mask-flame-windows.ps1 -Etl $etl"
if ($stopCode -ne 0 -or $ms.Count -eq 0) { exit 1 }
