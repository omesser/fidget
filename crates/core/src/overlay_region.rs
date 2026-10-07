/// Platform-neutral overlay region calculation.
///
/// Regions define which pixels are drawn and receive input. On Windows, `SetWindowRgn`
/// clips both. The region must include sprite DrawTrail, hotspots (clickable controls),
/// and painted rects (bubble body, thinking dots) so Windows doesn't clip them away.

/// Rectangle in overlay coordinates: `[left, top, right, bottom]`.
pub type RegionRect = [i32; 4];

/// Calculate overlay region rects from sprite trail, hotspots, and painted areas.
///
/// - `trail`: Sprite alpha mask rects `[x, y, width, height]`
/// - `hotspots`: Clickable control rects `[x, y, width, height]`
/// - `painted`: Painted UI rects (bubble, thinking) `[x, y, width, height]`
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

#[cfg(test)]
mod tests {
    use super::*;

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
        // Trail is already [left, top, right, bottom]
        let trail = vec![[0, 0, 10, 10], [20, 20, 30, 30]];
        // Hotspots and painted are [x, y, width, height] and get converted
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
}
