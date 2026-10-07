//! Hit-testing the sprite against the cursor and placing the per-display
//! overlays, in points (physical pixels stop at `window_source::in_points`).
//! Click-through on macOS is per-window, so this decides per pixel whether it is drawn.

use crate::window_source::Rect;

/// The index of the display a point is on, or of the nearest one when it is on
/// none (a gap between displays, or a Throw past the outermost edge). `None`
/// only when no display was reported.
pub fn display_index_for(point: (f64, f64), displays: &[Rect]) -> Option<usize> {
    displays
        .iter()
        .position(|display| covers(point, display))
        .or_else(|| {
            displays
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| outside_by(point, a).total_cmp(&outside_by(point, b)))
                .map(|(index, _)| index)
        })
}

/// The one overlay that draws an Instance's speech bubble, or `None` when none
/// should: the renderer pulls a bubble back inside its display, so a copy on the
/// wrong display reappears at an edge. The overlay under the feet owns it.
pub fn bubble_owner(feet: (f64, f64), displays: &[Rect]) -> Option<usize> {
    displays
        .iter()
        .position(|display| covers(feet, display))
        // A seam resolves one way, to the display below. An edge with nothing
        // beyond it is a floor feet rest exactly on, hence the second pass. Off
        // every display, nothing: nearest would put a bubble beside an unseen sprite.
        .or_else(|| {
            displays
                .iter()
                .position(|display| outside_by(feet, display) <= FLOOR_SLACK * FLOOR_SLACK)
        })
}

/// Hysteresis margin: feet must move this far (in points) past a seam before
/// ownership switches. Prevents flicker when feet hover near a display boundary.
const HYSTERESIS_MARGIN: f64 = 8.0;

/// The one overlay that draws an Instance's speech bubble, with hysteresis to
/// prevent ownership flicker near seams. Keeps the previous owner until feet
/// move clearly onto another display.
pub fn bubble_owner_with_hysteresis(
    feet: (f64, f64),
    displays: &[Rect],
    previous_owner: Option<usize>,
) -> Option<usize> {
    // If we have a previous owner and feet are still near that display, keep it
    if let Some(prev) = previous_owner {
        if let Some(display) = displays.get(prev) {
            // Check if feet are still on the previous display or within hysteresis margin
            if covers(feet, display)
                || outside_by(feet, display) <= HYSTERESIS_MARGIN * HYSTERESIS_MARGIN
            {
                return Some(prev);
            }
        }
    }

    // Otherwise, determine owner normally
    bubble_owner(feet, displays)
}

/// How far outside its display feet may be and still be standing on it, in
/// points. Slack rather than equality because the floor is the display scaled
/// and clamped, which need not land back on the edge. Squared at the call site.
const FLOOR_SLACK: f64 = 1.0;

/// Whether `feet` are on `display`, including its floor and not its top edge.
/// Art hangs above the feet, so the edge shared with a display above is that
/// display's floor, not this interior.
pub(crate) fn stands_on(feet: (f64, f64), display: &Rect) -> bool {
    let on_floor = display.spans_x(feet.0) && (feet.1 - display.bottom()).abs() <= FLOOR_SLACK;
    let on_top_edge = display.spans_x(feet.0) && (feet.1 - display.y).abs() <= FLOOR_SLACK;
    (covers(feet, display) && !on_top_edge) || on_floor
}

/// Whether a display's window has this point, right and bottom edges excluded.
/// Half-open because a window is: a display 1920 wide at x=0 covers columns 0 to
/// 1919, and the seam column belongs to the display whose window starts there.
fn covers(point: (f64, f64), rect: &Rect) -> bool {
    (rect.x..rect.x + rect.width).contains(&point.0)
        && (rect.y..rect.y + rect.height).contains(&point.1)
}

/// How far outside a rectangle a point lies, squared. Zero anywhere inside it.
/// Squared because only the ordering is read.
fn outside_by(point: (f64, f64), rect: &Rect) -> f64 {
    let dx = (rect.x - point.0)
        .max(point.0 - (rect.x + rect.width))
        .max(0.0);
    let dy = (rect.y - point.1)
        .max(point.1 - (rect.y + rect.height))
        .max(0.0);

    dx * dx + dy * dy
}

