# Fold a WPR trace from scripts\bench-mask-rebuild-windows-flame.ps1 into a
# flamegraph of fidget's CPU samples (#428). No admin: recording needs it,
# reading the trace does not.
# Usage: scripts\export-mask-flame-windows.ps1 [-Etl PATH\trace.etl] [-Out DIR]
#          [-Process fidget.exe] [-Focus REGEX]
#        scripts\export-mask-flame-windows.ps1 -Csv PATH\export.csv [-Out DIR] ...
#
# -Etl defaults to the newest E:\workspace\fidget-428-flame\*\trace.etl, and
# -Out to an "export" folder beside it. wpaexporter.exe, from the Windows
# Performance Toolkit, writes the CPU Usage (Sampled) table to CSV with the
# scripts\wpa profile, resolving fidget's frames from the fidget.pdb beside
# the trace and Windows frames from Microsoft's symbol server, cached under
# E:\workspace\symbols. The first export downloads those symbols, so it is
# the slow one. -Csv skips wpaexporter and folds a CSV already exported.
#
# Writes, in -Out:
#   folded.txt       every -Process sample, one "frame;frame;... count" line
#                    per stack, for inferno-flamegraph or speedscope.app
#   mask-folded.txt  the stacks with a frame matching -Focus, the mask path
#   summary.txt      sample counts; a sample is about 1 ms on a CPU
#   flame.svg, mask-flame.svg, when inferno-flamegraph is on PATH
#     (cargo install inferno)

param(
    [string]$Etl = "",
    [string]$Csv = "",
    [string]$Out = "",
    [string]$Process = "fidget.exe",
    # Release inlines apply_input_mask, so the region calls and the function
    # that keeps its name both count.
    [string]$Focus = 'update_input_region|apply_input_mask|SetWindowRgn|CombineRgn|CreateRectRgn'
)

$ErrorActionPreference = "Stop"

# One CSV row is one sample. Its stack runs from the root, frames split by
# "/", as "module!function"; "n/a" or nothing when no stack was taken.
# Returns folded stacks, root first, mapped to sample counts.
function Get-FoldedStacks {
    param([object[]]$Rows, [string]$Process)
    $counts = @{}
    foreach ($row in $Rows) {
        # "fidget.exe (1234)": the image name, then the process id.
        if (-not ([string]$row.Process).StartsWith("$Process ", [StringComparison]::OrdinalIgnoreCase) -and
            [string]$row.Process -ne $Process) { continue }
        $stack = [string]$row.Stack
        $frames = @()
        if ($stack -and $stack -ne 'n/a') {
            $frames = @($stack -split '/' | Where-Object { $_ -and $_ -ne '[Root]' } | ForEach-Object { $_ -replace ';', ':' })
        }
        if ($frames.Count -eq 0) { $frames = @('[no stack]') }
        $key = $frames -join ';'
        $counts[$key] = 1 + [int]$counts[$key]
    }
    $counts
}

function Write-Folded {
    param([hashtable]$Counts, [string]$Path)
    $lines = @($Counts.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Name) $($_.Value)" })
    [System.IO.File]::WriteAllLines($Path, [string[]]$lines)
}

