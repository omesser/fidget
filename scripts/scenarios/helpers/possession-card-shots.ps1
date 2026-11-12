# Pure helpers for the Windows possession-card screenshots leaf.
# Parse a card trace line, size the crop from the card plus the charge bar,
# scale that crop from logical pixels into physical pixels, and judge whether
# every margin pixel is the backdrop grey.

# Same values as CHARGE_BAR_PX and POSSESSION_GAP_PX in src/main.js.
$script:PossessionBarPx = 8
$script:PossessionGapPx = 8
$script:PossessionShotMarginPx = 16

function Get-BackdropGrey {
    return @{ R = 160; G = 160; B = 160 }
}

function Get-PossessionCardLine([string]$Line) {
    if ($Line -notmatch 'overlay ([^:]+): possession card at \((-?\d+),(-?\d+)\) (\d+)x(\d+) paused=(true|false)') {
        return $null
    }
    return @{
        Label  = $Matches[1]
        X      = [int]$Matches[2]
        Y      = [int]$Matches[3]
        Width  = [int]$Matches[4]
        Height = [int]$Matches[5]
        Paused = ($Matches[6] -ceq "true")
    }
}

function Get-LatestPossessionCard([string[]]$Lines) {
    $found = $null
    foreach ($line in $Lines) {
        $card = Get-PossessionCardLine $line
        if ($card) { $found = $card }
    }
    return $found
}

function Get-OverlayOrigin([string[]]$Lines, [string]$Label) {
    $found = $null
    $pattern = "overlay: $([regex]::Escape($Label)) covers \d+x\d+ at \((-?\d+),(-?\d+)\)"
    foreach ($line in $Lines) {
        if ($line -match $pattern) {
            $found = @{ X = [int]$Matches[1]; Y = [int]$Matches[2] }
        }
    }
    return $found
}

# Effective scale for one monitor: Windows effective DPI divided by 96.
# 96 dpi is 100%; 144 dpi is 150%.
function Get-DpiScale([int]$Dpi) {
    if ($Dpi -le 0) { return 1.0 }
    return [double]$Dpi / 96.0
}

# Screen-space crop in physical pixels: the card, the charge bar under it
# (hidden on a paused card, so left out), and a margin around both. Card and
# origin are logical; $Scale is that display's effective DPI / 96.
function Get-CardShotBox($Card, $Origin, [int]$BarPx, [int]$GapPx, [int]$MarginPx, [double]$Scale = 1) {
    if ($Scale -le 0) { $Scale = 1 }
    if ($Card.Paused) { $BarPx = 0; $GapPx = 0 }
    $cardX = [int][Math]::Round(($Origin.X + $Card.X) * $Scale)
    $cardY = [int][Math]::Round(($Origin.Y + $Card.Y) * $Scale)
    $cardW = [int][Math]::Round($Card.Width * $Scale)
    $cardH = [int][Math]::Round($Card.Height * $Scale)
    $barH = [int][Math]::Round($BarPx * $Scale)
    $gap = [int][Math]::Round($GapPx * $Scale)
    $margin = [int][Math]::Round($MarginPx * $Scale)
    $barY = $cardY + $cardH + $gap
    $left = $cardX - $margin
    $top = $cardY - $margin
    $right = $cardX + $cardW + $margin
    $bottom = $barY + $barH + $margin
    return @{
        X          = $left
        Y          = $top
        Width      = $right - $left
        Height     = $bottom - $top
        CardX      = $cardX - $left
        CardY      = $cardY - $top
        CardWidth  = $cardW
        CardHeight = $cardH
        BarX       = $cardX - $left
        BarY       = $barY - $top
        BarWidth   = $cardW
        BarHeight  = $barH
    }
}

# A pixel as the screen grab returns it: 0xRRGGBB, alpha masked off.
function Get-PackedRgb([int]$R, [int]$G, [int]$B) {
    return ($R -shl 16) -bor ($G -shl 8) -bor $B
}

function Get-GreyPx($Grey) { return Get-PackedRgb $Grey.R $Grey.G $Grey.B }

# $Pixels is a packed row-major grab $Width wide. $Rects (the card first, then
# the bar) may be any color; everything else must be the backdrop grey. A card
# that is all grey was not captured, so that shot is refused too.
function Get-BackdropVerdict([int[]]$Pixels, [int]$Width, $Rects, $Grey) {
    if ($Width -lt 1 -or $Pixels.Length -lt $Width) {
        return @{ Ok = $false; Message = "the shot is empty, so the margin cannot be the grey backdrop" }
    }
    $greyPx = Get-GreyPx $Grey
    $height = [int][Math]::Floor($Pixels.Length / $Width)
    $cardDrawn = $false
    for ($y = 0; $y -lt $height; $y++) {
        for ($x = 0; $x -lt $Width; $x++) {
            $px = $Pixels[$y * $Width + $x] -band 0xFFFFFF
            $inside = $false
            foreach ($rect in $Rects) {
                if ($x -ge $rect.X -and $x -lt ($rect.X + $rect.Width) -and $y -ge $rect.Y -and $y -lt ($rect.Y + $rect.Height)) { $inside = $true; break }
            }
            if ($inside) {
                if ($px -ne $greyPx) { $cardDrawn = $true }
                continue
            }
            if ($px -ne $greyPx) {
                $r = ($px -shr 16) -band 0xFF; $g = ($px -shr 8) -band 0xFF; $b = $px -band 0xFF
                return @{
                    Ok      = $false
                    Message = "margin pixel at ($x,$y) is rgb($r,$g,$b), not the grey backdrop"
                }
            }
        }
    }
    if (-not $cardDrawn) {
        return @{ Ok = $false; Message = "the card is all backdrop grey, so the overlay was not captured" }
    }
    return @{ Ok = $true; Message = "every margin pixel is the grey backdrop" }
}

# The bar's middle row shows fill and track, neither one alone and no backdrop.
# Its rounded ends (4 px a side) are not read.
function Test-BarPartFull([int[]]$Row, $Grey) {
    $greyPx = Get-GreyPx $Grey
    $counts = @{}
    for ($n = 4; $n -lt $Row.Length - 4; $n++) {
        $px = $Row[$n] -band 0xFFFFFF
        if ($px -eq $greyPx) { return $false }
        $counts[$px] = 1 + [int]$counts[$px]
    }
    return @($counts.Values | Where-Object { $_ -ge 3 }).Count -ge 2
}

# The backdrop is painted and the overlay drawn over it: the crop's top margin
# row is all grey and the card's middle row is not.
function Test-ShotSettled([int[]]$MarginRow, [int[]]$CardRow, $Grey) {
    $greyPx = Get-GreyPx $Grey
    foreach ($px in $MarginRow) { if (($px -band 0xFFFFFF) -ne $greyPx) { return $false } }
    foreach ($px in $CardRow) { if (($px -band 0xFFFFFF) -ne $greyPx) { return $true } }
    return $false
}
