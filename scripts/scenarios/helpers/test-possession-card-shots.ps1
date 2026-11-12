#!/usr/bin/env pwsh
# Self-check for possession-card-shots.ps1: parse a card trace line, size the
# crop from the card plus the charge bar, and refuse a shot whose margin is
# not the backdrop grey. Runs under pwsh and Windows PowerShell 5.1. No GUI.
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "possession-card-shots.ps1")

$failed = 0
function Check([bool]$Ok, [string]$What) {
    if ($Ok) { Write-Output "PASS: $What" } else { Write-Output "FAIL: $What"; $script:failed++ }
}

$line = "1790000000 overlay overlay-0: possession card at (16,40) 320x200 paused=false"
$card = Get-PossessionCardLine $line
Check ($null -ne $card) "a sized card line is parsed"
Check ($card.Label -ceq "overlay-0") "the overlay label is overlay-0"
Check ($card.X -eq 16 -and $card.Y -eq 40) "the card origin is (16,40)"
Check ($card.Width -eq 320 -and $card.Height -eq 200) "the card size is 320x200"
Check ($card.Paused -eq $false) "paused=false is not paused"

$paused = Get-PossessionCardLine "overlay overlay-1: possession card at (-8,12) 140x52 paused=true"
Check ($paused.Label -ceq "overlay-1") "a line without a seconds prefix still names the overlay"
Check ($paused.X -eq -8 -and $paused.Y -eq 12 -and $paused.Width -eq 140 -and $paused.Height -eq 52) "negative origins and the paused size are kept"
Check ($paused.Paused -eq $true) "paused=true is paused"

Check ($null -eq (Get-PossessionCardLine "1790000000 overlay overlay-0: possession card at (16,40) paused=false")) "a card line with no size is rejected"
Check ($null -eq (Get-PossessionCardLine "overlay overlay-0: pill follows instance=1")) "a non-card trace line is rejected"

$live = Get-PossessionCardLine "100 overlay overlay-0: possession card at (16,40) 320x200 paused=false dnd=off"
Check ($live.Width -eq 320 -and $live.Paused -eq $false) "a live trace line with the dnd suffix still parses"


$lines = @(
    "100 overlay: overlay-0 covers 1920x1080 at (0,0)",
    "101 overlay: overlay-1 covers 1440x900 at (1920,-80)",
    "102 overlay overlay-1: possession card at (16,16) 200x80 paused=false"
)
$latest = Get-LatestPossessionCard $lines
Check ($latest.Label -ceq "overlay-1" -and $latest.X -eq 16) "the newest sized card line is the one used"
$origin = Get-OverlayOrigin $lines $latest.Label
Check ($origin.X -eq 1920 -and $origin.Y -eq -80) "the overlay origin comes from that overlay's covers line"

# Card (16,16) 100x50 in overlay space, origin (0,0), gap 8, bar 8, margin 16.
# Screen card (16,16)-(116,66), bar (16,74)-(116,82), inflated crop (0,0)-(132,98).
$box = Get-CardShotBox @{ X = 16; Y = 16; Width = 100; Height = 50 } @{ X = 0; Y = 0 } 8 8 16
Check ($box.X -eq 0 -and $box.Y -eq 0 -and $box.Width -eq 132 -and $box.Height -eq 98) "the crop is the card, the bar, and a 16 px margin (0,0) 132x98"
Check ($box.CardX -eq 16 -and $box.CardY -eq 16 -and $box.CardWidth -eq 100 -and $box.CardHeight -eq 50) "the card sits at (16,16) 100x50 inside the crop"
Check ($box.BarX -eq 16 -and $box.BarY -eq 74 -and $box.BarWidth -eq 100 -and $box.BarHeight -eq 8) "the bar sits 8 px below the card at (16,74) 100x8"

$shifted = Get-CardShotBox @{ X = 16; Y = 16; Width = 100; Height = 50 } @{ X = 1920; Y = -80 } 8 8 16
Check ($shifted.X -eq 1920 -and $shifted.Y -eq -80) "the crop follows the overlay origin"

$grey = Get-BackdropGrey
Check ($grey.R -eq 160 -and $grey.G -eq 160 -and $grey.B -eq 160) "the backdrop grey is rgb(160,160,160)"

# 8x6 shot. Card (2,2) 4x2, bar (2,4) 4x1. Every other pixel is grey, then one leak.
function New-Shot([int]$W, [int]$H, $Grey) {
    $rows = @()
    for ($y = 0; $y -lt $H; $y++) {
        $row = @()
        for ($x = 0; $x -lt $W; $x++) { $row += ,(@($Grey.R, $Grey.G, $Grey.B)) }
        $rows += ,$row
    }
    return $rows
}
$shot = New-Shot 8 6 $grey
# Paint the card and bar with a non-grey color the verdict must ignore.
for ($y = 2; $y -le 3; $y++) { for ($x = 2; $x -le 5; $x++) { $shot[$y][$x] = @(20, 30, 40) } }
for ($x = 2; $x -le 5; $x++) { $shot[4][$x] = @(200, 100, 0) }
$rects = @(
    @{ X = 2; Y = 2; Width = 4; Height = 2 },
    @{ X = 2; Y = 4; Width = 4; Height = 1 }
)
$ok = Get-BackdropVerdict $shot $rects $grey
Check ($ok.Ok -eq $true -and $ok.Message -match 'every margin pixel is the grey backdrop') "a shot whose margin is all grey is accepted"

$shot[0][7] = @(255, 0, 0)
$bad = Get-BackdropVerdict $shot $rects $grey
Check ($bad.Ok -eq $false) "a red margin pixel is refused"
Check ($bad.Message -match 'margin pixel at \(7,0\) is rgb\(255,0,0\), not the grey backdrop') "the verdict names the first leaking pixel"

if ($failed -eq 0) { Write-Output "all checks passed"; exit 0 }
Write-Output "$failed checks failed"
exit 1
