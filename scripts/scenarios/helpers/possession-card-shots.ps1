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

# Screen-space crop in physical pixels: the card, the charge bar under it,
# and a margin around both. Card and origin are logical (overlay-local and
# the overlay's covers point). $Scale is that display's effective DPI / 96.
function Get-CardShotBox($Card, $Origin, [int]$BarPx, [int]$GapPx, [int]$MarginPx, [double]$Scale = 1) {
    if ($Scale -le 0) { $Scale = 1 }
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

function Test-PointInRect([int]$X, [int]$Y, $Rect) {
    return ($X -ge $Rect.X -and $X -lt ($Rect.X + $Rect.Width) -and $Y -ge $Rect.Y -and $Y -lt ($Rect.Y + $Rect.Height))
}

# $Pixels is a row-major list of rows, each a list of @(R,G,B). $Rects are
# regions (the card and the bar) that may be any color. Everything else must
# be the backdrop grey or the shot is refused.
function Get-BackdropVerdict($Pixels, $Rects, $Grey) {
    $height = $Pixels.Count
    if ($height -lt 1) {
        return @{ Ok = $false; Message = "the shot is empty, so the margin cannot be the grey backdrop" }
    }
    $width = $Pixels[0].Count
    for ($y = 0; $y -lt $height; $y++) {
        $row = $Pixels[$y]
        for ($x = 0; $x -lt $width; $x++) {
            $inside = $false
            foreach ($rect in $Rects) {
                if (Test-PointInRect $x $y $rect) { $inside = $true; break }
            }
            if ($inside) { continue }
            $px = $row[$x]
            $r = [int]$px[0]; $g = [int]$px[1]; $b = [int]$px[2]
            if ($r -ne $Grey.R -or $g -ne $Grey.G -or $b -ne $Grey.B) {
                return @{
                    Ok      = $false
                    Message = "margin pixel at ($x,$y) is rgb($r,$g,$b), not the grey backdrop"
                }
            }
        }
    }
    return @{ Ok = $true; Message = "every margin pixel is the grey backdrop" }
}
