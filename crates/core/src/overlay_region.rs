/// Platform-neutral overlay region calculation.
///
/// Regions define which pixels are drawn and receive input. On Windows, `SetWindowRgn`
/// clips both. The region must include sprite DrawTrail, hotspots (clickable controls),
/// and painted rects (bubble body, thinking dots) so Windows doesn't clip them away.
/// Rectangle in overlay coordinates: `[left, top, right, bottom]`.
pub type RegionRect = [i32; 4];

/// Calculate overlay region rects from sprite trail, hotspots, and painted areas.
///
/// - `trail`: Sprite alpha mask rects `[left, top, right, bottom]` (already bounds)
/// - `hotspots`: Clickable control rects `[x, y, width, height]`
/// - `painted`: Painted UI rects (bubble, thinking, QM body if treated as paint)
///   `[x, y, width, height]`
///
/// Returns rects in `[left, top, right, bottom]` format for region APIs.
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

/// Comparison key for whether a Windows overlay must rebuild `SetWindowRgn`.
///
/// **Required shape:** art + hotspots + painted. Tip `89fb28d7` stores only
/// `(art, hotspots)` in `frame_loop::RegionParams` and fetches painted at apply
/// time — so an idle sprite whose bubble appears never rebuilds the region.
pub type WindowsRegionKey = (Vec<[i32; 4]>, Vec<[i32; 4]>, Vec<[i32; 4]>);

pub fn windows_region_key(
    art: &[[i32; 4]],
    hotspots: &[[i32; 4]],
    painted: &[[i32; 4]],
) -> WindowsRegionKey {
    (art.to_vec(), hotspots.to_vec(), painted.to_vec())
}

/// Whether the Windows region must be re-applied.
pub fn windows_region_must_rebuild(
    last: &WindowsRegionKey,
    next: &WindowsRegionKey,
) -> bool {
    last != next
}