if ($Csv) {
    if (-not (Test-Path -LiteralPath $Csv)) {
        [Console]::Error.WriteLine("no CSV at $Csv")
        exit 2
    }
    $Csv = (Resolve-Path -LiteralPath $Csv).Path
    if (-not $Out) { $Out = Split-Path -Parent $Csv }
} else {
    if (-not $Etl) {
        $runs = "E:\workspace\fidget-428-flame"
        $newest = Get-ChildItem -Path $runs -Filter trace.etl -Recurse -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime | Select-Object -Last 1
        if (-not $newest) {
            [Console]::Error.WriteLine("no trace.etl under $runs; record one with scripts\bench-mask-rebuild-windows-flame.ps1 or pass -Etl")
            exit 2
        }
        $Etl = $newest.FullName
    }
    if (-not (Test-Path -LiteralPath $Etl)) {
        [Console]::Error.WriteLine("no trace at $Etl")
        exit 2
    }
    $Etl = (Resolve-Path -LiteralPath $Etl).Path
    $exporter = (Get-Command wpaexporter.exe -ErrorAction SilentlyContinue).Source
    if (-not $exporter) {
        $kit = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\Windows Performance Toolkit\wpaexporter.exe"
        if (Test-Path -LiteralPath $kit) { $exporter = $kit }
    }
    if (-not $exporter) {
        [Console]::Error.WriteLine("wpaexporter.exe is not installed. It ships with the Windows Performance Toolkit:")
        [Console]::Error.WriteLine("  winget install --id Microsoft.WindowsADK --override `"/features OptionId.WindowsPerformanceToolkit /quiet`"")
        [Console]::Error.WriteLine("which installs that one feature, about 150 MB, and asks for admin once.")
        [Console]::Error.WriteLine("Or export by hand in WPA and pass the CSV with -Csv.")
        exit 2
    }
    if (-not $Out) { $Out = Join-Path (Split-Path -Parent $Etl) "export" }
    $null = New-Item -ItemType Directory -Force -Path $Out
    $Out = (Resolve-Path -LiteralPath $Out).Path
    $wpaProfile = Join-Path $PSScriptRoot "wpa\ExportCPUUsageSampled.wpaProfile"
    $env:_NT_SYMBOL_PATH = "$(Split-Path -Parent $Etl);srv*E:\workspace\symbols*https://msdl.microsoft.com/download/symbols"
    Write-Host "exporting $Etl with $exporter (symbols load first; minutes on a first run)"
    & $exporter -i $Etl -profile $wpaProfile -symbols -outputfolder $Out
    if ($LASTEXITCODE -ne 0) {
        [Console]::Error.WriteLine("wpaexporter failed ($LASTEXITCODE)")
        exit 1
    }
    $Csv = Get-ChildItem -LiteralPath $Out -Filter "CPU_Usage*.csv" | Sort-Object LastWriteTime | Select-Object -Last 1 -ExpandProperty FullName
    if (-not $Csv) {
        [Console]::Error.WriteLine("wpaexporter wrote no CPU_Usage*.csv to $Out")
        exit 1
    }
}

$null = New-Item -ItemType Directory -Force -Path $Out
$Out = (Resolve-Path -LiteralPath $Out).Path
$rows = @(Import-Csv -LiteralPath $Csv)
$all = Get-FoldedStacks -Rows $rows -Process $Process
$mask = @{}
foreach ($entry in $all.GetEnumerator()) {
    if ($entry.Name -split ';' | Where-Object { $_ -match $Focus }) { $mask[$entry.Name] = $entry.Value }
}
$folded = Join-Path $Out "folded.txt"
$maskFolded = Join-Path $Out "mask-folded.txt"
Write-Folded -Counts $all -Path $folded
Write-Folded -Counts $mask -Path $maskFolded

$total = ($all.Values | Measure-Object -Sum).Sum
$inMask = ($mask.Values | Measure-Object -Sum).Sum
if (-not $total) { $total = 0 }
if (-not $inMask) { $inMask = 0 }
$share = if ($total -gt 0) { "{0:P1}" -f ($inMask / $total) } else { "n/a" }
$summary = @(
    "csv: $Csv"
    "$Process samples: $total (about 1 ms each)"
    "mask path samples: $inMask ($share), frames matching $Focus"
    "folded: $folded"
    "mask folded: $maskFolded"
)

$inferno = Get-Command inferno-flamegraph -ErrorAction SilentlyContinue
if ($inferno) {
    foreach ($pair in @(@($folded, "flame.svg", "fidget"), @($maskFolded, "mask-flame.svg", "fidget mask path"))) {
        $svg = Join-Path $Out $pair[1]
        if ((Get-Item -LiteralPath $pair[0]).Length -eq 0) { continue }
        $text = & $inferno.Source --title "$($pair[2]) (#428)" $pair[0]
        [System.IO.File]::WriteAllLines($svg, [string[]]$text)
        $summary += "svg: $svg"
    }
} else {
    $summary += "svg: none; cargo install inferno for inferno-flamegraph, or open folded.txt at https://www.speedscope.app"
}

$summary | Set-Content -LiteralPath (Join-Path $Out "summary.txt") -Encoding UTF8
$summary | ForEach-Object { Write-Host $_ }
if ($total -eq 0) {
    [Console]::Error.WriteLine("no $Process samples in $Csv")
    exit 1
}
