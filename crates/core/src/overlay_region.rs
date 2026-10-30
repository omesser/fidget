//! Which pixels an overlay draws and takes input on. Windows' `SetWindowRgn`
//! clips both, so the region must hold the sprite's swept art and every rect
//! the renderer draws outside it, clickable or not.

use serde::Deserialize;

use crate::overlay::DrawTrail;

/// Rectangle in overlay coordinates: `[left, top, right, bottom]`.
pub type RegionRect = [i32; 4];

/// Something the renderer draws outside the art, in overlay coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OverlayRect {
    /// `[x, y, width, height]`.
    pub rect: [i32; 4],
    /// "Open chat" and the quick pill take clicks. The bubble body and the
    /// thinking dots are only drawn, so clicks pass through them.
    pub clickable: bool,
    /// The sprite this rect rides with, and where that sprite was drawn when
    /// the rect was measured. `None` for a rect that stays put.
    #[serde(default)]
    pub follows: Option<Follows>,
}

/// Which Instance a rect is placed from, and that Instance's top-left, in
/// overlay coordinates, at the moment the renderer measured the rect.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Follows {
    pub instance: String,
    pub at: [i32; 2],
}

/// The rects a Windows region has to hold while the sprites they ride with
/// move. A rect kept only where it was measured loses its leading edge under
/// a drag, so each one is stretched over every spot its sprite is drawn at.
pub fn follow_placements(
    rects: &[OverlayRect],
    placements: impl Fn(&str) -> Option<Vec<(i32, i32)>>,
) -> Vec<OverlayRect> {
    rects
        .iter()
        .map(|overlay_rect| {
            let Some(Follows { instance, at }) = &overlay_rect.follows else {
                return overlay_rect.clone();
            };
            let [x, y, width, height] = overlay_rect.rect;
            let (mut left, mut top) = (x, y);
            let (mut right, mut bottom) = (x + width, y + height);
            for (px, py) in placements(instance).unwrap_or_default() {
                let (dx, dy) = (px - at[0], py - at[1]);
                left = left.min(x + dx);
                top = top.min(y + dy);
                right = right.max(x + width + dx);
                bottom = bottom.max(y + height + dy);
            }
            OverlayRect {
                rect: [left, top, right - left, bottom - top],
                clickable: overlay_rect.clickable,
                follows: None,
            }
        })
        .collect()
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

/// What an X11 overlay's bounding shape, which clips what it shows, needs.
#[derive(Debug, PartialEq, Eq)]
pub enum ShapeChange<'a> {
    /// Show only these `[left, top, right, bottom]` rects; none shows nothing.
    Clip(&'a [RegionRect]),
    /// Show the whole window: a compositor blends its transparent pixels.
    Unclip,
}

/// One X11 overlay's bounding shape, as last sent. With no compositor the
/// server fills transparent pixels with black, so the shape keeps only what
/// the renderer draws.
#[derive(Debug, Default)]
pub struct BoundingShape {
    clipped: Option<Vec<RegionRect>>,
}

impl BoundingShape {
    /// The change this frame needs, if any. `drawn` is what the renderer
    /// draws, and runs only when no compositor does.
    pub fn change_for(
        &mut self,
        composited: bool,
        drawn: impl FnOnce() -> Vec<RegionRect>,
    ) -> Option<ShapeChange<'_>> {
        if composited {
            return self.clipped.take().map(|_| ShapeChange::Unclip);
        }
        let drawn = drawn();
        if self.clipped.as_ref() == Some(&drawn) {
            return None;
        }
        Some(ShapeChange::Clip(self.clipped.insert(drawn)))
    }
}

/// Every overlay's bounding shape, one per display.
#[derive(Debug, Default)]
pub struct BoundingShapes(Vec<BoundingShape>);

impl BoundingShapes {
    /// Grows to cover every overlay; a new one starts unshaped. Never shrinks:
    /// a lost display's overlay is only hidden and keeps its X shape.
    pub fn fit(&mut self, overlays: usize) {
        if overlays > self.0.len() {
            self.0.resize_with(overlays, Default::default);
        }
    }
}

impl std::ops::Index<usize> for BoundingShapes {
    type Output = BoundingShape;
    fn index(&self, overlay: usize) -> &BoundingShape {
        &self.0[overlay]
    }
}

impl std::ops::IndexMut<usize> for BoundingShapes {
    fn index_mut(&mut self, overlay: usize) -> &mut BoundingShape {
        &mut self.0[overlay]
    }
}