/// Where to draw the art, given where the Character's feet are: the art hangs
/// above the feet and is centred on them, in the point space every display
/// shares. `SpriteRect::in_overlay` turns it into one overlay's own coordinates.
pub fn place_sprite(contact: (f64, f64), art_size: (i32, i32), scale: i32) -> SpriteRect {
    let (width, height) = art_size;

    SpriteRect {
        x: contact.0.round() as i32 - width / 2,
        y: contact.1.round() as i32 - height,
        scale,
    }
}

/// Where the sprite sits, and how far its art is blown up. `x` and `y` are the
/// top-left corner in whichever space it was placed in (`place_sprite` shared,
/// `in_overlay` one overlay's own); `scale` is the integer nearest-neighbour factor.
#[derive(Clone, Copy)]
pub struct SpriteRect {
    pub x: i32,
    pub y: i32,
    pub scale: i32,
}

impl SpriteRect {
    /// The same rectangle in one overlay's coordinates: points from the top-left
    /// of the display it covers. Every overlay draws its part of the same
    /// rectangle, so the halves of a Character on a seam meet with no gap or overlap.
    pub fn in_overlay(&self, overlay: Rect) -> SpriteRect {
        // No scale factor: displays with different backing factors share one point
        // space, and the overlay window covers its display exactly.
        SpriteRect {
            x: self.x - overlay.x.round() as i32,
            y: self.y - overlay.y.round() as i32,
            scale: self.scale,
        }
    }
}

/// Which pixels of an Animation frame are drawn, at the art's own resolution.
/// `PartialEq` and `Clone` because `character::Character` derives both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlphaMask {
    width: i32,
    height: i32,
    opaque: Vec<bool>,
}

impl AlphaMask {
    /// Build a mask from an ASCII picture: `#` is drawn, anything else is not.
    /// Tests only; it trusts every row to be the width of the first, fine for a
    /// literal and wrong for anything else.
    #[cfg(test)]
    pub fn from_rows(rows: &[&str]) -> Self {
        let height = rows.len() as i32;
        let width = rows.first().map_or(0, |r| r.chars().count()) as i32;
        let opaque = rows
            .iter()
            .flat_map(|r| r.chars().map(|c| c == '#'))
            .collect();

        Self {
            width,
            height,
            opaque,
        }
    }

    /// The art's own dimensions, before any scaling.
    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Raw mask data, (width, height, row-major opaque bools), for platform
    /// input-region APIs such as X11's XShapeCombineMask.
    pub fn raw(&self) -> (i32, i32, &[bool]) {
        (self.width, self.height, &self.opaque)
    }

