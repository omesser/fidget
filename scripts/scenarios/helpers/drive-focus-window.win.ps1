# Throwaway #1920 probe: a plain form at (100,100) 500x350 that appends each
# focus, click and key event it gets to <log>, one per line.
# Usage: powershell -STA -File drive-focus-window.win.ps1 <log>
$Log = $args[0]
Add-Type -AssemblyName System.Windows.Forms
function Say([string]$Line) { [System.IO.File]::AppendAllText($Log, "$Line`r`n") }
$form = New-Object System.Windows.Forms.Form
$form.Text = "drive-focus-window"
$form.StartPosition = "Manual"
$form.Location = New-Object System.Drawing.Point(100, 100)
$form.Size = New-Object System.Drawing.Size(500, 350)
$form.KeyPreview = $true
$form.Add_Activated({ Say "window: activated" })
$form.Add_Deactivate({ Say "window: deactivate" })
$form.Add_MouseDown({ Say "window: mousedown" })
$form.Add_KeyDown({ param($s, $e) Say "window: keydown $($e.KeyCode)" })
$form.Add_Shown({ $form.Activate(); Say "window: up" })
[System.Windows.Forms.Application]::Run($form)
