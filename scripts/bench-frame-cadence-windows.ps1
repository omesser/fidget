# Usage: scripts\bench-frame-cadence-windows.ps1 matrix -Seconds 20
#   -Bin target\release\fidget.exe. FIDGET_BENCH_GREEN_LIGHT=1: the operator
#   agreed to fidget on the desktop. Type only numbers it printed.

param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet("idle", "idle-quiet", "walking", "load", "matrix")]
    [string]$Scenario,

    [int]$Seconds = 20,
    [int]$WalkTimeout = 180,
    [string]$Bin = "",
    [string]$Out = ""
)

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

Set-Location (Split-Path -Parent $PSScriptRoot)

if ($env:FIDGET_BENCH_GREEN_LIGHT -ne '1') {
    [Console]::Error.WriteLine("$Scenario launches fidget on the live desktop. Set FIDGET_BENCH_GREEN_LIGHT=1 once the operator has agreed.")
    exit 2
}
if ($env:OS -ne 'Windows_NT') {
    $osEnv = $env:OS
    if (-not $osEnv) { $osEnv = "unset" }
    [Console]::Error.WriteLine("This bench launches fidget on Windows. OS is '$osEnv', not Windows_NT.")
    exit 2
}

if (-not $Bin) {
    if ($env:FIDGET_VERIFY_BIN) {
        $Bin = $env:FIDGET_VERIFY_BIN
    } else {
        $Bin = "target\debug\fidget.exe"
    }
}
if (-not (Test-Path -LiteralPath $Bin)) {
    [Console]::Error.WriteLine("no $Bin; run: cd src-tauri; cargo build --bin fidget  or  cargo build --release --bin fidget")
    exit 2
}
$script:BinPath = (Resolve-Path -LiteralPath $Bin).ProviderPath

if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    [Console]::Error.WriteLine("node is not on PATH, so scripts/frame-cadence.mjs cannot run.")
    exit 2
}

if (-not $Out) {
    $Out = Join-Path ([System.IO.Path]::GetTempPath()) ("fidget-cadence-bench-" + [guid]::NewGuid().ToString("N"))
}
if (-not (Test-Path -LiteralPath $Out)) {
    $null = New-Item -ItemType Directory -Path $Out
}
$Out = (Resolve-Path -LiteralPath $Out).ProviderPath

$script:AppProc = $null
$script:Scratch = $null
$script:Burners = @()
$script:LogWriter = $null
$script:ErrCopy = $null
$script:OutDrain = $null

function Get-CimOrUnavailable([scriptblock]$Read) {
    try {
        $value = & $Read
        if ($null -eq $value) { return "unavailable" }
        $text = "$value".Trim()
        if ($text -eq "") { return "unavailable" }
        return $text
    } catch {
        return "unavailable"
    }
}

function Get-GitRev {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) { return "unknown" }
    $rev = & git rev-parse --short HEAD 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $rev) { return "unknown" }
    return ("$rev").Trim()
}

function Test-AppAlive {
    if ($null -eq $script:AppProc) { return $false }
    try {
        return -not $script:AppProc.HasExited
    } catch {
        return $false
    }
}

function Stop-App {
    if ($null -ne $script:AppProc) {
        try {
            if (-not $script:AppProc.HasExited) {
                # taskkill /T stops this fidget and the WebView2 processes it spawned.
                # A name-wide stop would take every other WebView2 on the machine.
                try {
                    & taskkill.exe /PID $script:AppProc.Id /T /F 2>$null | Out-Null
                } catch {
                    Stop-Process -Id $script:AppProc.Id -Force -ErrorAction SilentlyContinue
                }
                try { [void]$script:AppProc.WaitForExit(3000) } catch {}
                try {
                    if (-not $script:AppProc.HasExited) {
                        Stop-Process -Id $script:AppProc.Id -Force -ErrorAction SilentlyContinue
                        try { [void]$script:AppProc.WaitForExit(3000) } catch {}
                    }
                } catch {}
            }
        } catch {}
    }
    # The pipe closes on exit, and EndInvoke returns after the last line is written.
    Stop-LineReader $script:ErrCopy
    $script:ErrCopy = $null
    Stop-LineReader $script:OutDrain
    $script:OutDrain = $null
    if ($null -ne $script:AppProc) {
        try { $script:AppProc.Dispose() } catch {}
        $script:AppProc = $null
    }
    if ($null -ne $script:LogWriter) {
        try { $script:LogWriter.Flush() } catch {}
        try { $script:LogWriter.Dispose() } catch {}
        $script:LogWriter = $null
    }
    if ($script:Scratch) {
        Remove-Item -LiteralPath $script:Scratch -Recurse -Force -ErrorAction SilentlyContinue
        $script:Scratch = $null
    }
}