    /// Build a mask from an 8-bit RGBA PNG, treating alpha at or above
    /// `threshold` as drawn. A threshold rather than "alpha > 0" so anti-aliased
    /// edges do not grow an invisible one-pixel border that swallows clicks.
    pub fn from_png(bytes: &[u8], threshold: u8) -> Result<Self, String> {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .map_err(|e| format!("sprite is not a readable PNG: {e}"))?;

        let size = reader
            .output_buffer_size()
            .ok_or_else(|| "sprite dimensions are implausibly large".to_string())?;
        let mut buf = vec![0; size];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| format!("sprite has no readable frame: {e}"))?;

        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(format!(
                "sprite must be 8-bit RGBA, found {:?} at {:?}",
                info.color_type, info.bit_depth
            ));
        }

        // `as_chunks` rather than `chunks_exact`: the 1.88 floor makes it
        // available and clippy asks for it. Same pixels, no remainder either way.
        let opaque = buf[..info.buffer_size()]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|px| px[3] >= threshold)
            .collect();

        Ok(Self {
            width: info.width as i32,
            height: info.height as i32,
            opaque,
        })
    }

    /// Whether the cursor is over a drawn pixel of the sprite. Bounds are checked
    /// before the divide because integer division truncates toward zero, so a
    /// cursor just left of the sprite would otherwise land on column 0.
    pub fn hit(&self, sprite: &SpriteRect, cursor_x: i32, cursor_y: i32, mirrored: bool) -> bool {
        let local_x = cursor_x - sprite.x;
        let local_y = cursor_y - sprite.y;
        if local_x < 0 || local_y < 0 {
            return false;
        }

        let px = local_x / sprite.scale;
        let py = local_y / sprite.scale;
        if px >= self.width || py >= self.height {
            return false;
        }

        // The renderer flips the art about the box's center when facing left; at
        // an integer scale that is exactly the reversed column.
        let px = if mirrored { self.width - 1 - px } else { px };
        self.opaque[(py * self.width + px) as usize]
    }

    /// Rectangles, `[left, top, right, bottom]`, covering every drawn pixel
    /// with the sprite's top-left anywhere between the `trail` positions.
    /// One rectangle per horizontal run of ink, stretched over the trail's bounds.
    pub fn swept_rects(&self, trail: &[(i32, i32)], mirrored: bool, scale: i32) -> Vec<[i32; 4]> {
        let Some(&(first_x, first_y)) = trail.first() else {
            return Vec::new();
        };
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (first_x, first_y, first_x, first_y);
        for &(x, y) in trail {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }

        let mut rects = Vec::new();
        for row in 0..self.height {
            let drawn = |column: i32| {
                let px = if mirrored {
                    self.width - 1 - column
                } else {
                    column
                };
                self.opaque[(row * self.width + px) as usize]
            };
            let mut column = 0;
            while column < self.width {
                if !drawn(column) {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < self.width && drawn(column) {
                    column += 1;
                }
                rects.push([
                    min_x + start * scale,
                    min_y + row * scale,
                    max_x + column * scale,
                    max_y + (row + 1) * scale,
                ]);
            }
        }
        rects
    }
}

/// One Instance's last three placements in shared space and the art drawn at
/// the last two. The renderer draws the previous art between the first two
/// placements until the latest arrives, then the latest art between the last two.
#[derive(Clone, Debug)]
pub struct DrawTrail {
    at: [(i32, i32); 3],
    art: [(AlphaMask, bool, i32); 2],
}

impl DrawTrail {
    /// A trail that has only ever been at `at`, drawing `mask`.
    pub fn start(at: (i32, i32), mask: &AlphaMask, mirrored: bool, scale: i32) -> Self {
        let art = (mask.clone(), mirrored, scale);
        Self {
            at: [at; 3],
            art: [art.clone(), art],
        }
    }

    /// The trail one placement later.
    pub fn advance(self, at: (i32, i32), mask: &AlphaMask, mirrored: bool, scale: i32) -> Self {
        let [_, before, last] = self.at;
        let [_, latest] = self.art;
        Self {
            at: [before, last, at],
            art: [latest, (mask.clone(), mirrored, scale)],
        }
    }

