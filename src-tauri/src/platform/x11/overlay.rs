//! X11 overlay window configuration: floating, non-activating, click-through.
//!
//! GTK has no click-through finer than the whole window, so `XShapeCombineMask`
//! carves the input region from the sprite's alpha mask, and
//! `XShapeCombineRectangles` unions any hotspot rectangles the renderer
//! reported so a control drawn outside the art still receives clicks.
//! EWMH window states float the overlay above other windows and skip the
//! taskbar and pager. On GDK's Wayland backend tao's handle is a
//! `wl_surface`, which nothing here matches; the input region is core Wayland
//! and unwired. DESIGN.md decision 3.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, Once};
use std::time::Instant;

use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, SK};
use x11rb::protocol::xproto::{self, AtomEnum, ConfigureWindowAux, PropMode, StackMode};
use x11rb::rust_connection::RustConnection;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

static MASK_UPDATE_COUNT: AtomicU64 = AtomicU64::new(0);
static MASK_UPDATE_NANOS: AtomicU64 = AtomicU64::new(0);
static FULL_REBUILD_COUNT: AtomicU64 = AtomicU64::new(0);
static POSITION_ONLY_COUNT: AtomicU64 = AtomicU64::new(0);

static WINDOW_ID_MAP: LazyLock<Mutex<HashMap<String, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, PartialEq, Eq, Debug)]
struct MaskShape {
    opaque: Vec<bool>,
    width: i32,
    height: i32,
    mirrored: bool,
    scale: i32,
}

struct CachedPixmap {
    pixmap: u32,
    gc: u32,
    shape: MaskShape,
}

/// An overlay window and which of the Characters on it, in placement order.
type MaskSlot = (u32, usize);

static PIXMAP_CACHE: LazyLock<Mutex<HashMap<MaskSlot, CachedPixmap>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// GDK's answer, kept current by `composited-changed`. True until the first
/// overlay is configured, so nothing is shaped before an answer exists.
static COMPOSITED: AtomicBool = AtomicBool::new(true);

/// Whether a compositor runs on the overlays' screen.
pub fn composited() -> bool {
    COMPOSITED.load(Ordering::Relaxed)
}

/// Whether a compositor runs now, before any overlay is configured. Main thread.
pub fn composited_now() -> bool {
    watch_compositor();
    composited()
}

/// Set once GDK's answer is read and followed.
static WATCH: Once = Once::new();

/// Stand in for GDK's answer: mark the watch done, then set the flag.
#[cfg(test)]
pub fn set_composited_for_test(composited: bool) {
    WATCH.call_once(|| {});
    COMPOSITED.store(composited, Ordering::Relaxed);
}

/// Read GDK's compositing state once and follow it. Runs on the GTK main thread.
fn watch_compositor() {
    WATCH.call_once(|| {
        let Some(screen) = gtk::gdk::Screen::default() else {
            return;
        };
        COMPOSITED.store(screen.is_composited(), Ordering::Relaxed);
        screen.connect_composited_changed(|screen| {
            COMPOSITED.store(screen.is_composited(), Ordering::Relaxed);
        });
    });
}

/// Float above other windows, non-activating, skip the taskbar and pager.
/// Returns Err when the handle is not realized yet, so the caller can retry.
pub fn configure_overlay(window: &tauri::WebviewWindow) -> Result<(), String> {
    let raw_window_handle = match window.window_handle() {
        Ok(handle) => handle,
        Err(e) => {
            return Err(format!("Window handle not available yet: {}", e));
        }
    };

    let x_window = match raw_window_handle.as_raw() {
        RawWindowHandle::Xlib(xlib_window) => xlib_window.window as u32,
        RawWindowHandle::Xcb(xcb_window) => xcb_window.window.get(),
        _ => {
            return Err("Wayland surface, not an X11 window (no X server answered, \
                        or GDK_BACKEND names wayland): EWMH states and the \
                        per-pixel input region are unwired for Wayland, so the \
                        overlay keeps GTK's defaults"
                .to_string());
        }
    };

    let Some(conn) = super::connection::connection() else {
        return Err("Failed to get X11 connection".to_string());
    };

    watch_compositor();
    // Record X window ID for cleanup (Destroyed event fires after handle is invalid)
    WINDOW_ID_MAP
        .lock()
        .unwrap()
        .insert(window.label().to_string(), x_window);

    set_ewmh_states(conn, x_window)?;
    Ok(())
}

