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

impl OverlayRect {
    /// Whether a click at `(x, y)`, in overlay coordinates, lands on this.
    pub fn takes_click_at(&self, x: i32, y: i32) -> bool {
        let [left, top, width, height] = self.rect;
        self.clickable && x >= left && x < left + width && y >= top && y < top + height
    }
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
    let bounds = rects.iter().map(|OverlayRect { rect, .. }| {
        let [x, y, width, height] = *rect;
        [x, y, x + width, y + height]
    });
    let region: Vec<RegionRect> = trail.iter().copied().chain(bounds).collect();
    if region.is_empty() {
        RegionPlan::Clear
    } else {
        RegionPlan::Apply(region)
    }
}

/// The rects that take clicks, as `[x, y, width, height]`. X11's input shape
/// gets only these: it clips clicks and not drawing, so a drawn-only rect in
/// it would catch clicks meant for the window underneath.
pub fn clickable_rects(rects: &[OverlayRect]) -> Vec<[i32; 4]> {
    rects
        .iter()
        .filter(|rect| rect.clickable)
        .map(|rect| rect.rect)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (name, art LTRB, overlay rects, region LTRB; empty is `Clear`)
    type Row<'a> = (&'a str, &'a [[i32; 4]], &'a [OverlayRect], &'a [[i32; 4]]);

    /// (name, overlay rects, clickable XYWH)
    type ClickRow<'a> = (&'a str, &'a [OverlayRect], &'a [[i32; 4]]);

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

    /// Only a clickable rect stops the overlay passing a click, over its
    /// pixels and not one past them.
    #[test]
    fn only_a_clickable_rect_takes_a_click() {
        let open_chat = clickable([220, 140, 72, 18]);
        let qm_pill = clickable([130, 60, 160, 36]);
        let bubble = drawn([120, 80, 200, 90]);
        let thinking = drawn([150, 100, 48, 24]);

        let rows = [
            ("Open chat, top left", open_chat, (220, 140), true),
            ("Open chat, bottom right", open_chat, (291, 157), true),
            (
                "Open chat, past the far edges",
                open_chat,
                (292, 158),
                false,
            ),
            ("the quick pill", qm_pill, (200, 70), true),
            ("the bubble body", bubble, (200, 120), false),
            ("the thinking dots", thinking, (170, 110), false),
        ];
        for (name, rect, (x, y), takes) in rows {
            assert_eq!(rect.takes_click_at(x, y), takes, "{name}");
        }
    }

    /// What X11's input shape gets. Only the rects that take clicks go in;
    /// the bubble body and the thinking dots stay out so clicks pass them.
    #[test]
    fn clickable_rects_table() {
        let open_chat = clickable([220, 140, 72, 18]);
        let qm_pill = clickable([130, 60, 160, 36]);
        let bubble = drawn([120, 80, 200, 90]);
        let thinking = drawn([150, 100, 48, 24]);

        let rows: &[ClickRow] = &[
            ("nothing reported", &[], &[]),
            ("bubble only", &[bubble], &[]),
            ("thinking only", &[thinking], &[]),
            (
                "truncated speech: Open chat + bubble",
                &[bubble, open_chat],
                &[[220, 140, 72, 18]],
            ),
            (
                "QM pill + bubble",
                &[qm_pill, bubble],
                &[[130, 60, 160, 36]],
            ),
            ("QM pill alone", &[qm_pill], &[[130, 60, 160, 36]]),
            (
                "everything at once keeps its order",
                &[bubble, open_chat, thinking, qm_pill],
                &[[220, 140, 72, 18], [130, 60, 160, 36]],
            ),
        ];
        for (name, rects, expect) in rows {
            assert_eq!(clickable_rects(rects), *expect, "{name}");
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
