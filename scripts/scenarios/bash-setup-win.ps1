# Shared helper for Windows leaves: bash.exe from Git Bash or PATH, as a short
# path so the harness line holds no spaces. Sets $bashPath, or $null if none.

function Get-ShortPath([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        return $null
    }
    $fso = New-Object -ComObject Scripting.FileSystemObject
    try {
        $file = $fso.GetFile($Path)
        return $file.ShortPath
    } catch {
        return $null
    } finally {
        [System.Runtime.Interopservices.Marshal]::ReleaseComObject($fso) | Out-Null
    }
}

$bashPath = $null

$gitBashPaths = @(
    "${env:ProgramFiles}\Git\bin\bash.exe",
    "${env:ProgramFiles(x86)}\Git\bin\bash.exe",
    "C:\Program Files\Git\bin\bash.exe",
    "C:\Program Files (x86)\Git\bin\bash.exe"
)

foreach ($candidate in $gitBashPaths) {
    if (Test-Path -LiteralPath $candidate) {
        $bashPath = Get-ShortPath $candidate
        if ($bashPath) {
            break
        }
    }
}

if (-not $bashPath) {
    $pathBash = Get-Command bash -ErrorAction SilentlyContinue
    if ($pathBash) {
        $bashPath = Get-ShortPath $pathBash.Source
    }
}