/// One Character as an overlay sees it: its draw trail, and the shift from
/// shared space to that overlay's coordinates.
pub struct DrawnInstance<'a> {
    pub id: &'a str,
    pub trail: &'a DrawTrail,
    pub offset: (i32, i32),
}

/// What an overlay draws: every Character's swept art and every overlay rect,
/// each following rect stretched over its sprite's placements. Nothing while
/// the Characters are hidden.
pub fn drawn_region(
    visible: bool,
    instances: &[DrawnInstance<'_>],
    rects: &[OverlayRect],
) -> Vec<RegionRect> {
    if !visible {
        return Vec::new();
    }
    let art: Vec<[i32; 4]> = instances
        .iter()
        .flat_map(|instance| instance.trail.clip_rects(instance.offset))
        .collect();
    let rects = follow_placements(rects, |id| {
        let instance = instances.iter().find(|instance| instance.id == id)?;
        Some(instance.trail.placements(instance.offset).to_vec())
    });
    match region_plan(&art, &rects) {
        RegionPlan::Clear => Vec::new(),
        RegionPlan::Apply(region) => region,
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

/// The Characters whose sprite reaches into an overlay of `size`, in the order
/// given. `frame` is each one's `[x, y, width, height]` in overlay coordinates.
pub fn characters_on_overlay<Character>(
    characters: impl IntoIterator<Item = Character>,
    frame: impl Fn(&Character) -> [i32; 4],
    (overlay_width, overlay_height): (i32, i32),
) -> Vec<Character> {
    characters
        .into_iter()
        .filter(|character| {
            let [x, y, width, height] = frame(character);
            x + width > 0 && x < overlay_width && y + height > 0 && y < overlay_height
        })
        .collect()
}

#[cfg(test)]
#[allow(non_upper_case_globals)]
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
            follows: None,
        }
    }

    const fn drawn(rect: [i32; 4]) -> OverlayRect {
        OverlayRect {
            rect,
            clickable: false,
            follows: None,
        }
    }

    fn riding(rect: [i32; 4], instance: &str, at: [i32; 2]) -> OverlayRect {
        OverlayRect {
            follows: Some(Follows {
                instance: instance.to_string(),
                at,
            }),
            ..drawn(rect)
        }
    }

    /// A bubble measured at sprite (100, 200), since dragged to (130, 190),
    /// is drawn 30px right and 10px up. The region must hold both spots, or
    /// Windows clips the leading edge.
    #[test]
    fn a_bubble_is_kept_over_every_spot_its_dragged_sprite_puts_it() {
        let bubble = riding([120, 80, 200, 90], "buddy", [100, 200]);
        const parked: OverlayRect = drawn([0, 0, 40, 20]);
        let elsewhere = riding([500, 80, 200, 90], "gone", [480, 200]);
        let trail = |id: &str| (id == "buddy").then(|| vec![(110, 200), (120, 195), (130, 190)]);

        let got = follow_placements(&[bubble, parked.clone(), elsewhere], trail);

        assert_eq!(
            got,
            vec![
                drawn([120, 70, 230, 100]),
                parked,
                drawn([500, 80, 200, 90]),
            ],
        );
        let RegionPlan::Apply(region) = region_plan(&[], &got) else {
            panic!("a bubble keeps a region");
        };
        let at_drag_tip = [150, 70, 350, 160];
        assert!(
            region.iter().any(|[l, t, r, b]| *l <= at_drag_tip[0]
                && *t <= at_drag_tip[1]
                && *r >= at_drag_tip[2]
                && *b >= at_drag_tip[3]),
            "the bubble where the drag has taken it is outside {region:?}"
        );
    }

    /// Only a clickable rect stops the overlay passing a click, over its
    /// pixels and not one past them.
    #[test]
    fn only_a_clickable_rect_takes_a_click() {
        const open_chat: OverlayRect = clickable([220, 140, 72, 18]);
        const qm_pill: OverlayRect = clickable([130, 60, 160, 36]);
        const bubble: OverlayRect = drawn([120, 80, 200, 90]);
        const thinking: OverlayRect = drawn([150, 100, 48, 24]);

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
        for (name, rect, (x, y), takes) in &rows {
            let (x, y, takes) = (*x, *y, *takes);
            assert_eq!(rect.takes_click_at(x, y), takes, "{name}");
        }
    }

    /// What X11's input shape gets. Only the rects that take clicks go in;
    /// the bubble body and the thinking dots stay out so clicks pass them.
    #[test]
    fn clickable_rects_table() {
        const open_chat: OverlayRect = clickable([220, 140, 72, 18]);
        const qm_pill: OverlayRect = clickable([130, 60, 160, 36]);
        const bubble: OverlayRect = drawn([120, 80, 200, 90]);
        const thinking: OverlayRect = drawn([150, 100, 48, 24]);

        let rows: &[ClickRow] = &[
            ("nothing reported", &[], &[]),
            ("bubble only", &[bubble], &[]),
            ("thinking only", &[thinking], &[]),
            (
                "truncated dialogue: Open chat + bubble",
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
        const open_chat: OverlayRect = clickable([220, 140, 72, 18]);
        const bubble: OverlayRect = drawn([120, 80, 200, 90]);
        const thinking: OverlayRect = drawn([150, 100, 48, 24]);
        const qm_pill: OverlayRect = clickable([130, 60, 160, 36]);

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
                "truncated dialogue: Open chat + bubble",
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
    /// A 4x4 block of ink; scale 1.
    fn block() -> crate::overlay::AlphaMask {
        crate::overlay::AlphaMask::from_rows(&["####", "####", "####", "####"])
    }

    /// The trail of a Character placed at each of `steps` in turn.
    fn trail_through(steps: &[(i32, i32)]) -> DrawTrail {
        let mut trail = None;
        for &at in steps {
            trail = Some(DrawTrail::this_frame(trail, at, &block(), false, 1));
        }
        trail.unwrap()
    }

    fn shows(region: &[RegionRect], (x, y): (i32, i32)) -> bool {
        region
            .iter()
            .any(|&[left, top, right, bottom]| x >= left && x < right && y >= top && y < bottom)
    }

    /// Each Character reaching into the overlay is kept, not only the first;
    /// one wholly on another display, or only touching the edge, is not.
    #[test]
    fn every_character_reaching_into_the_overlay_is_kept() {
        let frames = [
            ("left", [100, 100, 64, 64]),
            ("on another display", [1920, 100, 64, 64]),
            ("right", [400, 100, 64, 64]),
            ("touching the left edge", [-64, 100, 64, 64]),
            ("straddling the left edge", [-32, 500, 64, 64]),
        ];
        let kept: Vec<&str> = characters_on_overlay(frames, |(_, frame)| *frame, (1920, 1080))
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(kept, ["left", "right", "straddling the left edge"]);
    }

    /// The shape hides whatever it leaves out, so it holds every
    /// Character on the overlay, not only the first.
    #[test]
    fn every_character_on_the_overlay_is_drawn() {
        let (one, two) = (trail_through(&[(100, 100)]), trail_through(&[(400, 100)]));
        let instances = [
            DrawnInstance {
                id: "one",
                trail: &one,
                offset: (0, 0),
            },
            DrawnInstance {
                id: "two",
                trail: &two,
                offset: (0, 0),
            },
        ];
        let region = drawn_region(true, &instances, &[]);
        assert!(shows(&region, (101, 101)), "first Character: {region:?}");
        assert!(shows(&region, (401, 101)), "second Character: {region:?}");
        assert!(
            !shows(&region, (250, 101)),
            "nothing between them: {region:?}"
        );
    }

    /// A moved Character is still shown where it was: the renderer may not
    /// have drawn the new spot yet when the shape lands.
    #[test]
    fn a_moved_character_is_drawn_where_it_was_too() {
        let moved = trail_through(&[(100, 100), (200, 100)]);
        let instances = [DrawnInstance {
            id: "one",
            trail: &moved,
            offset: (0, 0),
        }];
        let region = drawn_region(true, &instances, &[]);
        assert!(shows(&region, (101, 101)), "where it was: {region:?}");
        assert!(shows(&region, (201, 101)), "where it is: {region:?}");
    }

    /// A bubble is drawn too, and rides with its sprite: measured with the
    /// sprite at 100, it is also shown 10 px right once the sprite moves there.
    #[test]
    fn a_bubble_is_drawn_and_follows_its_sprite() {
        let moved = trail_through(&[(100, 100), (110, 100)]);
        let instances = [DrawnInstance {
            id: "one",
            trail: &moved,
            offset: (0, 0),
        }];
        let bubble = OverlayRect {
            rect: [100, 40, 40, 20],
            clickable: false,
            follows: Some(Follows {
                instance: "one".into(),
                at: [100, 100],
            }),
        };
        let region = drawn_region(true, &instances, &[bubble]);
        assert!(
            shows(&region, (101, 41)),
            "where it was measured: {region:?}"
        );
        assert!(
            shows(&region, (145, 41)),
            "where its sprite took it: {region:?}"
        );
    }

    /// Each Character is shifted into the overlay's own coordinates.
    #[test]
    fn a_character_is_drawn_in_overlay_coordinates() {
        let trail = trail_through(&[(2020, 100)]);
        let instances = [DrawnInstance {
            id: "one",
            trail: &trail,
            offset: (-1920, 0),
        }];
        let region = drawn_region(true, &instances, &[]);
        assert!(shows(&region, (101, 101)), "{region:?}");
    }

    /// Hidden Characters draw nothing, bubble included.
    #[test]
    fn hidden_characters_draw_nothing() {
        let trail = trail_through(&[(100, 100)]);
        let instances = [DrawnInstance {
            id: "one",
            trail: &trail,
            offset: (0, 0),
        }];
        let bubble = OverlayRect {
            rect: [100, 40, 40, 20],
            clickable: false,
            follows: None,
        };
        assert_eq!(
            drawn_region(false, &instances, &[bubble]),
            Vec::<RegionRect>::new()
        );
    }

    /// Frames of a walk: the sprite's ink one step further right each time.
    fn walk() -> impl Iterator<Item = Vec<RegionRect>> {
        (0..5).map(|step| vec![[100 + step * 4, 200, 180 + step * 4, 328]])
    }

    /// With a compositor the shape is never touched, and what is drawn is
    /// never even worked out, however the sprite moves.
    #[test]
    fn a_composited_overlay_sends_no_shape_while_its_sprite_walks() {
        let mut shape = BoundingShape::default();
        for _ in walk() {
            let change = shape.change_for(true, || panic!("drawn worked out under a compositor"));
            assert_eq!(change, None);
        }
    }

    /// With no compositor the overlay shows what is drawn and nothing
    /// else, and an overlay drawing nothing shows nothing rather than black.
    #[test]
    fn without_a_compositor_the_shape_follows_what_is_drawn() {
        let mut shape = BoundingShape::default();
        for drawn in walk() {
            let sent = drawn.clone();
            assert_eq!(
                shape.change_for(false, || sent),
                Some(ShapeChange::Clip(&drawn))
            );
        }
        let last = walk().last().unwrap();
        assert_eq!(
            shape.change_for(false, || last),
            None,
            "a still sprite resends nothing"
        );
        assert_eq!(
            shape.change_for(false, Vec::new),
            Some(ShapeChange::Clip(&[]))
        );
    }

    /// A compositor starting clears the clip once; stopping brings it back.
    #[test]
    fn the_shape_follows_the_compositor_both_ways() {
        let drawn = vec![[100, 200, 180, 328]];
        let mut shape = BoundingShape::default();
        assert_eq!(
            shape.change_for(false, || drawn.clone()),
            Some(ShapeChange::Clip(&drawn))
        );
        assert_eq!(
            shape.change_for(true, || panic!()),
            Some(ShapeChange::Unclip)
        );
        assert_eq!(shape.change_for(true, || panic!()), None);
        assert_eq!(
            shape.change_for(false, || drawn.clone()),
            Some(ShapeChange::Clip(&drawn))
        );
    }

    /// A display plugged in after launch gets an overlay, and that overlay
    /// is clipped on its first frame.
    #[test]
    fn a_display_added_after_launch_is_clipped() {
        let drawn = vec![[100, 200, 180, 328]];
        let mut shapes = BoundingShapes::default();
        shapes.fit(1);
        shapes.fit(2);
        assert_eq!(
            shapes[1].change_for(false, || drawn.clone()),
            Some(ShapeChange::Clip(&drawn))
        );
    }

    /// An unplugged display's overlay is only hidden and keeps its clip, so
    /// a compositor that starts meanwhile must unclip it once it's back.
    #[test]
    fn a_replugged_display_is_unclipped_when_a_compositor_started_meanwhile() {
        let drawn = vec![[100, 200, 180, 328]];
        let mut shapes = BoundingShapes::default();
        shapes.fit(2);
        shapes[1].change_for(false, || drawn.clone());
        shapes.fit(1);
        shapes.fit(2);
        assert_eq!(
            shapes[1].change_for(true, || panic!()),
            Some(ShapeChange::Unclip)
        );
    }

    /// The replugged overlay still has its clip, so unchanged art with no
    /// compositor sends nothing.
    #[test]
    fn a_replugged_display_with_unchanged_art_sends_nothing() {
        let drawn = vec![[100, 200, 180, 328]];
        let mut shapes = BoundingShapes::default();
        shapes.fit(2);
        shapes[1].change_for(false, || drawn.clone());
        shapes.fit(1);
        shapes.fit(2);
        assert_eq!(shapes[1].change_for(false, || drawn.clone()), None);
    }
}