function Stop-Burners {
    foreach ($proc in @($script:Burners)) {
        if ($null -eq $proc) { continue }
        try {
            if (-not $proc.HasExited) {
                Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
            }
        } catch {}
        try { $proc.Dispose() } catch {}
    }
    $script:Burners = @()
}

function Start-Burners {
    $script:Burners = @()
    $count = [Environment]::ProcessorCount
    for ($i = 0; $i -lt $count; $i++) {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = "powershell.exe"
        $psi.Arguments = '-NoProfile -Command "while ($true) {}"'
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $true
        $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
        $script:Burners += [System.Diagnostics.Process]::Start($psi)
    }
}

function Read-LogText([string]$Path, [switch]$Soft) {
    $lastError = $null
    $tries = 1
    if (-not $Soft) { $tries = 20 }
    for ($attempt = 0; $attempt -lt $tries; $attempt++) {
        if (-not (Test-Path -LiteralPath $Path)) {
            if ($Soft) { return "" }
            $lastError = "missing"
            Start-Sleep -Milliseconds 50
            continue
        }
        $stream = $null
        $reader = $null
        try {
            $stream = New-Object System.IO.FileStream(
                $Path,
                [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read,
                [System.IO.FileShare]::ReadWrite
            )
            $reader = New-Object System.IO.StreamReader($stream, [System.Text.Encoding]::UTF8, $true)
            return $reader.ReadToEnd()
        } catch {
            $lastError = $_
            if ($Soft) { return "" }
            Start-Sleep -Milliseconds 50
        } finally {
            if ($null -ne $reader) { $reader.Dispose() }
            elseif ($null -ne $stream) { $stream.Dispose() }
        }
    }
    throw "could not read ${Path}: $lastError"
}

function New-StillCharacters {
    $dir = Join-Path $Out "still-characters"
    if (Test-Path -LiteralPath $dir) {
        Remove-Item -LiteralPath $dir -Recurse -Force
    }
    $null = New-Item -ItemType Directory -Path $dir
    $source = Join-Path (Join-Path (Get-Location).Path "characters") "bmo"
    Copy-Item -LiteralPath $source -Destination $dir -Recurse
    $manifest = Join-Path (Join-Path $dir "bmo") "character.manifest"
    $section = ""
    $rewritten = New-Object System.Collections.Generic.List[string]
    foreach ($line in [System.IO.File]::ReadAllLines($manifest)) {
        if ($line.StartsWith("[")) { $section = $line }
        $next = $line
        if ($line.StartsWith("weight = ") -and $section.StartsWith("[behaviors.") -and ($section -ne "[behaviors.fidget]")) {
            $next = "weight = 0"
        }
        $rewritten.Add($next)
    }
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllLines($manifest, $rewritten.ToArray(), $utf8)
    return $dir
}

function Start-LineReader($Reader, $Writer) {
    $hostPs = [powershell]::Create()
    if ($null -ne $Writer) {
        $null = $hostPs.AddScript({
            param($reader, $writer)
            try {
                while ($null -ne ($line = $reader.ReadLine())) {
                    $writer.WriteLine($line)
                }
            } catch {}
        }).AddArgument($Reader).AddArgument($Writer)
    } else {
        $null = $hostPs.AddScript({
            param($reader)
            try {
                while ($null -ne ($line = $reader.ReadLine())) {}
            } catch {}
        }).AddArgument($Reader)
    }
    return @{ Host = $hostPs; Async = $hostPs.BeginInvoke() }
}

function Stop-LineReader($Job) {
    if ($null -eq $Job) { return }
    try { $null = $Job.Host.EndInvoke($Job.Async) } catch {}
    try { $Job.Host.Dispose() } catch {}
}

function Start-Fidget([string]$Log, [string]$TraceFrames, [string]$Characters) {
    # dirs-sys reads the roaming known folder, not HOME. A default folder path
    # expands %USERPROFILE%, and that call does not read APPDATA, so scratch is
    # APPDATA, LOCALAPPDATA, USERPROFILE, and HOME.
    $script:Scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("fidget-cadence-scratch-" + [guid]::NewGuid().ToString("N"))
    $null = New-Item -ItemType Directory -Path $script:Scratch

    $instances = $env:FIDGET_INSTANCES
    if (-not $instances) { $instances = "BMO" }

    $utf8 = New-Object System.Text.UTF8Encoding $false
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $script:BinPath
    $psi.WorkingDirectory = (Get-Location).Path
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardOutput = $true
    $psi.StandardErrorEncoding = $utf8
    $psi.StandardOutputEncoding = $utf8
    $psi.EnvironmentVariables["APPDATA"] = $script:Scratch
    $psi.EnvironmentVariables["LOCALAPPDATA"] = $script:Scratch
    $psi.EnvironmentVariables["USERPROFILE"] = $script:Scratch
    $psi.EnvironmentVariables["HOME"] = $script:Scratch
    $psi.EnvironmentVariables["FIDGET_DIRECTOR_API_KEY"] = "bench-placeholder"
    $psi.EnvironmentVariables["FIDGET_DIRECTOR"] = "0"
    $psi.EnvironmentVariables["FIDGET_TRACE_FRAMES"] = $TraceFrames
    $psi.EnvironmentVariables["FIDGET_TRACE_CADENCE"] = "1"
    $psi.EnvironmentVariables["FIDGET_INSTANCES"] = $instances
    $psi.EnvironmentVariables["FIDGET_CHARACTERS"] = $Characters

    $writer = New-Object System.IO.StreamWriter($Log, $false, $utf8)
    $writer.NewLine = "`n"
    $writer.AutoFlush = $true
    $script:LogWriter = $writer

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    $null = $proc.Start()
    $script:AppProc = $proc
    $script:ErrCopy = Start-LineReader $proc.StandardError $writer
    # Stdout is discarded as it arrives so a full pipe cannot stall the process.
    $script:OutDrain = Start-LineReader $proc.StandardOutput $null
}

function Wait-Landed([string]$Log) {
    for ($i = 0; $i -lt 80; $i++) {
        $text = Read-LogText $Log -Soft
        if ($text -cmatch '(?m)^frame: .* (Grounded|Perched) ') { return $true }
        if (-not (Test-AppAlive)) { return $false }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Wait-Walk([string]$Log) {
    $deadline = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() + ([long]$WalkTimeout * 1000)
    while ([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -lt $deadline) {
        $text = Read-LogText $Log -Soft
        if ($text -cmatch ' (walk|ballwalk)#') { return $true }
        if (-not (Test-AppAlive)) { return $false }
        Start-Sleep -Milliseconds 500
    }
    return $false
}

function Get-WalkCount([string]$Log, [long]$From, [long]$To) {
    $text = Read-LogText $Log
    $n = 0
    foreach ($line in @($text -split "\r?\n")) {
        if ($line -cnotmatch '^frame: ') { continue }
        if ($line -cnotmatch ' (walk|ballwalk)#') { continue }
        $fields = $line.Split(" ")
        if ($fields.Length -lt 2) { continue }
        $at = [long]0
        if (-not [long]::TryParse($fields[1], [ref]$at)) { continue }
        if ($at -ge $From -and $at -le $To) { $n = $n + 1 }
    }
    return $n
}

function Write-ScenarioReport([string]$Name, [string]$Log, [long]$From, [long]$To, $Walks) {
    $stdout = & node scripts/frame-cadence.mjs $Log --from "$From" --to "$To"
    $code = $LASTEXITCODE
    $report = ""
    if ($null -ne $stdout) {
        $report = (@($stdout) -join "`n")
    }
    $text = "## $Name`n`nwindow: $From..$To ms, walk frames: $Walks`n`n$report`n"
    $path = Join-Path $Out "$Name.md"
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($path, $text, $utf8)
    [Console]::Out.Write($text)
    [Console]::Out.WriteLine()
    if ($code -ne 0) {
        [Console]::Error.WriteLine("${Name}: node scripts/frame-cadence.mjs exited $code; see $Log")
        return $false
    }
    return $true
}

function Invoke-Scenario([string]$Name) {
    $log = Join-Path $Out "$Name.log"
    if ($Name -eq "load") {
        Start-Burners
    }
    $idle = $false
    $characters = $env:FIDGET_CHARACTERS
    if (-not $characters) {
        $characters = Join-Path (Get-Location).Path "characters"
    }
    if ($Name -eq "idle" -or $Name -eq "idle-quiet") {
        $idle = $true
        $characters = New-StillCharacters
    }
    if ($Name -eq "idle-quiet") {
        Start-Fidget $log "0" $characters
        Start-Sleep -Seconds 5
    } else {
        Start-Fidget $log "1" $characters
        if (-not (Wait-Landed $log)) {
            [Console]::Error.WriteLine("${Name}: the sprite never landed; see $log")
            Stop-App
            Stop-Burners
            return $false
        }
    }
    Start-Sleep -Seconds 2
    if (-not $idle) {
        if (-not (Wait-Walk $log)) {
            [Console]::Error.WriteLine("${Name}: no walk frame within ${WalkTimeout}s; see $log")
            Stop-App
            Stop-Burners
            return $false
        }
    }
    $from = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Start-Sleep -Seconds $Seconds
    $to = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Start-Sleep -Milliseconds 1500
    Stop-App
    Stop-Burners
    $walks = "untraced"
    if ($Name -ne "idle-quiet") {
        $walks = Get-WalkCount $log $from $to
    }
    if (-not (Write-ScenarioReport $Name $log $from $to $walks)) {
        return $false
    }
    if ($Name -eq "idle" -and [int]$walks -gt 0) {
        [Console]::Error.WriteLine("${Name}: BMO walked $walks frames in the window, so this is not an idle sample; see $log")
        return $false
    }
    return $true
}

$machine = Get-CimOrUnavailable { (Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop | Select-Object -First 1).Model }
$osName = Get-CimOrUnavailable { (Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop | Select-Object -First 1).Caption }
$refresh = Get-CimOrUnavailable { (Get-CimInstance -ClassName Win32_VideoController -ErrorAction Stop | Select-Object -First 1).CurrentRefreshRate }
[Console]::Out.WriteLine("machine=$machine")
[Console]::Out.WriteLine("os=$osName")
[Console]::Out.WriteLine("refresh_hz=$refresh")
[Console]::Out.WriteLine("cpus=$([Environment]::ProcessorCount)")
[Console]::Out.WriteLine("bin=$Bin")
[Console]::Out.WriteLine("git_rev=$(Get-GitRev)")
[Console]::Out.WriteLine("seconds=$Seconds")
[Console]::Out.WriteLine("")

$failed = $false
try {
    switch ($Scenario) {
        "matrix" {
            if (-not (Invoke-Scenario "idle")) { $failed = $true; break }
            if (-not (Invoke-Scenario "idle-quiet")) { $failed = $true; break }
            if (-not (Invoke-Scenario "walking")) { $failed = $true; break }
            if (-not (Invoke-Scenario "load")) { $failed = $true; break }
        }
        default {
            if (-not (Invoke-Scenario $Scenario)) { $failed = $true }
        }
    }
} finally {
    Stop-App
    Stop-Burners
}

if ($failed) { exit 1 }
[Console]::Out.WriteLine("out=$Out")
