//! Which pixels an overlay draws and takes input on. Windows' `SetWindowRgn`
//! clips both, so the region must hold the sprite's swept art and every rect
//! the renderer draws outside it, clickable or not.

use serde::Deserialize;

/// Rectangle in overlay coordinates: `[left, top, right, bottom]`.
pub type RegionRect = [i32; 4];

/// Something the renderer draws outside the art, in overlay coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct OverlayRect {
    /// `[x, y, width, height]`.
    pub rect: [i32; 4],
    /// "Open chat" and the quick pill take clicks. The bubble body and the
    /// thinking dots are only drawn, so clicks pass through them.
    pub clickable: bool,
}

/// What a Windows overlay's `SetWindowRgn` gets this frame.
#[derive(Debug, PartialEq, Eq)]
pub enum RegionPlan {
    /// Nothing to draw: no region, and the window passes every click.
    Clear,
    Apply(Vec<RegionRect>),
}

/// Clears only when there is nothing at all to keep: no sprite and no rect.
/// `trail` is already `[left, top, right, bottom]`.
pub fn region_plan(trail: &[[i32; 4]], rects: &[OverlayRect]) -> RegionPlan {
    let drawn = rects.iter().map(|OverlayRect { rect, .. }| {
        let [x, y, width, height] = *rect;
        [x, y, x + width, y + height]
    });
    let region: Vec<RegionRect> = trail.iter().copied().chain(drawn).collect();
    if region.is_empty() {
        RegionPlan::Clear
    } else {
        RegionPlan::Apply(region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (name, art LTRB, overlay rects, region LTRB; empty is `Clear`)
    type Row<'a> = (&'a str, &'a [[i32; 4]], &'a [OverlayRect], &'a [[i32; 4]]);

    const fn clickable(rect: [i32; 4]) -> OverlayRect {
        OverlayRect {
            rect,
            clickable: true,
        }
    }

    const fn drawn(rect: [i32; 4]) -> OverlayRect {
        OverlayRect {
            rect,
            clickable: false,
        }
    }

    /// The plan `apply_input_mask` carries out. An overlay without the sprite
    /// still keeps a bubble straddling the seam, or a control, in its region.
    #[test]
    fn region_plan_table() {
        let art = [[100, 200, 180, 328]]; // sprite swept bounds
        let open_chat = clickable([220, 140, 72, 18]);
        let bubble = drawn([120, 80, 200, 90]);
        let thinking = drawn([150, 100, 48, 24]);
        let qm_pill = clickable([130, 60, 160, 36]);

        let rows: &[Row] = &[
            ("nothing to draw clears", &[], &[], &[]),
            (
                "bubble fully on owner",
                &art,
                &[bubble],
                &[[100, 200, 180, 328], [120, 80, 320, 170]],
            ),
            (
                "thinking only",
                &art,
                &[thinking],
                &[[100, 200, 180, 328], [150, 100, 198, 124]],
            ),
            (
                "QM pill + bubble",
                &art,
                &[qm_pill, bubble],
                &[
                    [100, 200, 180, 328],
                    [130, 60, 290, 96],
                    [120, 80, 320, 170],
                ],
            ),
            (
                "truncated speech: Open chat + bubble",
                &art,
                &[open_chat, bubble],
                &[
                    [100, 200, 180, 328],
                    [220, 140, 292, 158],
                    [120, 80, 320, 170],
                ],
            ),
            ("bubble hidden", &art, &[], &[[100, 200, 180, 328]]),
            (
                "no sprite, bubble straddling the seam",
                &[],
                &[bubble],
                &[[120, 80, 320, 170]],
            ),
            (
                "no sprite, a control still on this display",
                &[],
                &[open_chat],
                &[[220, 140, 292, 158]],
            ),
        ];

        for (name, trail, rects, expect) in rows {
            let plan = region_plan(trail, rects);
            if expect.is_empty() {
                assert_eq!(plan, RegionPlan::Clear, "{name}");
                continue;
            }
            let RegionPlan::Apply(got) = plan else {
                panic!("{name}: cleared, want a region of {expect:?}");
            };
            assert_eq!(got.len(), expect.len(), "{name}: rect count");
            for rect in *expect {
                assert!(got.contains(rect), "{name}: missing {rect:?} in {got:?}");
            }
        }
    }
}
