# Shared waits for Windows scenario leaves. Each re-checks on a wake (a line
# on Fidget's piped stderr, a file change, a UI Automation event) and gives up
# after its seconds. Nothing sleeps or polls.

. (Join-Path $PSScriptRoot "..\uia-signal-win.ps1")

# One wake count, bumped by each piped line and each file change. A pipe since
# Windows reports writes to a file held open (process.log) only once its cache
# flushes (ReadDirectoryChangesW docs). C# 5, so PowerShell 5.1 compiles it.
if (-not ("FidgetSignal" -as [type])) {
    Add-Type @"
using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
public static class FidgetSignal {
    static readonly object gate = new object();
    static long ticks;
    public static long Seen { get { lock (gate) { return ticks; } } }
    public static void Pulse() { lock (gate) { ticks++; Monitor.PulseAll(gate); } }
    // True once the count moves past seen; false when ms run out first.
    public static bool WaitPast(long seen, int ms) {
        DateTime deadline = DateTime.UtcNow.AddMilliseconds(ms);
        lock (gate) {
            while (ticks == seen) {
                int left = (int)(deadline - DateTime.UtcNow).TotalMilliseconds;
                if (left <= 0 || !Monitor.Wait(gate, left)) { return ticks != seen; }
            }
            return true;
        }
    }
    public static FileSystemWatcher Watch(string dir) {
        FileSystemWatcher w = new FileSystemWatcher(dir);
        w.IncludeSubdirectories = true;
        w.NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName | NotifyFilters.Size | NotifyFilters.LastWrite;
        FileSystemEventHandler on = delegate { Pulse(); };
        w.Created += on; w.Changed += on; w.Deleted += on;
        w.Renamed += delegate { Pulse(); };
        w.EnableRaisingEvents = true;
        return w;
    }
    // Each line of a redirected stream into path as UTF-8 with LF, then a
    // wake. At end of stream the log closes and the handler comes off.
    public static void Tee(Process p, bool stderr, string path) {
        StreamWriter log = new StreamWriter(new FileStream(path, FileMode.Append, FileAccess.Write, FileShare.ReadWrite | FileShare.Delete), new UTF8Encoding(false));
        log.NewLine = "\n";
        log.AutoFlush = true;
        DataReceivedEventHandler on = null;
        on = delegate(object sender, DataReceivedEventArgs e) {
            if (e.Data != null) {
                lock (log) { log.WriteLine(e.Data); }
            } else {
                lock (log) { log.Dispose(); }
                if (stderr) { p.ErrorDataReceived -= on; } else { p.OutputDataReceived -= on; }
            }
            Pulse();
        };
        if (stderr) { p.ErrorDataReceived += on; p.BeginErrorReadLine(); }
        else { p.OutputDataReceived += on; p.BeginOutputReadLine(); }
    }
}
"@
}

# Launch Fidget with stdout and stderr piped, as UTF-8, to these logs. Only
# debug builds copy Fidget's stderr lines back from process.log to stderr, so
# scenarios run debug builds. Stdin is a pipe Fidget leaves unused.
function Start-FidgetPiped([string]$Bin, [string[]]$Arguments, [string]$StdoutLog, [string]$StderrLog, [string]$WorkingDirectory = "") {
    $start = New-Object System.Diagnostics.ProcessStartInfo $Bin
    if ($WorkingDirectory) { $start.WorkingDirectory = $WorkingDirectory }
    $start.Arguments = (@($Arguments | Where-Object { $null -ne $_ } | ForEach-Object { '"' + ($_ -replace '"', '\"') + '"' }) -join ' ')
    $start.UseShellExecute = $false
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = New-Object System.Text.UTF8Encoding $false
    $start.StandardErrorEncoding = New-Object System.Text.UTF8Encoding $false
    $process = [System.Diagnostics.Process]::Start($start)
    [FidgetSignal]::Tee($process, $false, $StdoutLog)
    [FidgetSignal]::Tee($process, $true, $StderrLog)
    return $process
}