/// Translate a shared-desktop `[x,y,w,h]` rect into one overlay's local coords
/// by subtracting the overlay origin (which may be negative).
pub fn rect_in_overlay(rect_xywh: [i32; 4], overlay_origin: (i32, i32)) -> [i32; 4] {
    let [x, y, w, h] = rect_xywh;
    [x - overlay_origin.0, y - overlay_origin.1, w, h]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oded's live dual-display geometry (points).
    fn oded_displays() -> [(i32, i32, i32, i32); 2] {
        // (x, y, w, h)
        [(0, 0, 3440, 1440), (-1200, -209, 1200, 1920)]
    }

    #[test]
    fn region_includes_painted_bubble_rect() {
        let trail = vec![[10, 10, 50, 50]];
        let hotspots = vec![[60, 60, 20, 15]];
        let painted = vec![[100, 20, 150, 80]];

        let rects = overlay_region_rects(&trail, &hotspots, &painted);

        let bubble_bounds = [100, 20, 250, 100];
        assert!(
            rects.contains(&bubble_bounds),
            "Region must include painted bubble rect. Expected {:?} in {:?}",
            bubble_bounds,
            rects
        );
    }

    #[test]
    fn region_without_painted_omits_bubble() {
        let trail = vec![[10, 10, 50, 50]];
        let hotspots = vec![[60, 60, 20, 15]];
        let painted = vec![];

        let rects = overlay_region_rects(&trail, &hotspots, &painted);

        assert_eq!(
            rects.len(),
            2,
            "Region should only have trail and hotspots when no painted rects"
        );
        assert!(rects.contains(&[10, 10, 50, 50]), "Should have trail");
        assert!(rects.contains(&[60, 60, 80, 75]), "Should have hotspot");
    }

    #[test]
    fn region_combines_all_sources() {
        let trail = vec![[0, 0, 10, 10], [20, 20, 30, 30]];
        let hotspots = vec![[40, 40, 5, 5]];
        let painted = vec![[60, 60, 20, 20]];

        let rects = overlay_region_rects(&trail, &hotspots, &painted);

        assert_eq!(rects.len(), 4, "Should have all rects from all sources");
        assert!(rects.contains(&[0, 0, 10, 10]));
        assert!(rects.contains(&[20, 20, 30, 30]));
        assert!(rects.contains(&[40, 40, 45, 45]));
        assert!(rects.contains(&[60, 60, 80, 80]));
    }

    #[test]
    fn empty_inputs_yield_empty_region() {
        let rects = overlay_region_rects(&[], &[], &[]);
        assert!(rects.is_empty(), "Empty inputs should yield empty region");
    }

    /// Table: (name, art LTRB, hotspots XYWH, painted XYWH, expect LTRB contains)
    #[test]
    fn region_composition_table() {
        let art = [[100, 200, 180, 328]]; // sprite swept bounds
        let more_hotspot = [220, 140, 72, 18]; // "Open chat"
        let bubble = [120, 80, 200, 90];
        let thinking = [150, 100, 48, 24];
        let qm_pill = [130, 60, 160, 36];

        let rows: &[(&str, &[[i32; 4]], &[[i32; 4]], &[[i32; 4]], &[[i32; 4]])] = &[
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
        ];

        for (name, trail, hotspots, painted, expect) in rows {
            let got = overlay_region_rects(trail, hotspots, painted);
            assert_eq!(
                got.len(),
                expect.len(),
                "{name}: rect count"
            );
            for rect in *expect {
                assert!(
                    got.contains(rect),
                    "{name}: missing {rect:?} in {got:?}"
                );
            }
        }
    }

    #[test]
    fn negative_origin_overlay_local_table() {
        let [_primary, portrait] = oded_displays();
        let origin = (portrait.0, portrait.1); // (-1200, -209)

        // Shared-desktop bubble near the portrait center.
        let shared_bubble = [-700, 400, 180, 70];
        let local = rect_in_overlay(shared_bubble, origin);
        assert_eq!(
            local,
            [500, 609, 180, 70],
            "portrait-local = shared - origin"
        );

        // Sprite straddling the seam x=0: shared top-left just left of seam.
        let shared_sprite = [-40, 600, 80, 128];
        let on_portrait = rect_in_overlay(shared_sprite, origin);
        assert_eq!(on_portrait, [1160, 809, 80, 128]);
        let on_primary = rect_in_overlay(shared_sprite, (0, 0));
        assert_eq!(on_primary, [-40, 600, 80, 128]);
    }

    #[test]
    fn windows_region_key_must_include_painted() {
        let art = [[0, 0, 10, 10]];
        let hotspots: [[i32; 4]; 0] = [];
        let before = windows_region_key(&art, &hotspots, &[]);
        let after = windows_region_key(&art, &hotspots, &[[100, 20, 150, 80]]);
        assert!(
            windows_region_must_rebuild(&before, &after),
            "idle sprite + new painted bubble must force SetWindowRgn rebuild"
        );
    }

    /// Documents tip's bug as a comparison: tip key = (art, hotspots) only.
    #[test]
    fn tip_bug_region_params_omit_painted_so_idle_bubble_never_rebuilds() {
        // Tip RegionParams type (frame_loop.rs): (Vec<[i32;4]>, Vec<[i32;4]>)
        type TipRegionParams = (Vec<[i32; 4]>, Vec<[i32; 4]>);
        let art = vec![[0, 0, 10, 10]];
        let hotspots: Vec<[i32; 4]> = vec![];
        let last: TipRegionParams = (art.clone(), hotspots.clone());
        let next: TipRegionParams = (art, hotspots);
        // Painted went [] → [[bubble]] but tip key unchanged:
        let tip_thinks_unchanged = last == next;
        assert!(
            !tip_thinks_unchanged,
            "RED at tip 89fb28d7: RegionParams omits painted, so decide_overlay_action \
             returns Nothing when an idle bubble/thinking appears. Fix: include painted \
             in the comparison key (see windows_region_key) and/or invalidate on \
             overlay_painted_rects."
        );
    }
}
