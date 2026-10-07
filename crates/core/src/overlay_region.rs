//! Which pixels an overlay draws and takes input on. Windows' `SetWindowRgn`
//! clips both, so the region must hold the sprite's swept art, the clickable
//! hotspots, and the painted rects (bubble body, thinking dots).

/// Rectangle in overlay coordinates: `[left, top, right, bottom]`.
pub type RegionRect = [i32; 4];

/// `trail` is already `[left, top, right, bottom]`; `hotspots` and `painted`
/// are `[x, y, width, height]`. Painted rects draw but are not clickable.
pub fn overlay_region_rects(
    trail: &[[i32; 4]],
    hotspots: &[[i32; 4]],
    painted: &[[i32; 4]],
) -> Vec<RegionRect> {
    let hotspot_bounds = hotspots
        .iter()
        .map(|&[x, y, width, height]| [x, y, x + width, y + height]);

    let painted_bounds = painted
        .iter()
        .map(|&[x, y, width, height]| [x, y, x + width, y + height]);

    trail
        .iter()
        .copied()
        .chain(hotspot_bounds)
        .chain(painted_bounds)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (name, art LTRB, hotspots XYWH, painted XYWH, expected region LTRB)
    type Row<'a> = (
        &'a str,
        &'a [[i32; 4]],
        &'a [[i32; 4]],
        &'a [[i32; 4]],
        &'a [[i32; 4]],
    );

    #[test]
    fn region_composition_table() {
        let art = [[100, 200, 180, 328]]; // sprite swept bounds
        let more_hotspot = [220, 140, 72, 18]; // "Open chat"
        let bubble = [120, 80, 200, 90];
        let thinking = [150, 100, 48, 24];
        let qm_pill = [130, 60, 160, 36];

        let rows: &[Row] = &[
            ("nothing to draw", &[], &[], &[], &[]),
            (
                "bubble fully on owner",
                &art,
                &[],
                &[bubble],
                &[[100, 200, 180, 328], [120, 80, 320, 170]],
            ),
            (
                "thinking only",
                &art,
                &[],
                &[thinking],
                &[[100, 200, 180, 328], [150, 100, 198, 124]],
            ),
            (
                "QM pill only (as hotspot)",
                &art,
                &[qm_pill],
                &[],
                &[[100, 200, 180, 328], [130, 60, 290, 96]],
            ),
            (
                "QM hotspot + bubble painted",
                &art,
                &[qm_pill],
                &[bubble],
                &[
                    [100, 200, 180, 328],
                    [130, 60, 290, 96],
                    [120, 80, 320, 170],
                ],
            ),
            (
                "truncated speech: hotspot + painted body",
                &art,
                &[more_hotspot],
                &[bubble],
                &[
                    [100, 200, 180, 328],
                    [220, 140, 292, 158],
                    [120, 80, 320, 170],
                ],
            ),
            (
                "bubble hidden (painted cleared)",
                &art,
                &[],
                &[],
                &[[100, 200, 180, 328]],
            ),
            (
                "bubble straddling seam onto non-sprite overlay",
                &[],
                &[],
                &[bubble],
                &[[120, 80, 320, 170]],
            ),
        ];

        for (name, trail, hotspots, painted, expect) in rows {
            let got = overlay_region_rects(trail, hotspots, painted);
            assert_eq!(got.len(), expect.len(), "{name}: rect count");
            for rect in *expect {
                assert!(got.contains(rect), "{name}: missing {rect:?} in {got:?}");
            }
        }
    }
}