/// One Character's part of an overlay's input region: its art, where it sits
/// in the window, and how it is drawn.
pub struct SpriteMask<'a> {
    pub mask: &'a fidget_core::overlay::AlphaMask,
    pub x: i32,
    pub y: i32,
    pub mirrored: bool,
    pub scale: i32,
}

/// `XShapeCombineMask` sets the input region: no sprites makes the entire
/// window click-through; otherwise clicks go only to every sprite's opaque
/// pixels and any hotspot rectangles the renderer reported.
pub fn update_input_region(
    window: &tauri::WebviewWindow,
    sprites: &[SpriteMask<'_>],
    hotspot_rects: &[[i32; 4]],
) -> Result<(), String> {
    let raw_window_handle = match window.window_handle() {
        Ok(handle) => handle,
        Err(e) => {
            return Err(format!("Window handle not available yet: {}", e));
        }
    };

    let x_window = match raw_window_handle.as_raw() {
        RawWindowHandle::Xlib(xlib_window) => xlib_window.window as u32,
        RawWindowHandle::Xcb(xcb_window) => xcb_window.window.get(),
        _ => {
            // Wayland: the input region is unwired here, and an Err would retry
            // forever.
            return Ok(());
        }
    };

    let Some(conn) = super::connection::connection() else {
        return Err("Failed to get X11 connection".to_string());
    };

    if sprites.is_empty() {
        clear_input_region(conn, x_window)?;
    } else {
        apply_input_mask(conn, x_window, sprites, hotspot_rects)?;
    }

    Ok(())
}

fn should_rebuild_mask(
    cache: &HashMap<MaskSlot, CachedPixmap>,
    slot: MaskSlot,
    current_shape: &MaskShape,
) -> bool {
    cache
        .get(&slot)
        .is_none_or(|cached| &cached.shape != current_shape)
}

/// One `scale`-sized square per opaque pixel, in pixmap coordinates.
/// `mirrored` flips the columns, matching `AlphaMask::hit` and the renderer.
fn mask_rects(shape: &MaskShape) -> Vec<xproto::Rectangle> {
    let MaskShape {
        opaque,
        width,
        height,
        mirrored,
        scale,
    } = shape;
    let mut rects = Vec::new();
    for y in 0..*height {
        for x in 0..*width {
            if opaque[(y * width + x) as usize] {
                let column = if *mirrored { width - 1 - x } else { x };
                rects.push(xproto::Rectangle {
                    x: (column * scale) as i16,
                    y: (y * scale) as i16,
                    width: *scale as u16,
                    height: *scale as u16,
                });
            }
        }
    }
    rects
}

/// Rectangles per `PolyFillRectangle` within the core request length, in 4-byte
/// units (at least 4096 by protocol): 12 bytes of header, 8 per rectangle. x11rb
/// errors past it rather than splitting, and BIG-REQUESTS costs a round trip.
fn rects_per_fill(max_units: u16) -> usize {
    (usize::from(max_units.max(4096)) * 4 - 12) / 8
}

/// The same bound for `ShapeRectangles`, whose header is 16 bytes.
fn rects_per_shape(max_units: u16) -> usize {
    (usize::from(max_units.max(4096)) * 4 - 16) / 8
}

/// The pixmap holding `sprite`'s mask in `slot`, rebuilt only when its shape
/// changed. Returns whether it was rebuilt.
fn sprite_pixmap(
    conn: &RustConnection,
    slot: MaskSlot,
    sprite: &SpriteMask<'_>,
) -> Result<(u32, bool), String> {
    let (width, height, opaque) = sprite.mask.raw();
    let current_shape = MaskShape {
        opaque: opaque.to_vec(),
        width,
        height,
        mirrored: sprite.mirrored,
        scale: sprite.scale,
    };

    let mut cache = PIXMAP_CACHE.lock().unwrap();
    if !should_rebuild_mask(&cache, slot, &current_shape) {
        POSITION_ONLY_COUNT.fetch_add(1, Ordering::Relaxed);
        return Ok((cache.get(&slot).unwrap().pixmap, false));
    }
    FULL_REBUILD_COUNT.fetch_add(1, Ordering::Relaxed);

    if let Some(cached) = cache.remove(&slot) {
        xproto::free_gc(conn, cached.gc).ok();
        xproto::free_pixmap(conn, cached.pixmap).ok();
    }

    let scaled_width = width * sprite.scale;
    let scaled_height = height * sprite.scale;

    let screen = &conn.setup().roots[0];
    let pixmap = conn
        .generate_id()
        .map_err(|e| format!("Failed to generate pixmap ID: {e}"))?;

    xproto::create_pixmap(
        conn,
        1, // 1-bit depth for mask
        pixmap,
        screen.root,
        scaled_width as u16,
        scaled_height as u16,
    )
    .map_err(|e| format!("Failed to create pixmap: {e}"))?;

    let gc = conn
        .generate_id()
        .map_err(|e| format!("Failed to generate GC ID: {e}"))?;
    xproto::create_gc(conn, gc, pixmap, &Default::default())
        .map_err(|e| format!("Failed to create GC: {e}"))?;

    // Clear the pixmap to 0 (transparent). CreatePixmap contents are undefined.
    xproto::poly_fill_rectangle(
        conn,
        pixmap,
        gc,
        &[xproto::Rectangle {
            x: 0,
            y: 0,
            width: scaled_width as u16,
            height: scaled_height as u16,
        }],
    )
    .map_err(|e| format!("Failed to clear pixmap: {e}"))?;

    // Set GC foreground to 1 for opaque pixels (default is 0)
    xproto::change_gc(conn, gc, &xproto::ChangeGCAux::new().foreground(1))
        .map_err(|e| format!("Failed to set GC foreground: {e}"))?;

    let rects = mask_rects(&current_shape);
    let per_fill = rects_per_fill(conn.setup().maximum_request_length);
    for batch in rects.chunks(per_fill) {
        xproto::poly_fill_rectangle(conn, pixmap, gc, batch)
            .map_err(|e| format!("Failed to draw rectangles: {e}"))?;
    }

    cache.insert(
        slot,
        CachedPixmap {
            pixmap,
            gc,
            shape: current_shape,
        },
    );

    Ok((pixmap, true))
}

/// The first sprite's mask sets the input region and each later one is
/// unioned in, so every Character on the overlay takes clicks.
fn apply_input_mask(
    conn: &RustConnection,
    window: u32,
    sprites: &[SpriteMask<'_>],
    hotspot_rects: &[[i32; 4]],
) -> Result<(), String> {
    let rebuild_start = Instant::now();

    let mut rebuilt = Vec::with_capacity(sprites.len());
    for (index, sprite) in sprites.iter().enumerate() {
        let (pixmap, shape_changed) = sprite_pixmap(conn, (window, index), sprite)?;
        rebuilt.push(shape_changed);
        let operation = if index == 0 {
            shape::SO::SET
        } else {
            shape::SO::UNION
        };
        shape::mask(
            conn,
            operation,
            SK::INPUT,
            window,
            sprite.x as i16,
            sprite.y as i16,
            pixmap,
        )
        .map_err(|e| format!("Failed to apply input mask: {e}"))?;
    }
    free_cached_pixmaps(conn, window, sprites.len());

    if !hotspot_rects.is_empty() {
        let x11_rects: Vec<xproto::Rectangle> = hotspot_rects
            .iter()
            .map(|&[hx, hy, hw, hh]| xproto::Rectangle {
                x: hx as i16,
                y: hy as i16,
                width: hw as u16,
                height: hh as u16,
            })
            .collect();

        shape::rectangles(
            conn,
            shape::SO::UNION,
            SK::INPUT,
            xproto::ClipOrdering::UNSORTED,
            window,
            0,
            0,
            &x11_rects,
        )
        .map_err(|e| format!("Failed to union hotspot rectangles: {e}"))?;
    }

    conn.flush()
        .map_err(|e| format!("Failed to flush X11: {e}"))?;

    let rebuild_elapsed = rebuild_start.elapsed().as_nanos() as u64;
    MASK_UPDATE_COUNT.fetch_add(1, Ordering::Relaxed);
    MASK_UPDATE_NANOS.fetch_add(rebuild_elapsed, Ordering::Relaxed);

    if std::env::var("FIDGET_TRACE_MASK_REBUILD").is_ok() {
        for (sprite, shape_changed) in sprites.iter().zip(rebuilt) {
            let (width, height, opaque) = sprite.mask.raw();
            let opaque_count = opaque.iter().filter(|&&b| b).count();
            eprintln!(
                "mask_rebuild: {}x{} @{}x scale, {} opaque pixels, {:.3} ms{}",
                width,
                height,
                sprite.scale,
                opaque_count,
                rebuild_elapsed as f64 / 1_000_000.0,
                if shape_changed {
                    ""
                } else {
                    " (position-only)"
                }
            );
        }
    }

    Ok(())
}

/// Send one overlay's bounding shape change, unchecked: an X error reaches the
/// XI2 listener's log instead of costing a round trip.
pub fn change_bounding_shape(
    label: &str,
    change: fidget_core::overlay_region::ShapeChange<'_>,
) -> bool {
    use fidget_core::overlay_region::ShapeChange;
    let Some(window) = WINDOW_ID_MAP.lock().unwrap().get(label).copied() else {
        return false;
    };
    let Some(conn) = super::connection::connection() else {
        return false;
    };
    let sent = match change {
        ShapeChange::Unclip => shape::mask(
            conn,
            shape::SO::SET,
            SK::BOUNDING,
            window,
            0,
            0,
            x11rb::NONE,
        )
        .is_ok(),
        ShapeChange::Clip(region) => {
            let rects: Vec<xproto::Rectangle> = region
                .iter()
                .map(|&[left, top, right, bottom]| xproto::Rectangle {
                    x: left as i16,
                    y: top as i16,
                    width: (right - left) as u16,
                    height: (bottom - top) as u16,
                })
                .collect();
            let per_request = rects_per_shape(conn.setup().maximum_request_length);
            // SET with no rects leaves nothing shown, which is what an empty region means.
            let mut batches = rects.chunks(per_request);
            let first = batches.next().unwrap_or(&[]);
            std::iter::once((shape::SO::SET, first))
                .chain(batches.map(|batch| (shape::SO::UNION, batch)))
                .all(|(op, batch)| {
                    shape::rectangles(
                        conn,
                        op,
                        SK::BOUNDING,
                        xproto::ClipOrdering::UNSORTED,
                        window,
                        0,
                        0,
                        batch,
                    )
                    .is_ok()
                })
        }
    };
    sent && conn.flush().is_ok()
}

/// Free the window's pixmaps from slot `first` on: Characters no longer on it.
fn free_cached_pixmaps(conn: &RustConnection, window: u32, first: usize) {
    PIXMAP_CACHE
        .lock()
        .unwrap()
        .retain(|&(cached_window, slot), cached| {
            let keep = cached_window != window || slot < first;
            if !keep {
                xproto::free_gc(conn, cached.gc).ok();
                xproto::free_pixmap(conn, cached.pixmap).ok();
            }
            keep
        });
}

fn clear_input_region(conn: &RustConnection, window: u32) -> Result<(), String> {
    free_cached_pixmaps(conn, window, 0);

    shape::mask(conn, shape::SO::SET, SK::INPUT, window, 0, 0, x11rb::NONE)
        .map_err(|e| format!("Failed to clear input region: {e}"))?
        .check()
        .map_err(|e| format!("X11 error clearing input region: {e}"))?;

    conn.flush()
        .map_err(|e| format!("Failed to flush X11: {e}"))?;

    Ok(())
}

/// Free cached X11 pixmap and GC for a destroyed overlay window.
pub fn cleanup_overlay_window(window: &tauri::WebviewWindow) {
    let Some(conn) = super::connection::connection() else {
        eprintln!("cleanup_overlay_window: failed to get X11 connection");
        return;
    };

    let label = window.label().to_string();

    // Destroyed fires after the window leaves Tauri's list, so window_handle() fails.
    // Use the X window ID we recorded at setup instead.
    let x_window = {
        let mut map = WINDOW_ID_MAP.lock().unwrap();
        match map.remove(&label) {
            Some(id) => {
                eprintln!(
                    "cleanup_overlay_window: freeing pixmap for window {} (X ID {})",
                    label, id
                );
                id
            }
            None => {
                eprintln!(
                    "cleanup_overlay_window: no recorded X window ID for {}",
                    label
                );
                return;
            }
        }
    };

    free_cached_pixmaps(conn, x_window, 0);
}

/// The overlay is not an application window, so the window manager must not treat it as one.
fn set_ewmh_states(conn: &RustConnection, window: u32) -> Result<(), String> {
    let atoms = super::atoms::atoms().ok_or("Failed to intern EWMH atoms")?;

    let states = [
        atoms.net_wm_state_above,
        atoms.net_wm_state_skip_taskbar,
        atoms.net_wm_state_skip_pager,
    ];
    xproto::change_property(
        conn,
        PropMode::REPLACE,
        window,
        atoms.net_wm_state,
        AtomEnum::ATOM,
        32,
        states.len() as u32,
        bytemuck::cast_slice(&states),
    )
    .map_err(|e| format!("Failed to set EWMH states: {e}"))?
    .check()
    .map_err(|e| format!("X11 error setting states: {e}"))?;

    conn.flush()
        .map_err(|e| format!("Failed to flush X11 after EWMH states: {e}"))?;

    // ABOVE band puts the overlay above normal windows. Within that band, BELOW
    // reorders so Settings (also ABOVE) stacks higher than the overlay.
    let lower_aux = ConfigureWindowAux::new().stack_mode(StackMode::BELOW);
    xproto::configure_window(conn, window, &lower_aux)
        .map_err(|e| format!("Failed to lower overlay: {e}"))?
        .check()
        .map_err(|e| format!("X11 error lowering overlay: {e}"))?;

    conn.flush()
        .map_err(|e| format!("Failed to flush X11: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mirrored_mask_draws_the_flipped_columns() {
        let at = |mirrored| -> Vec<(i16, i16)> {
            let shape = MaskShape {
                opaque: vec![true, false, false],
                width: 3,
                height: 1,
                mirrored,
                scale: 2,
            };
            mask_rects(&shape).iter().map(|r| (r.x, r.y)).collect()
        };
        assert_eq!(at(false), [(0, 0)], "as authored, ink stays on the left");
        assert_eq!(at(true), [(4, 0)], "mirrored, ink moves to the right");
    }

    #[test]
    fn a_fill_request_stays_within_the_core_request_length() {
        // Units of 4 bytes, from Setup. 65535 is the core ceiling, 4096 the floor
        // the protocol guarantees.
        assert_eq!(rects_per_fill(65535), 32766);
        assert_eq!(rects_per_fill(4096), 2046);
        assert_eq!(rects_per_shape(4096), 2046);
    }

    #[test]
    fn mask_shape_equality() {
        let shape1 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 2,
        };

        let shape2 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 2,
        };

        let shape3 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: true,
            scale: 2,
        };

        assert_eq!(shape1, shape2);
        assert_ne!(shape1, shape3);
    }

    #[test]
    fn frame_change_triggers_rebuild() {
        let mut cache = HashMap::new();
        let window1 = (1, 0);

        let shape1 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        let shape2 = MaskShape {
            opaque: vec![true, true, false],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        cache.insert(
            window1,
            CachedPixmap {
                pixmap: 100,
                gc: 200,
                shape: shape1.clone(),
            },
        );

        assert!(
            should_rebuild_mask(&cache, window1, &shape2),
            "Frame change should trigger rebuild"
        );
    }

    #[test]
    fn mirror_change_triggers_rebuild() {
        let mut cache = HashMap::new();
        let window1 = (1, 0);

        let shape1 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        let shape2 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: true,
            scale: 1,
        };

        cache.insert(
            window1,
            CachedPixmap {
                pixmap: 100,
                gc: 200,
                shape: shape1.clone(),
            },
        );

        assert!(
            should_rebuild_mask(&cache, window1, &shape2),
            "Mirror change should trigger rebuild"
        );
    }

    #[test]
    fn scale_change_triggers_rebuild() {
        let mut cache = HashMap::new();
        let window1 = (1, 0);

        let shape1 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        let shape2 = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 2,
        };

        cache.insert(
            window1,
            CachedPixmap {
                pixmap: 100,
                gc: 200,
                shape: shape1.clone(),
            },
        );

        assert!(
            should_rebuild_mask(&cache, window1, &shape2),
            "Scale change should trigger rebuild"
        );
    }

    #[test]
    fn position_only_reuses_cache() {
        let mut cache = HashMap::new();
        let window1 = (1, 0);

        let shape = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        cache.insert(
            window1,
            CachedPixmap {
                pixmap: 100,
                gc: 200,
                shape: shape.clone(),
            },
        );

        assert!(
            !should_rebuild_mask(&cache, window1, &shape),
            "Same shape should reuse cache (position-only update)"
        );
    }

    #[test]
    fn two_windows_have_separate_caches() {
        let mut cache = HashMap::new();

        let shape1 = MaskShape {
            opaque: vec![true, false],
            width: 2,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        let shape2 = MaskShape {
            opaque: vec![false, true],
            width: 2,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        let window1 = (1, 0);
        let window2 = (2, 0);

        cache.insert(
            window1,
            CachedPixmap {
                pixmap: 100,
                gc: 200,
                shape: shape1.clone(),
            },
        );

        cache.insert(
            window2,
            CachedPixmap {
                pixmap: 300,
                gc: 400,
                shape: shape2.clone(),
            },
        );

        assert_eq!(cache.get(&window1).unwrap().shape, shape1);
        assert_eq!(cache.get(&window2).unwrap().shape, shape2);

        cache.remove(&window1);
        assert!(!cache.contains_key(&window1));
        assert_eq!(cache.get(&window2).unwrap().shape, shape2);
    }

    #[test]
    fn window2_same_shape_as_window1_rebuilds() {
        let mut cache = HashMap::new();

        let shape = MaskShape {
            opaque: vec![true, false, true],
            width: 3,
            height: 1,
            mirrored: false,
            scale: 1,
        };

        let window1 = (1, 0);
        let window2 = (2, 0);

        cache.insert(
            window1,
            CachedPixmap {
                pixmap: 100,
                gc: 200,
                shape: shape.clone(),
            },
        );

        assert!(
            should_rebuild_mask(&cache, window2, &shape),
            "Window 2 with same shape as cached window 1 must rebuild (per-window cache)"
        );
    }

    /// Run with: Xvfb :97 &
    /// DISPLAY=:97 cargo test --bin fidget x11::overlay -- --include-ignored
    #[test]
    #[ignore = "needs an X server"]
    fn every_character_on_a_window_takes_clicks() {
        use x11rb::protocol::shape::ConnectionExt as _;
        use x11rb::protocol::xproto::ConnectionExt as _;
        let (conn, screen) = x11rb::connect(None).expect("an X server on DISPLAY");
        let root = conn.setup().roots[screen].root;
        let window = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            100,
            100,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &Default::default(),
        )
        .unwrap();
        let mask = fidget_core::overlay::AlphaMask::from_png(
            include_bytes!("../../../../crates/core/tests/fixtures/alpha-2x2.png"),
            128,
        )
        .unwrap();
        let sprite = |x, y| SpriteMask {
            mask: &mask,
            x,
            y,
            mirrored: false,
            scale: 1,
        };

        apply_input_mask(&conn, window, &[sprite(0, 0), sprite(50, 50)], &[]).unwrap();

        let rects = conn
            .shape_get_rectangles(window, SK::INPUT)
            .unwrap()
            .reply()
            .unwrap()
            .rectangles;
        let takes_click = |x: i16, y: i16| {
            rects.iter().any(|r| {
                (r.x..r.x + r.width as i16).contains(&x)
                    && (r.y..r.y + r.height as i16).contains(&y)
            })
        };
        assert!(takes_click(1, 0) && takes_click(0, 1), "first Character");
        assert!(
            takes_click(51, 50) && takes_click(50, 51),
            "second Character"
        );
        assert!(!takes_click(0, 0) && !takes_click(25, 25), "clear pixels");
        free_cached_pixmaps(&conn, window, 0);
    }

    #[test]
    fn stats_tracking() {
        MASK_UPDATE_COUNT.store(0, Ordering::Relaxed);
        MASK_UPDATE_NANOS.store(0, Ordering::Relaxed);
        FULL_REBUILD_COUNT.store(0, Ordering::Relaxed);
        POSITION_ONLY_COUNT.store(0, Ordering::Relaxed);

        let total = MASK_UPDATE_COUNT.load(Ordering::Relaxed);
        let full = FULL_REBUILD_COUNT.load(Ordering::Relaxed);
        let position_only = POSITION_ONLY_COUNT.load(Ordering::Relaxed);

        assert_eq!(total, 0);
        assert_eq!(full, 0);
        assert_eq!(position_only, 0);
    }
}