    /// Rectangles covering both spans the renderer may be drawing, shifted
    /// from shared space by `offset`. A Windows window region clips drawing,
    /// and it can land before or after the placement it was built from.
    pub fn clip_rects(&self, (dx, dy): (i32, i32)) -> Vec<[i32; 4]> {
        let at = self.at.map(|(x, y)| (x + dx, y + dy));
        let [(previous, previous_mirrored, previous_scale), (latest, mirrored, scale)] = &self.art;
        let mut rects = previous.swept_rects(&at[..2], *previous_mirrored, *previous_scale);
        rects.extend(latest.swept_rects(&at[1..], *mirrored, *scale));
        rects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `#` is an opaque pixel, `.` is fully transparent.
    #[test]
    fn cursor_over_transparent_pixel_is_not_a_hit() {
        let mask = AlphaMask::from_rows(&["..##..", ".####."]);
        let sprite = SpriteRect {
            x: 100,
            y: 200,
            scale: 4,
        };

        assert!(
            !mask.hit(&sprite, 100, 200, false),
            "top-left corner is transparent"
        );
        assert!(mask.hit(&sprite, 108, 200, false), "third column is opaque");
    }

    /// Facing left, the renderer draws the art flipped, so the clickable
    /// pixels must flip with it or an asymmetric sprite is clickable where it
    /// is empty and click-through where it is drawn.
    #[test]
    fn a_mirrored_sprite_hit_tests_against_the_flipped_columns() {
        let mask = AlphaMask::from_rows(&["##....", "##...."]);
        let sprite = SpriteRect {
            x: 100,
            y: 200,
            scale: 4,
        };

        assert!(
            mask.hit(&sprite, 100, 200, false),
            "as authored, the ink is on the left"
        );
        assert!(
            !mask.hit(&sprite, 100, 200, true),
            "mirrored, the left edge is clear"
        );
        assert!(
            mask.hit(&sprite, 123, 200, true),
            "and the ink is on the right"
        );
        assert!(!mask.hit(&sprite, 123, 200, false));
    }

    #[test]
    fn cursor_outside_the_sprite_is_not_a_hit() {
        let mask = AlphaMask::from_rows(&["####", "####"]);
        let sprite = SpriteRect {
            x: 100,
            y: 200,
            scale: 4,
        };

        // The sprite covers 100..116 horizontally, 200..208 vertically.
        assert!(
            mask.hit(&sprite, 100, 200, false),
            "top-left corner is inside"
        );
        assert!(
            mask.hit(&sprite, 115, 207, false),
            "bottom-right corner is inside"
        );

        assert!(
            !mask.hit(&sprite, 99, 204, false),
            "one point left of the sprite"
        );
        assert!(
            !mask.hit(&sprite, 116, 204, false),
            "one point right of the sprite"
        );
        assert!(
            !mask.hit(&sprite, 108, 199, false),
            "one point above the sprite"
        );
        assert!(
            !mask.hit(&sprite, 108, 208, false),
            "one point below the sprite"
        );
    }

    /// The renderer draws the sprite anywhere between its placements, and a
    /// Windows window region clips what is drawn, so each run of ink must
    /// reach from where it starts to where it ends.
    #[test]
    fn swept_rects_cover_the_sprite_drawn_between_placements() {
        let mask = AlphaMask::from_rows(&["##.#", "...."]);

        assert_eq!(
            mask.swept_rects(&[(10, 0), (16, 3), (13, 1)], false, 2),
            vec![[10, 0, 20, 5], [16, 0, 24, 5]],
        );
    }

    /// A walk changes frame mid-stride. Until the new placement lands, the
    /// renderer still draws the old frame, so its ink stays in the clip.
    #[test]
    fn a_trail_keeps_the_frame_still_on_screen_in_the_clip() {
        let stride = AlphaMask::from_rows(&["#."]);
        let next = AlphaMask::from_rows(&[".#"]);
        let trail = DrawTrail::start((0, 0), &stride, false, 1)
            .advance((10, 0), &stride, false, 1)
            .advance((20, 0), &next, false, 1);

        assert_eq!(
            trail.clip_rects((-100, 5)),
            vec![[-100, 5, -89, 6], [-89, 5, -78, 6]],
        );
    }

    #[test]
    fn swept_rects_at_rest_are_the_mirrored_ink() {
        let mask = AlphaMask::from_rows(&["##.#", ".#.."]);

        assert_eq!(
            mask.swept_rects(&[(10, 20)], true, 2),
            vec![[10, 20, 12, 22], [14, 20, 18, 22], [14, 22, 16, 24]],
        );
    }

    /// The fixture is a 2x2 RGBA PNG whose alpha values are, row-major:
    /// 0 (clear), 255 (opaque), 200 (mostly opaque), 10 (nearly clear).
    #[test]
    fn png_alpha_above_the_threshold_is_opaque() {
        let mask = AlphaMask::from_png(include_bytes!("../tests/fixtures/alpha-2x2.png"), 128)
            .expect("fixture decodes");
        let sprite = SpriteRect {
            x: 0,
            y: 0,
            scale: 1,
        };

        assert!(!mask.hit(&sprite, 0, 0, false), "alpha 0 is clear");
        assert!(mask.hit(&sprite, 1, 0, false), "alpha 255 is opaque");
        assert!(
            mask.hit(&sprite, 0, 1, false),
            "alpha 200 is above the threshold"
        );
        assert!(
            !mask.hit(&sprite, 1, 1, false),
            "alpha 10 is below the threshold"
        );
    }

    #[test]
    fn a_png_without_an_alpha_channel_is_rejected_by_name() {
        let err = AlphaMask::from_png(include_bytes!("../tests/fixtures/greyscale-2x2.png"), 128)
            .expect_err("greyscale has no alpha to hit-test against");

        assert!(
            err.contains("8-bit RGBA"),
            "error states the requirement: {err}"
        );
        assert!(
            err.contains("Grayscale"),
            "error names what was found: {err}"
        );
    }

    /// The art hangs above the feet and is centred on them, so a Character
    /// standing at a point is drawn above and to the left of it.
    #[test]
    fn the_art_hangs_above_the_contact_point_and_is_centred_on_it() {
        let sprite = place_sprite((500.0, 300.0), (128, 128), 4);

        assert_eq!(
            (sprite.x, sprite.y),
            (436, 172),
            "half a width left, a whole height up"
        );
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Two displays side by side: the sizes `window_source` produces for a 1080p
    /// display beside a Retina one, sharing one point space. The seam at 1920 is
    /// the point of the fixture; every case below is about which side a point falls.
    fn two_displays() -> [Rect; 2] {
        [
            rect(0.0, 0.0, 1920.0, 1080.0),
            rect(1920.0, 0.0, 1728.0, 1117.0),
        ]
    }

    /// The issue this module is here for: a Character standing on the seam is
    /// drawn by both overlays, and the halves have to meet.
    #[test]
    fn a_sprite_on_a_seam_is_two_halves_that_meet() {
        let displays = two_displays();
        // Feet on the seam. 128 points of art, so 64 fall on each display.
        let sprite = place_sprite((1920.0, 700.0), (128, 128), 4);

        let left = sprite.in_overlay(displays[0]);
        let right = sprite.in_overlay(displays[1]);

        assert_eq!((left.x, left.y), (1856, 572), "overlay 0 draws from 1856");
        assert_eq!((right.x, right.y), (-64, 572), "overlay 1 draws from -64");
        assert_eq!(
            1920 - left.x,
            -right.x,
            "the columns overlay 0 runs out of room for are the ones overlay 1 hides \
             behind its own left edge: no gap and no overlap"
        );
    }

    /// Displays stack vertically too, and the origin subtracted is the whole
    /// origin rather than an x.
    #[test]
    fn an_overlay_below_another_subtracts_its_own_top_edge() {
        let displays = [
            rect(0.0, 0.0, 1920.0, 1080.0),
            rect(0.0, 1080.0, 1920.0, 1080.0),
        ];
        let sprite = place_sprite((500.0, 1080.0), (128, 128), 4);

        assert_eq!(
            sprite.in_overlay(displays[0]).y,
            952,
            "128 up from the seam"
        );
        assert_eq!(sprite.in_overlay(displays[1]).y, -128, "and 128 above it");
    }

    /// A display whose origin is not a whole number of points, which fractional
    /// scaling produces. The sprite still lands on the pixel grid.
    #[test]
    fn a_fractional_display_origin_is_rounded_to_the_pixel_grid() {
        let sprite = place_sprite((500.0, 300.0), (128, 128), 4);

        // Both halves round up, so truncating either one is a wrong answer.
        assert_eq!(
            sprite.in_overlay(rect(100.6, 50.6, 800.0, 600.0)).x,
            335,
            "436 - 101"
        );
        assert_eq!(
            sprite.in_overlay(rect(100.6, 50.6, 800.0, 600.0)).y,
            121,
            "172 - 51"
        );
    }

    /// A bubble is drawn by exactly one overlay, the one under the feet, and by
    /// none when the feet are on no display, where the cursor's nearest-display
    /// fallback would clamp a bubble into view beside a sprite that is not there.
    #[test]
    fn a_bubble_belongs_to_the_display_under_the_feet_or_to_none() {
        let displays = two_displays();

        // Art wide enough to straddle the seam; the feet are still on the first.
        assert_eq!(bubble_owner((1919.9, 1000.0), &displays), Some(0));
        assert_eq!(
            bubble_owner((1920.0, 1000.0), &displays),
            Some(1),
            "ownership flips once, at the seam column, and nowhere else"
        );

        assert_eq!(
            bubble_owner((4000.0, 500.0), &displays),
            None,
            "thrown past the right edge: no overlay draws a bubble"
        );
        assert_eq!(
            bubble_owner((960.0, 1200.0), &displays),
            None,
            "below the first display's bottom edge, with nothing beneath it"
        );
        assert_eq!(
            display_index_for((4000.0, 500.0), &displays),
            Some(1),
            "where the cursor's question still gets its nearest answer"
        );
    }

    /// An edge with nothing beyond it is not a seam but a floor a Character
    /// comes to rest on. See `bubble_owner`.
    #[test]
    fn an_edge_is_a_seam_when_a_display_lies_beyond_it_and_a_floor_when_none_does() {
        let displays = two_displays();

        assert_eq!(
            bubble_owner((960.0, 1080.0), &displays),
            Some(0),
            "standing on the first display's bottom edge, not off the desktop"
        );
        assert_eq!(
            bubble_owner((3648.0, 500.0), &displays),
            Some(1),
            "and where a walk ends against the outermost side"
        );

        let a_hair_adrift = f64::from_bits(1080.0_f64.to_bits() + 1);
        assert_eq!(
            bubble_owner((960.0, a_hair_adrift), &displays),
            Some(0),
            "a floor one unit in the last place adrift is still that floor"
        );

        let stacked = [
            rect(0.0, 0.0, 1920.0, 1080.0),
            rect(0.0, 1080.0, 1920.0, 1080.0),
        ];
        assert_eq!(
            bubble_owner((960.0, 1080.0), &stacked),
            Some(1),
            "and a display below turns that same edge back into a seam"
        );
    }

    /// The excluded bottom edge is where feet rest on the Dock's display too:
    /// once the Dock's bounds are readable it becomes a Perch, and
    /// `floor_under_dock` hands the floor beside it back at the display's bottom.
    #[test]
    fn the_dock_rests_feet_on_the_very_edge_its_own_display_excludes() {
        use crate::window_source::{floor_under_dock, plausible_dock};

        let displays = two_displays();
        // This desktop as the window server reports it: a Dock strip near the
        // bottom of the first display, and work areas reserving the menu bar on
        // both with the Dock's thickness on the first alone.
        let dock = rect(120.0, 978.0, 1680.0, 92.0);
        let work = [
            rect(0.0, 30.0, 1920.0, 952.0),
            rect(1920.0, 32.0, 1728.0, 1085.0),
        ];

        assert!(
            plausible_dock(&dock, displays[0], work[0]),
            "the Dock read is believed, so the floor below is built from it"
        );

        let floors = [
            floor_under_dock(work[0], displays[0], &dock),
            floor_under_dock(work[1], displays[1], &dock),
        ];
        let feet = |floor: Rect, x: f64| (x, floor.y + floor.height);

        assert_eq!(
            feet(floors[0], 960.0).1,
            1080.0,
            "the Dock's display rests feet on its own bottom edge, not above the Dock"
        );
        assert_eq!(
            feet(floors[1], 2784.0).1,
            1117.0,
            "and a display reserving nothing below already did"
        );

        assert_eq!(
            bubble_owner(feet(floors[0], 960.0), &displays),
            Some(0),
            "a Character idling beside the Dock owns the bubble on the Dock's display"
        );
        assert_eq!(
            bubble_owner(feet(floors[1], 2784.0), &displays),
            Some(1),
            "and one on the second screen's floor owns it there — #178 as reported"
        );
    }

    /// Several Instances are several bubbles. Ownership is a function of one
    /// Instance's feet, so two characters on two displays each own a bubble at
    /// once, the case a fix aimed at "only one bubble" wrongly suppresses.
    #[test]
    fn each_instance_owns_a_bubble_on_the_display_it_stands_on() {
        let displays = two_displays();

        let first = bubble_owner((960.0, 1000.0), &displays);
        let second = bubble_owner((2600.0, 1000.0), &displays);

        assert_eq!(first, Some(0));
        assert_eq!(second, Some(1));
        assert_ne!(
            first, second,
            "two characters on two displays are two bubbles, not one"
        );

        assert_eq!(
            bubble_owner((100.0, 1000.0), &displays),
            first,
            "and two characters on the same display share that display's overlay"
        );
    }

    #[test]
    fn a_point_on_a_display_belongs_to_that_display() {
        let displays = two_displays();

        assert_eq!(display_index_for((960.0, 540.0), &displays), Some(0));
        assert_eq!(display_index_for((2600.0, 500.0), &displays), Some(1));
        assert_eq!(
            display_index_for((1920.5, 500.0), &displays),
            Some(1),
            "half a point across the seam is across it"
        );
        assert_eq!(
            display_index_for((1920.0, 500.0), &displays),
            Some(1),
            "the seam column is the second display's first column, not the first \
             display's last: that is where its window starts"
        );
    }

    /// Displays are not always flush, and a cursor can sit in the gap between
    /// two of them. The click still belongs to one overlay.
    #[test]
    fn a_point_in_the_gap_between_displays_belongs_to_the_nearer_one() {
        let displays = [
            rect(0.0, 0.0, 1920.0, 1080.0),
            rect(2000.0, 0.0, 1728.0, 1117.0),
        ];

        assert_eq!(display_index_for((1930.0, 500.0), &displays), Some(0));
        assert_eq!(display_index_for((1990.0, 500.0), &displays), Some(1));
    }

    /// A Throw carries the sprite past the outermost edge before physics
    /// catches it, and a display can be unplugged with the sprite on it.
    #[test]
    fn a_point_off_every_display_belongs_to_the_nearest_one() {
        let displays = two_displays();

        assert_eq!(
            display_index_for((4000.0, 2000.0), &displays),
            Some(1),
            "off the bottom-right corner of the desktop"
        );
        assert_eq!(
            display_index_for((-500.0, -500.0), &displays),
            Some(0),
            "off the top-left corner of the desktop"
        );
    }

    #[test]
    fn no_displays_is_no_overlay_to_belong_to() {
        assert_eq!(display_index_for((960.0, 540.0), &[]), None);
    }

    /// Negative-origin displays (portrait monitor LEFT of primary): Windows dual-display geometry
    /// from live failure (overlay-0 3440x1440 at origin, overlay-1 1200x1920 at negative offset).
    #[test]
    fn bubble_owner_with_negative_origin_displays() {
        let displays = [
            rect(0.0, 0.0, 3440.0, 1440.0),
            rect(-1200.0, -209.0, 1200.0, 1920.0),
        ];

        assert_eq!(
            bubble_owner((80.0, 80.0), &displays),
            Some(0),
            "startup on primary"
        );
        assert_eq!(
            bubble_owner((1720.0, 720.0), &displays),
            Some(0),
            "center of primary"
        );

        assert_eq!(
            bubble_owner((-600.0, 500.0), &displays),
            Some(1),
            "center of portrait"
        );
        assert_eq!(
            bubble_owner((-100.0, 100.0), &displays),
            Some(1),
            "near right edge of portrait"
        );

        assert_eq!(
            bubble_owner((0.0, 100.0), &displays),
            Some(0),
            "seam x=0 belongs to primary"
        );
        assert_eq!(
            bubble_owner((-1.0, 100.0), &displays),
            Some(1),
            "just left of seam is portrait"
        );
    }

    #[test]
    fn bubble_owner_with_hysteresis_negative_origin() {
        let displays = [
            rect(0.0, 0.0, 3440.0, 1440.0),
            rect(-1200.0, -209.0, 1200.0, 1920.0),
        ];

        let mut prev = None;

        let owner = bubble_owner_with_hysteresis((80.0, 80.0), &displays, prev);
        assert_eq!(owner, Some(0), "starts on primary");
        prev = owner;

        let owner = bubble_owner_with_hysteresis((5.0, 100.0), &displays, prev);
        assert_eq!(owner, Some(0), "hysteresis keeps primary near seam");
        prev = owner;

        let owner = bubble_owner_with_hysteresis((-20.0, 100.0), &displays, prev);
        assert_eq!(owner, Some(1), "switches to portrait beyond hysteresis");
        prev = owner;

        let owner = bubble_owner_with_hysteresis((-5.0, 100.0), &displays, prev);
        assert_eq!(owner, Some(1), "hysteresis keeps portrait near seam");
    }
}