function Get-NowMs {
    return [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
}

# Point every folder Fidget writes under $Out (FIDGET_DATA_DIR too, as a private
# HOME does not move dirs::data_dir) and mark the first-run tour shown.
# Returns the data folder, which holds process.log.
function Initialize-ScenarioData([string]$Out, [hashtable]$Settings = @{}) {
    $data = Join-Path $Out "data"
    $homeDir = Join-Path $Out "home"
    foreach ($d in @($data, "$homeDir\AppData\Roaming", "$homeDir\AppData\Local", "$Out\tmp", "$Out\webview2")) {
        New-Item -ItemType Directory -Force -Path $d | Out-Null
    }
    $seed = @{ first_run_tour_shown = $true }
    foreach ($k in $Settings.Keys) { $seed[$k] = $Settings[$k] }
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText((Join-Path $data "settings.json"), ($seed | ConvertTo-Json -Compress), $utf8)
    $env:FIDGET_DATA_DIR = $data
    $env:HOME = $homeDir
    $env:USERPROFILE = $homeDir
    $env:APPDATA = "$homeDir\AppData\Roaming"
    $env:LOCALAPPDATA = "$homeDir\AppData\Local"
    $env:TEMP = "$Out\tmp"
    $env:TMP = "$Out\tmp"
    $env:WEBVIEW2_USER_DATA_FOLDER = "$Out\webview2"
    $script:WatchDir = $Out
    return $data
}

function Wait-OnChanges([double]$Seconds, [string]$Dir, [scriptblock]$Test) {
    if (-not $Dir) { $Dir = $script:WatchDir }
    if (-not $Dir -or -not (Test-Path -LiteralPath $Dir)) { $Dir = [System.IO.Path]::GetTempPath() }
    $watcher = [FidgetSignal]::Watch($Dir)
    $deadline = (Get-Date).AddSeconds($Seconds)
    try {
        while ($true) {
            # Noted before the check, so a wake during it is not lost.
            $seen = [FidgetSignal]::Seen
            if (& $Test) { return $true }
            if ($script:proc -and $script:proc.HasExited) { return $false }
            $left = [int]($deadline - (Get-Date)).TotalMilliseconds
            if ($left -le 0 -or -not [FidgetSignal]::WaitPast($seen, $left)) { return [bool](& $Test) }
        }
    } finally {
        $watcher.Dispose()
    }
}

function Test-FileMatches([string]$FilePath, [string]$Pattern) {
    (Test-Path -LiteralPath $FilePath) -and [bool](Select-String -LiteralPath $FilePath -Pattern $Pattern -Quiet)
}

function Wait-ForLine([double]$Seconds, [string]$FilePath, [string]$Pattern) {
    return (Wait-OnChanges $Seconds (Split-Path -Parent $FilePath) { Test-FileMatches $FilePath $Pattern })
}

function Wait-ForTrace([double]$Seconds, [string]$LogFile, [string]$Pattern) {
    return (Wait-ForLine $Seconds $LogFile $Pattern)
}

function Wait-ForMarks([double]$Seconds, [string]$MarksFile, [string]$Pattern) {
    return (Wait-ForLine $Seconds $MarksFile $Pattern)
}

function Wait-ForFile([double]$Seconds, [string]$FilePath) {
    $dir = Split-Path -Parent $FilePath
    while ($dir -and -not (Test-Path -LiteralPath $dir)) { $dir = Split-Path -Parent $dir }
    return (Wait-OnChanges $Seconds $dir { Test-Path -LiteralPath $FilePath })
}

# Waits for one more matching line than $Before, for a line that repeats (a
# second spawn, a second Poke); take the count before the trigger.
function Wait-ForCount([double]$Seconds, [string]$FilePath, [string]$Pattern, [int]$Before) {
    return (Wait-OnChanges $Seconds (Split-Path -Parent $FilePath) {
            (Test-Path -LiteralPath $FilePath) -and
            @(Get-Content -LiteralPath $FilePath | Where-Object { $_ -match $Pattern }).Count -gt $Before
        })
}

# Wait until a probe returns true, for state with no file of its own (a UI
# Automation dump, a window frame); re-run after each change under WatchDir.
function Wait-Until([double]$Seconds, [scriptblock]$Probe) {
    return (Wait-OnChanges $Seconds $script:WatchDir $Probe)
}

# Run ax-window-win.ps1 in this session (no new powershell per check), splatting
# $AxArgs as its parameters, e.g. @{ Command = "dump"; ProcessId = $id }.
$script:AxScript = Join-Path $PSScriptRoot "..\ax-window-win.ps1"
function Invoke-AxWin([string]$OutFile, [hashtable]$AxArgs) {
    $global:LASTEXITCODE = 0
    try {
        & $script:AxScript @AxArgs 2>&1 | Out-File -FilePath $OutFile -Encoding utf8
    } catch {
        $_ | Out-File -FilePath $OutFile -Encoding utf8 -Append
        return $false
    }
    return ($LASTEXITCODE -eq 0)
}

function Wait-ForUia([double]$Seconds, [int]$ProcessId, [scriptblock]$Check) {
    return [bool](Wait-UiaChange $ProcessId $Seconds $Check)
}

function Wait-ForAxDump([double]$Seconds, [int]$ProcessId, [string]$Title, [string]$Pattern) {
    $ax = Join-Path $PSScriptRoot "..\ax-window-win.ps1"
    return [bool](Wait-UiaChange $ProcessId $Seconds {
            try {
                $lines = & $ax dump -ProcessId $ProcessId -Title $Title 2>$null
            } catch {
                return $false
            }
            [bool]($lines | Select-String -Pattern $Pattern -Quiet)
        })
}

# The sprite's position when it is at rest, else $null: the newest five frame
# lines are all Grounded or Perched, not walking or falling, in one spot (within 2 px).
# Frames land every draw, so this needs no second look after a pause.
function Get-SpriteAtRest([string]$TraceFile) {
    if (-not (Test-Path -LiteralPath $TraceFile)) { return $null }
    $frames = @(Get-Content -LiteralPath $TraceFile | Where-Object { $_ -match '^\d+\.?\d* frame: ' } | Select-Object -Last 5)
    if ($frames.Count -lt 5) { return $null }
    $at = $null
    foreach ($line in $frames) {
        if ($line -match 'walk#|Falling' -or $line -notmatch ' (?:Grounded|Perched) .* sprite\((-?\d+),(-?\d+)\) ') { return $null }
        $here = @{ X = [int]$Matches[1]; Y = [int]$Matches[2] }
        # Within 2 px of the first frame, so a slow drift is not rest.
        if ($at -and ([Math]::Abs($here.X - $at.X) -gt 2 -or [Math]::Abs($here.Y - $at.Y) -gt 2)) { return $null }
        if (-not $at) { $at = $here }
    }
    return $at
}

function In-State([string]$LogFile, [string]$StatePattern) {
    if (-not (Test-Path -LiteralPath $LogFile)) { return $false }
    $frame = Get-Content -LiteralPath $LogFile | Where-Object { $_ -match '^frame: ' } | Select-Object -Last 1
    if (-not $frame) { return $false }
    return $frame -match " ($StatePattern) "
}

# Cleanup for a finally block: kill Fidget and every process under it, then
# any fixture Harness whose command line holds $Pattern (bash can outlive the
# tree when its parent is already gone).
function Stop-FidgetTree($Process, [string]$Pattern) {
    if ($Process -and -not $Process.HasExited) {
        & taskkill.exe /PID $Process.Id /T /F 2>&1 | Out-Null
        if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue }
    }
    if ($Pattern) {
        Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Where-Object { $_.CommandLine -and $_.CommandLine -like "*$Pattern*" } |
            ForEach-Object { & taskkill.exe /PID $_.ProcessId /T /F 2>&1 | Out-Null }
    }
}
