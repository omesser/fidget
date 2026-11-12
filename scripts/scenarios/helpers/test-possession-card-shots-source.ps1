#!/usr/bin/env pwsh
# Source checks for the Windows card-shots leaf and its helpers. Windows
# PowerShell 5.1 compiles Add-Type as C# 5 on .NET Framework, which pwsh does not.
$ErrorActionPreference = "Stop"

$failed = 0
function Check([bool]$Ok, [string]$What) {
    if ($Ok) { Write-Output "PASS: $What" } else { Write-Output "FAIL: $What"; $script:failed++ }
}

$files = @(
    (Join-Path $PSScriptRoot "..\manual-only\possession-card-shots.win.ps1"),
    (Join-Path $PSScriptRoot "possession-card-shots.ps1"),
    (Join-Path $PSScriptRoot "send-input.win.ps1")
)

# .NET Framework's CopyFromScreen rejects SourceCopy|CaptureBlt as an invalid
# CopyPixelOperation; .NET (pwsh) accepts it, so only this check catches it.
foreach ($file in $files) {
    $text = [System.IO.File]::ReadAllText($file)
    $name = Split-Path -Leaf $file
    Check (-not ($text -match 'CopyFromScreen[^;]*CaptureBlt')) "$name does not pass CaptureBlt to CopyFromScreen"
}

# A file whose Add-Type the pattern below misses would pass the C# 5 parse
# with nothing parsed, so each file's block count is pinned.
$addType = '(?ms)^Add-Type[^\n]*@"\r?\n(.*?)\r?\n"@'
$blocks = @{ "possession-card-shots.win.ps1" = 1; "possession-card-shots.ps1" = 0; "send-input.win.ps1" = 1 }
foreach ($file in $files) {
    $name = Split-Path -Leaf $file
    $count = [regex]::Matches([System.IO.File]::ReadAllText($file), $addType).Count
    Check ($count -eq $blocks[$name]) "$name holds $($blocks[$name]) Add-Type block(s) (found $count)"
}

# Each Add-Type here-string parses as C# 5. Needs Roslyn, which pwsh carries.
$roslyn = "Microsoft.CodeAnalysis.CSharp.CSharpSyntaxTree" -as [type]
if (-not $roslyn) {
    Write-Output "SKIP: no Roslyn in this PowerShell; the C# 5 parse runs under pwsh"
} else {
    $options = New-Object Microsoft.CodeAnalysis.CSharp.CSharpParseOptions ([Microsoft.CodeAnalysis.CSharp.LanguageVersion]::CSharp5)
    foreach ($file in $files) {
        $text = [System.IO.File]::ReadAllText($file)
        $name = Split-Path -Leaf $file
        $n = 0
        foreach ($m in [regex]::Matches($text, $addType)) {
            $n++
            $tree = [Microsoft.CodeAnalysis.CSharp.CSharpSyntaxTree]::ParseText($m.Groups[1].Value, $options)
            # Unbound: only syntax errors and C# 6+ features (CS8026 and kin) count.
            $comp = [Microsoft.CodeAnalysis.CSharp.CSharpCompilation]::Create("csharp5", [Microsoft.CodeAnalysis.SyntaxTree[]]@($tree))
            $errors = @($tree.GetDiagnostics()) + @($comp.GetDiagnostics() | Where-Object { $_.GetMessage() -match 'not available in C# 5' })
            $errors = @($errors | Where-Object { $_.Severity -eq [Microsoft.CodeAnalysis.DiagnosticSeverity]::Error })
            Check ($errors.Count -eq 0) "$name Add-Type block $n parses as C# 5$(if ($errors.Count) { ': ' + ($errors | Select-Object -First 3 | ForEach-Object { $_.ToString() }) -join '; ' })"
        }
    }
}

if ($failed -eq 0) { Write-Output "all checks passed"; exit 0 }
Write-Output "$failed checks failed"
exit 1
