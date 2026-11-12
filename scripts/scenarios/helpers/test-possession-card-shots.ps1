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

Check ((Get-DpiScale 96) -eq 1) "96 dpi is a 1.0 scale"
Check ((Get-DpiScale 144) -eq 1.5) "144 dpi is a 1.5 scale"

# Same card at 150%: every logical length is multiplied by 1.5 (16->24, 100->150, 50->75, 8->12).
$hi = Get-CardShotBox @{ X = 16; Y = 16; Width = 100; Height = 50 } @{ X = 0; Y = 0 } 8 8 16 1.5
Check ($hi.X -eq 0 -and $hi.Y -eq 0 -and $hi.Width -eq 198 -and $hi.Height -eq 147) "at 150% the crop is (0,0) 198x147"
Check ($hi.CardX -eq 24 -and $hi.CardY -eq 24 -and $hi.CardWidth -eq 150 -and $hi.CardHeight -eq 75) "at 150% the card sits at (24,24) 150x75 inside the crop"
Check ($hi.BarX -eq 24 -and $hi.BarY -eq 111 -and $hi.BarWidth -eq 150 -and $hi.BarHeight -eq 12) "at 150% the bar sits at (24,111) 150x12"
$hiShift = Get-CardShotBox @{ X = 16; Y = 16; Width = 100; Height = 50 } @{ X = 1920; Y = -80 } 8 8 16 1.5
Check ($hiShift.X -eq 2880 -and $hiShift.Y -eq -120) "at 150% the crop origin scales to (2880,-120)"

$grey = Get-BackdropGrey
Check ($grey.R -eq 160 -and $grey.G -eq 160 -and $grey.B -eq 160) "the backdrop grey is rgb(160,160,160)"

# Paused, the bar is hidden, so the crop is the card and the margin alone:
# (0,0)-(132,82), with no bar rect left in it.
$pausedBox = Get-CardShotBox @{ X = 16; Y = 16; Width = 100; Height = 50; Paused = $true } @{ X = 0; Y = 0 } 8 8 16
Check ($pausedBox.Width -eq 132 -and $pausedBox.Height -eq 82) "a paused crop is the card and a 16 px margin, 132x82"
Check ($pausedBox.BarHeight -eq 0) "a paused crop holds no bar"

$greyPx = Get-PackedRgb 160 160 160
Check ($greyPx -eq 0xA0A0A0) "rgb(160,160,160) packs to 0xA0A0A0"
Check ((Get-PackedRgb 255 0 0) -eq 0xFF0000) "rgb(255,0,0) packs to 0xFF0000"

# 8x6 shot, packed row-major. Card (2,2) 4x2, bar (2,4) 4x1. Every other pixel
# is grey, then one leak.
$W = 8; $H = 6
$shot = [int[]]::new($W * $H)
for ($n = 0; $n -lt $shot.Length; $n++) { $shot[$n] = $greyPx }
for ($y = 2; $y -le 3; $y++) { for ($x = 2; $x -le 5; $x++) { $shot[$y * $W + $x] = Get-PackedRgb 20 30 40 } }
for ($x = 2; $x -le 5; $x++) { $shot[4 * $W + $x] = Get-PackedRgb 200 100 0 }
$rects = @(
    @{ X = 2; Y = 2; Width = 4; Height = 2 },
    @{ X = 2; Y = 4; Width = 4; Height = 1 }
)
$ok = Get-BackdropVerdict $shot $W $rects $grey
Check ($ok.Ok -eq $true -and $ok.Message -match 'every margin pixel is the grey backdrop') "a shot whose margin is all grey is accepted"

# The alpha byte is not color: an opaque 0xFFA0A0A0 is still the grey.
$opaque = [int[]]$shot.Clone()
$opaque[0] = -16777216 -bor $greyPx  # 0xFF000000 as a signed int
Check ((Get-BackdropVerdict $opaque $W $rects $grey).Ok) "the alpha byte is ignored"

$shot[0 * $W + 7] = Get-PackedRgb 255 0 0
$bad = Get-BackdropVerdict $shot $W $rects $grey
Check ($bad.Ok -eq $false) "a red margin pixel is refused"
Check ($bad.Message -match 'margin pixel at \(7,0\) is rgb\(255,0,0\), not the grey backdrop') "the verdict names the first leaking pixel"

# A card that was not captured (the overlay missing from the grab) is all grey.
$blank = [int[]]::new($W * $H)
for ($n = 0; $n -lt $blank.Length; $n++) { $blank[$n] = $greyPx }
$missing = Get-BackdropVerdict $blank $W $rects $grey
Check ($missing.Ok -eq $false -and $missing.Message -match 'card is all backdrop grey') "a shot with no card in it is refused"

# Bar middle row: track only, fill only, part-full, hidden.
$track = Get-PackedRgb 90 90 90
$fill = Get-PackedRgb 0 120 255
function New-Row([int]$Filled, [int]$Width) { $r = [int[]]::new($Width); for ($n = 0; $n -lt $Width; $n++) { $r[$n] = $(if ($n -lt $Filled) { $fill } else { $track }) }; return ,$r }
Check (-not (Test-BarPartFull (New-Row 0 40) $grey)) "an empty bar is not part-full"
Check (-not (Test-BarPartFull (New-Row 40 40) $grey)) "a full bar is not part-full"
Check (Test-BarPartFull (New-Row 15 40) $grey) "a bar with fill and track is part-full"
$hidden = [int[]]::new(40); for ($n = 0; $n -lt 40; $n++) { $hidden[$n] = $greyPx }
Check (-not (Test-BarPartFull $hidden $grey)) "a hidden bar over the backdrop is not part-full"
$edge = New-Row 15 40; $edge[0] = $greyPx; $edge[39] = $greyPx
Check (Test-BarPartFull $edge $grey) "the bar's rounded ends are not read"

# Settled: the crop's top row is all grey and the card's middle row is not.
$cardRow = [int[]]@($greyPx, (Get-PackedRgb 20 30 40), $greyPx)
Check (Test-ShotSettled $hidden $cardRow $grey) "a grey margin row and a drawn card row are settled"
Check (-not (Test-ShotSettled $edge $cardRow $grey)) "a margin row with the bar in it is not settled"
Check (-not (Test-ShotSettled $hidden $hidden $grey)) "a card row that is all grey is not settled"

if ($failed -eq 0) { Write-Output "all checks passed"; exit 0 }
Write-Output "$failed checks failed"
exit 1
