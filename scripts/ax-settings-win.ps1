#!/usr/bin/env pwsh
param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('wait', 'dump', 'pick-source', 'expand-disclosure')]
    [string]$Command,
    [int]$Timeout = 30,
    [string]$Title,
    [string]$DisclosureLabel,
    [Parameter(Position=1)]
    [int]$ProcessId = 0
)
$ErrorActionPreference = "Continue"
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

function Get-AutomationElementFromHandle {
    # FromHandle, not RootElement.FindFirst(Descendants): a desktop-wide search
    # can hang indefinitely on a multi-monitor session.
    param([IntPtr]$WindowHandle)
    if ($WindowHandle -eq [IntPtr]::Zero) { return $null }
    try {
        return [System.Windows.Automation.AutomationElement]::FromHandle($WindowHandle)
    } catch {
        return $null
    }
}

function Find-SettingsWindow {
    # ProcessId, not Pid: $PID is Constant+AllScope and cannot be a parameter.
    param([int]$ProcessId = 0, [IntPtr]$WindowHandle = 0)

    if ($WindowHandle -ne [IntPtr]::Zero) {
        $fromHandle = Get-AutomationElementFromHandle -WindowHandle $WindowHandle
        if ($null -ne $fromHandle) { return $fromHandle }
    }

    $classCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ClassNameProperty,
        "Tauri Window"
    )
    $nameCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty,
        "Settings"
    )
    $typeCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Window
    )
    # Class + name + window: Name=Settings alone matches the Windows Settings app.
    if ($ProcessId -gt 0) {
        $pidCond = New-Object System.Windows.Automation.PropertyCondition(
            [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
            $ProcessId
        )
        $webviewCond = New-Object System.Windows.Automation.AndCondition($classCond, $nameCond, $typeCond, $pidCond)
    } else {
        $webviewCond = New-Object System.Windows.Automation.AndCondition($classCond, $nameCond, $typeCond)
    }
    return [System.Windows.Automation.AutomationElement]::RootElement.FindFirst(
        [System.Windows.Automation.TreeScope]::Descendants,
        $webviewCond
    )
}

function Wait-ForSettings {
    param([int]$TimeoutSec, [int]$ProcessId = 0)
    $waited = 0
    while ($waited -lt $TimeoutSec) {
        if ($null -ne (Find-SettingsWindow -ProcessId $ProcessId)) { return 0 }
        Start-Sleep -Seconds 1
        $waited++
    }
    return 1
}

function Get-ElementState {
    param($element)
    $states = @()
    try {
        if ($element.Current.IsEnabled) { $states += "enabled" } else { $states += "disabled" }
    } catch { $states += "unknown" }
    try {
        $vp = $element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        if ($vp -and $vp.Current.IsReadOnly) { $states += "readonly" }
    } catch {}
    return ($states -join ",")
}

function Dump-Tree {
    param($element, [int]$Depth = 0)
    if ($null -eq $element) { return }
    $indent = "  " * $Depth
    Write-Output ("{0}[{1}] '{2}' ({3})" -f $indent, $element.Current.ControlType.ProgrammaticName, $element.Current.Name, (Get-ElementState $element))
    $walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
    $child = $walker.GetFirstChild($element)
    while ($null -ne $child) {
        Dump-Tree -element $child -Depth ($Depth + 1)
        $child = $walker.GetNextSibling($child)
    }
}

function Invoke-Dump {
    param([int]$ProcessId = 0)
    $window = Find-SettingsWindow -ProcessId $ProcessId
    if ($null -eq $window) { Write-Host "ERROR: Settings window not found"; exit 1 }
    Dump-Tree -element $window
    exit 0
}

function Invoke-PickSource {
    param([string]$SourceTitle, [int]$ProcessId = 0)
    $window = Find-SettingsWindow -ProcessId $ProcessId
    if ($null -eq $window) { Write-Host "ERROR: Settings window not found"; exit 1 }

    $comboCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::ComboBox
    )
    $combos = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, $comboCond)
    $combo = $null
    foreach ($cbox in $combos) {
        if ($cbox.Current.Name -eq 'AI source') { $combo = $cbox; break }
    }
    if ($null -eq $combo -and $combos.Count -gt 0) { $combo = $combos.Item(0) }
    if ($null -eq $combo) { Write-Host "ERROR: AI source ComboBox not found"; exit 1 }

    try {
        $expand = $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        if ($expand) { $expand.Expand(); Start-Sleep -Milliseconds 400 }
    } catch {}

    $itemType = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::ListItem)
    $items = $combo.FindAll([System.Windows.Automation.TreeScope]::Descendants, $itemType)
    $item = $null
    foreach ($li in $items) {
        if ($li.Current.Name -eq $SourceTitle) { $item = $li; break }
    }
    if ($null -eq $item -and $SourceTitle -like 'Harness*') {
        $token = ($SourceTitle -replace '^Harness\s+', '').Trim()
        foreach ($li in $items) {
            $n = $li.Current.Name
            if ($n -like 'Harness*' -and $n.EndsWith($token)) { $item = $li; break }
            if ($n -like 'Harness*' -and $n -like ("*" + $token)) { $item = $li; break }
        }
    }
    if ($null -eq $item -and $SourceTitle -eq 'Model API') {
        foreach ($li in $items) {
            if ($li.Current.Name -eq 'Model API') { $item = $li; break }
        }
    }
    if ($null -eq $item) {
        $names = @(); foreach ($li in $items) { $names += $li.Current.Name }
        Write-Host ("ERROR: ListItem matching '{0}' not found (have: {1})" -f $SourceTitle, ($names -join ', '))
        exit 1
    }

    $sel = $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern)
    $sel.Select()
    try {
        $expand = $combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        if ($expand) { $expand.Collapse() }
    } catch {}
    Write-Output ("picked:{0}" -f $item.Current.Name)
    exit 0
}

function Invoke-ExpandDisclosure {
    param([string]$Label, [int]$ProcessId = 0)
    $window = Find-SettingsWindow -ProcessId $ProcessId
    if ($null -eq $window) { Write-Host "ERROR: Settings window not found"; exit 1 }
    $nameCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::NameProperty, $Label)
    $typeCond = New-Object System.Windows.Automation.PropertyCondition(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::Button)
    $andCond = New-Object System.Windows.Automation.AndCondition($nameCond, $typeCond)
    $button = $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $andCond)
    if ($null -eq $button) { Write-Host "ERROR: Disclosure button '$Label' not found"; exit 1 }
    $invoke = $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $invoke.Invoke()
    Write-Output "expanded:$Label"
    exit 0
}

switch ($Command) {
    'wait' {
        $code = Wait-ForSettings -TimeoutSec $Timeout -ProcessId $ProcessId
        if ($code -ne 0) { Write-Host "ERROR: Timed out waiting for Settings"; exit 1 }
        Write-Output "ready"; exit 0
    }
    'dump' { Invoke-Dump -ProcessId $ProcessId }
    'pick-source' {
        if (-not $Title) { Write-Host "ERROR: -Title required"; exit 1 }
        Invoke-PickSource -SourceTitle $Title -ProcessId $ProcessId
    }
    'expand-disclosure' {
        if (-not $DisclosureLabel) { Write-Host "ERROR: -DisclosureLabel required"; exit 1 }
        Invoke-ExpandDisclosure -Label $DisclosureLabel -ProcessId $ProcessId
    }
}
