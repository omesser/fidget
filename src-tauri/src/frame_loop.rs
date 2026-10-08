use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fidget_core::director::{self, Context, Happened, Wake};
use fidget_core::dispatch::{
    dispatch, DenyList, DispatchContext, FeetAt, InstanceInfo, PlacementQuery,
};
use fidget_core::engine::{bring_off_fullscreen, BehaviorProposal, State, Verb};
use fidget_core::input::press_target;
#[cfg(not(unix))]
use fidget_core::overlay::DrawTrail;
use fidget_core::overlay::{bubble_owner, display_index_for, drawn_frame, place_sprite};
use fidget_core::quick_message::walk_held;
use fidget_core::roster::{InstanceId, Roster};
use fidget_core::scheduler;
use fidget_core::sensing::{Activity, DesktopSense, SystemClock};
use fidget_core::snapshot::SnapshotAssembler;
use fidget_core::speech;
use fidget_core::visibility::{fullscreen_displays, Change, Desktop, HideRules};
use fidget_core::window_source::{Rect, WindowSource};
use tauri::{Emitter, Manager};

use super::chat_surface::{CHAT_APPEARANCE_EVENT, CHAT_EVENT, CHAT_STATUS_EVENT, CHAT_UI_EVENT};
use super::session_log;
use super::settings::{ChatAppearance, SettingsOp};
use super::{
    apply_menu_action, cancelled_caret, chat_is_up, chat_label, close_chat, completer,
    describe_menu, dev_flags, harness, mcp_http, mcp_resources, menu, model, note_happened,
    open_chat, overlay_label, paced, place_overlays, platform, publish_instances,
    push_chat_opening, push_chat_openings, remember_instances, spawn_live, sprite_width,
    stand_roster, switch_instance, tray, ChatMsg, ChatReply, ChatStatus, ChatStatusPush,
    DirectorRun, Drawn, FrameExtras, InstanceState, MenuChannel, MenuHold, MenuSignal, Placed,
    Placement, SpritePlacement, Traced, TrayHandle, ENGINE_TICK, FRAME_EVENT, MENU_HOLD_TIMEOUT,
    SENSE_INTERVAL,
};

/// How long an overlay may go without being told anything.
/// A webview that begins listening mid-silence hears nothing until the sprite
/// next moves. 250ms is under a hide-rule fade, so a launch-hidden Character still goes.
const FRAME_RESEND: Duration = Duration::from_millis(250);

fn cursor_near_sprite(cursor: Option<(f64, f64)>, sprites: &[(i32, i32, i32, i32)]) -> bool {
    let Some((cx, cy)) = cursor else {
        return false;
    };
    const INFLATE: f64 = 48.0;
    for &(sx, sy, sw, sh) in sprites {
        let left = (sx as f64) - INFLATE;
        let top = (sy as f64) - INFLATE;
        let right = (sx as f64) + (sw as f64) + INFLATE;
        let bottom = (sy as f64) + (sh as f64) + INFLATE;
        if cx >= left && cx <= right && cy >= top && cy <= bottom {
            return true;
        }
    }
    false
}

/// One overlay's last applied shape: mask, x, y, facing, scale, and hotspot
/// rectangles. Named because clippy's `type_complexity` rejects the tuple
/// inline. Only X11 keeps one: XShape must not rebuild every tick.
#[cfg(any(test, all(unix, not(target_os = "macos"))))]
type MaskParams = (Option<Vec<bool>>, i32, i32, i32, i32, Vec<[i32; 4]>);

/// One Windows overlay's last applied region: the sprite's swept ink and the
/// rects the renderer draws outside it. The region is what gets compared, so a
/// trail that settles after a walk still rebuilds it.
#[cfg(not(unix))]
type RegionParams = (Vec<[i32; 4]>, Vec<fidget_core::overlay_region::OverlayRect>);

#[derive(Debug, PartialEq, Eq)]
#[cfg(any(test, not(unix)))]
enum OverlayAction {
    ApplyMask,
    ToggleOnly,
    Nothing,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg(any(test, not(unix)))]
enum RegionAction {
    ApplyMask,
    ToggleClickThrough,
    Nothing,
}

/// `region_plan` says whether there is anything to keep; an overlay with nothing
/// passes clicks, and one already doing so needs no call.
#[cfg(any(test, not(unix)))]
fn decide_region_action(
    sprite: &[[i32; 4]],
    rects: &[fidget_core::overlay_region::OverlayRect],
    ignore: bool,
) -> RegionAction {
    use fidget_core::overlay_region::{region_plan, RegionPlan};

    if region_plan(sprite, rects) != RegionPlan::Clear {
        RegionAction::ApplyMask
    } else if ignore {
        RegionAction::Nothing
    } else {
        RegionAction::ToggleClickThrough
    }
}

#[cfg(any(test, not(unix)))]
fn decide_overlay_action<Shape: PartialEq>(
    last_mask: Option<&Shape>,
    new_mask: &Shape,
    mask_in_flight: bool,
    toggle_in_flight: bool,
    applied_ignoring: Option<bool>,
    new_ignore: bool,
) -> OverlayAction {
    let mask_changed = last_mask != Some(new_mask);
    let ignore_changed = applied_ignoring != Some(new_ignore);

    if mask_changed && !mask_in_flight {
        OverlayAction::ApplyMask
    } else if !mask_changed && ignore_changed && !toggle_in_flight {
        OverlayAction::ToggleOnly
    } else {
        OverlayAction::Nothing
    }
}

/// The frame loop: assemble a snapshot, tick the Engine, apply the `Frame`.
/// Webview and hit-test share a loop; the hit-test leads by up to one tick (src/interpolate.js).
// One over clippy's cap: Director config belongs here, not mixed with window geometry.
/// Decide which dialogue the Chat surface should show.
/// Returns the Harness's parsed dialogue if available,
/// otherwise falls back to the Engine's frame dialogue.
fn chat_dialogue<'a>(
    parsed_from_harness: Option<&'a str>,
    from_engine_frame: Option<&'a str>,
) -> Option<&'a str> {
    parsed_from_harness.or(from_engine_frame)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_frame_loop(
    app: tauri::AppHandle,
    mut roster: Roster,
    mut lives: Vec<InstanceState>,
    source: impl WindowSource + Send + Sync + 'static,
    displays: platform::DisplayCache,
    rules: Arc<Mutex<HideRules>>,
    covered: Vec<Rect>,
    director_run: DirectorRun,
    menu_channel: MenuChannel,
    extras: FrameExtras,
) {
    let MenuChannel {
        sender: menu_sender,
        receiver: menu_signals,
        quit_generation,
    } = menu_channel;
    thread::spawn(move || {
        // macOS only. The window-list read there costs milliseconds, so it runs
        // beside this loop and the tick copies the sample when it lands.
        // Other platforms stay inline.
        #[cfg(target_os = "macos")]
        let mut assembler = SnapshotAssembler::new(source).detach_poll();
        #[cfg(not(target_os = "macos"))]
        let mut assembler = SnapshotAssembler::new(source);
        let DirectorRun {
            mut config,
            settings: mut director,
            inspect,
        } = director_run;
        // Track whether we need to emit "something can answer now" after the
        // first successful wake. Set when `first_connection` is true (going from
        // not configured to configured), cleared after the first successful wake
        // or if configuration is lost. The bug: emitting it immediately on Retarget
        // shows the banner even when harness spawn will fail moments later.
        let mut pending_first_connection_message = false;
        let FrameExtras {
            settings,
            settings_path,
            characters,
            instances: instance_rows,
            ops,
            chat,
            mcp,
        } = extras;
        let mut slots = completer::Slots::new();
        publish_instances(&roster, &instance_rows);
        let (mut tray_actions, mut last_menu) = {
            let installed: Vec<String> = characters.keys().cloned().collect();
            let current = lives
                .first()
                .map(|live| live.character.name.clone())
                .unwrap_or_default();
            let id = lives
                .first()
                .map(|live| live.id.clone())
                .unwrap_or_default();
            let settings_now = settings.lock().ok().map(|s| s.clone()).unwrap_or_default();
            let rules_now = rules.lock().ok();
            let description = describe_menu(
                &installed,
                &current,
                &roster,
                &id,
                &settings_now,
                rules_now.as_deref().unwrap_or(&HideRules::default()),
            );
            (description.actions.clone(), Some(description))
        };

        // Read once for every Instance: there is one desktop and one user, and
        // asking AppKit how long they have been idle once per character would be
        // the same answer bought several times.
        let mut desktop_sense = DesktopSense::default();
        let activity_source = platform::activity_source();
        let mut since_sense = Duration::ZERO;
        let mut last_activity: Option<Activity> = None;

        // One click-through flag per overlay, `None` until that overlay's first
        // decision so the first tick always applies.
        let mut ignoring: Vec<Option<bool>> = vec![None; covered.len()];

        // Confirmed click-through per overlay, set on main thread when
        // toggle or mask apply succeeds. Frame thread checks to decide
        // when new toggle or mask apply is needed.
        #[cfg(not(unix))]
        let applied_ignoring: Arc<Mutex<Vec<Option<bool>>>> =
            Arc::new(Mutex::new(vec![None; covered.len()]));

        // The last instruction sent to each overlay and when, so a tick that
        // repeats one does not send it again. See the emit site for the
        // measurement that earns this.
        let mut last_frame: Vec<Option<(String, Instant)>> = vec![None; covered.len()];

        // EWMH configure retried until it succeeds: GTK may have no window
        // handle immediately after show(). Shared so the main thread can report.
        let configured = Arc::new(Mutex::new(vec![false; covered.len()]));

        // XShape mask retried until it succeeds. Do not set
        // ignore_cursor_events(false) until then, or the overlay becomes a
        // click-eater. Shared so the main thread can report.
        let mask_applied = Arc::new(Mutex::new(vec![false; covered.len()]));

        // Avoid queuing redundant main-thread posts every 16ms.
        #[cfg(not(target_os = "macos"))]
        let configure_in_flight = Arc::new(Mutex::new(vec![false; covered.len()]));
        #[cfg(not(target_os = "macos"))]
        let mask_in_flight = Arc::new(Mutex::new(vec![false; covered.len()]));
        #[cfg(not(unix))]
        let toggle_in_flight = Arc::new(Mutex::new(vec![false; covered.len()]));

        // Cache last applied mask parameters to avoid rebuilding the region
        // every 16ms. Shared with the main thread, which writes the params back
        // here once update_input_region has accepted them.
        #[cfg(all(unix, not(target_os = "macos")))]
        let last_mask: Arc<Mutex<Vec<MaskParams>>> =
            Arc::new(Mutex::new(vec![
                (None, 0, 0, 1, 1, Vec::new());
                covered.len()
            ]));
        #[cfg(not(unix))]
        let last_mask: Arc<Mutex<Vec<RegionParams>>> =
            Arc::new(Mutex::new(vec![(Vec::new(), Vec::new()); covered.len()]));
        // Each Instance's draw trail, for a Windows region that clips drawing.
        #[cfg(not(unix))]
        let mut trails: std::collections::HashMap<InstanceId, DrawTrail> =
            std::collections::HashMap::new();

        // The displays the overlays cover, as setup left them. Shared with the
        // main thread, which is the only place that can change what they cover
        // and so the only place that knows when this is true again.
        let covered = Arc::new(Mutex::new(covered));

        // Spawn XI2 input event listener on X11. When available, the frame loop
        // blocks on this channel when idle instead of polling at 16ms. #183.
        #[cfg(all(unix, not(target_os = "macos")))]
        let mut input_events = platform::spawn_xi2_listener();
        // macOS: the same channel, fed by a mouse event tap, and only once the
        // user has turned Input Monitoring on. Starts `None` so a leftover OS
        // grant cannot listen before the first tick reads the setting. `mut`
        // because that setting can be flipped while the loop runs (#721).
        #[cfg(target_os = "macos")]
        let mut input_events: Option<platform::EventTap> = None;
        #[cfg(not(unix))]
        let mut input_events: Option<mpsc::Receiver<()>> = None;

        let mut button_was_down = false;
        let mut sound_allowed = true;
        let mut ticks: u32 = 0;
        // Counted apart from `frame:` lines, so the tick rate can be read with
        // FIDGET_TRACE_FRAMES off and its per-tick print ruled out as the cause.
        let counting_ticks = crate::tracing_cadence();

        // Log DND state at startup when TRACE_BUBBLE is on.
        if dev_flags::TRACE_BUBBLE.is_on() {
            let dnd = settings
                .lock()
                .ok()
                .map(|s| s.do_not_disturb)
                .unwrap_or(false);
            eprintln!("startup: dnd={}", if dnd { "on" } else { "off" });
        }
        let mut counted_ticks: u32 = 0;
        let mut counted_since = Instant::now();
        let mut last_tick = Instant::now();
        let mut turn_started = Instant::now();
        let mut tick_deadline = turn_started;
        let mut moving = true;
        let mut time_since_launch = Duration::ZERO;
        let mut tour_triggered = false;
        let mut schedule_mode = scheduler::ScheduleMode::Active;
        let mut was_visible = true;
        let mut names_sent: Option<crate::names_hint::NamesHint> = None;

        let mut last_cursor: Option<(f64, f64)> = None;
        let mut last_sprite_rects: Vec<(i32, i32, i32, i32)> = Vec::new();

        loop {
            // The tap follows the setting: checked and granted, it starts here
            // and the arms below block on it; unchecked, Drop stops the tap
            // thread, back to the Stage 2b back-off (#721).
            #[cfg(target_os = "macos")]
            {
                let wanted = crate::consent::wanted(crate::consent::CapabilityId::InputMonitoring);
                if !wanted {
                    // Drop stops the tap thread from this side. A grant
                    // revoked while the setting stays on is still not caught
                    // here: the loop waits out the deadline below, which is
                    // the back-off it would have had anyway (#721).
                    input_events = None;
                } else if input_events.is_none() && schedule_mode == scheduler::ScheduleMode::Idle {
                    // Idle only: this asks TCC whether the grant has landed
                    // yet, and an Active tick asking 60 times a second would
                    // cost more than the tap saves. An idle wait is at most a
                    // second while the poll is still on, so a grant made in
                    // System Settings is picked up about that fast.
                    input_events = platform::spawn_event_tap();
                }
            }

            // A listener that hung up is worse than none: `recv_timeout` on a
            // closed channel returns at once, which is a spin rather than a
            // sleep. Dropped after the match, where nothing borrows it.
            let mut listener_hung_up = false;

            // Active sleeps 16ms. Idle blocks until a real deadline with no
            // cap (#183). Hidden: sleep, ignore input events. Visible
            // (Asleep/DND included): the listener stays on so hit-testing keeps
            // Poke/Grab/Throw.
            match (schedule_mode, was_visible, &input_events) {
                (scheduler::ScheduleMode::Idle, true, Some(events)) => {
                    let next_director = lives
                        .iter()
                        .filter_map(|live| {
                            let remaining = live.pace.wait().saturating_sub(live.since_wake);
                            if remaining.is_zero() {
                                None
                            } else {
                                Some(remaining)
                            }
                        })
                        .min()
                        .unwrap_or(Duration::from_secs(3600));

                    let next_sense = SENSE_INTERVAL.saturating_sub(since_sense);
                    let deadline = next_director.min(next_sense);

                    // `matches!` rather than `==`: the X11 channel carries an
                    // `InputEvent`, which derives no `PartialEq`.
                    listener_hung_up = matches!(
                        events.recv_timeout(deadline),
                        Err(mpsc::RecvTimeoutError::Disconnected)
                    );
                }
                (scheduler::ScheduleMode::Idle, false, Some(_events)) => {
                    // Hidden idle: deep sleep, input events ignored. Only
                    // non-input callbacks unblock.
                    let next_director = lives
                        .iter()
                        .filter_map(|live| {
                            let remaining = live.pace.wait().saturating_sub(live.since_wake);
                            if remaining.is_zero() {
                                None
                            } else {
                                Some(remaining)
                            }
                        })
                        .min()
                        .unwrap_or(Duration::from_secs(3600));

                    let next_sense = SENSE_INTERVAL.saturating_sub(since_sense);
                    let deadline = next_director.min(next_sense);

                    thread::sleep(deadline);
                }
                _ => {
                    // No listener: poll with back-off. That is Windows always,
                    // Wayland always, and macOS until the user grants Input
                    // Monitoring (#183 Stage 2b).
                    match (schedule_mode, was_visible) {
                        (scheduler::ScheduleMode::Active, _) => {
                            let now = Instant::now();
                            tick_deadline = scheduler::next_tick(
                                ENGINE_TICK,
                                tick_deadline,
                                turn_started,
                                now,
                                moving,
                            );
                            // Still ticks keep the coalesced sleep, the slower
                            // rate #183 relies on.
                            if moving {
                                platform::sleep_precisely(tick_deadline - now);
                            } else {
                                thread::sleep(tick_deadline - now);
                            }
                        }
                        (scheduler::ScheduleMode::Idle, false) => {
                            // Hidden idle: uncapped deep sleep. Only non-input
                            // callbacks unblock.
                            let next_director = lives
                                .iter()
                                .filter_map(|live| {
                                    let remaining =
                                        live.pace.wait().saturating_sub(live.since_wake);
                                    if remaining.is_zero() {
                                        None
                                    } else {
                                        Some(remaining)
                                    }
                                })
                                .min()
                                .unwrap_or(Duration::from_secs(3600));

                            let next_sense = SENSE_INTERVAL.saturating_sub(since_sense);
                            let deadline = next_director.min(next_sense);

                            thread::sleep(deadline);
                        }
                        (scheduler::ScheduleMode::Idle, true) => {
                            let next_director = lives
                                .iter()
                                .filter_map(|live| {
                                    let remaining =
                                        live.pace.wait().saturating_sub(live.since_wake);
                                    if remaining.is_zero() {
                                        None
                                    } else {
                                        Some(remaining)
                                    }
                                })
                                .min()
                                .unwrap_or(Duration::from_secs(3600));

                            let next_sense = SENSE_INTERVAL.saturating_sub(since_sense);
                            let deadline = next_director.min(next_sense);

                            // Visible idle input polling. Cap at 100ms when the cursor is near a
                            // sprite (within 48px) for hover gesture responsiveness. Otherwise cap
                            // at 1s to avoid burning CPU waiting for rare events (right-click).
                            let capped = if cursor_near_sprite(last_cursor, &last_sprite_rects) {
                                deadline.min(Duration::from_millis(100))
                            } else {
                                deadline.min(Duration::from_secs(1))
                            };
                            thread::sleep(capped);
                        }
                    }
                }
            }

            // After the wait, so a turn that ends early at a `continue` still
            // sleeps a whole tick next time rather than spinning.
            turn_started = Instant::now();

            if listener_hung_up {
                // The listener thread ended: the tap lost its grant, or X11
                // went away. Back off rather than spin; on macOS the next
                // iteration starts a new tap if the setting is still on.
                input_events = None;
            }

            // Read per tick: the Development tab can flip these while the
            // loop runs. Click-through is invisible; this trace is the only
            // way to watch it. Off unless asked; see scripts/verify-overlay.sh.
            let tracing = dev_flags::TRACE_HITTEST.is_on();

            // Likewise for the Frame: where the sprite is and what it is doing
            // is the loop's only output, and a screenshot cannot say whether it
            // got there by falling.
            let tracing_frames = dev_flags::TRACE_FRAMES.is_on();
            // And for what the Engine is playing. The frame line above says
            // which Animation is on screen but not what chose it: a `talk` is a
            // proposed Behavior, a cursor reaction and a Dwell alike.
            let tracing_engine = dev_flags::TRACE_ENGINE.is_on();
            // A click is two edges. The periodic hit-test line only prints on
            // a click-through flip or every two seconds, so a press that did
            // not flip left no record of whether the button was seen.
            let tracing_director = model::tracing();
            let tracing_clicks = tracing || tracing_frames || tracing_director;

            if dev_flags::DEBUG_IPC.is_on() {
                if let Some(live) = lives.first() {
                    let state_str = live
                        .last_state
                        .map(|s| format!("{:?}", s))
                        .unwrap_or_else(|| "None".to_string());
                    crate::debug::check_debug_commands(
                        (live.last_position.x as i32, live.last_position.y as i32),
                        &state_str,
                        &mut roster,
                    );
                }
            }

            let Ok(cursor) = app.cursor_position() else {
                continue;
            };

            // The windowing layer reports the global cursor against the
            // primary's scale, so that factor undoes it. From the cache:
            // asking a monitor means `NSScreen`, main-thread only.
            let displays = displays.read();
            let cursor_scale = displays.cursor_scale;

            // The Engine works in points; undoing the cursor's scale puts it in that space.
            // Computed once and reused for both hover detection (last_cursor) and hit tests.
            let cursor_points = fidget_core::engine::Point {
                x: cursor.x / cursor_scale,
                y: cursor.y / cursor_scale,
            };
            last_cursor = Some((cursor_points.x, cursor_points.y));

            ignoring.resize(displays.frames.len(), None);
            last_frame.resize(displays.frames.len(), None);
            configured
                .lock()
                .unwrap()
                .resize(displays.frames.len(), false);
            mask_applied
                .lock()
                .unwrap()
                .resize(displays.frames.len(), false);
            #[cfg(not(unix))]
            applied_ignoring
                .lock()
                .unwrap()
                .resize(displays.frames.len(), None);
            #[cfg(not(target_os = "macos"))]
            configure_in_flight
                .lock()
                .unwrap()
                .resize(displays.frames.len(), false);
            #[cfg(not(target_os = "macos"))]
            mask_in_flight
                .lock()
                .unwrap()
                .resize(displays.frames.len(), false);
            #[cfg(not(unix))]
            toggle_in_flight
                .lock()
                .unwrap()
                .resize(displays.frames.len(), false);
            #[cfg(all(unix, not(target_os = "macos")))]
            last_mask
                .lock()
                .unwrap()
                .resize(displays.frames.len(), (None, 0, 0, 1, 1, Vec::new()));
            #[cfg(not(unix))]
            last_mask
                .lock()
                .unwrap()
                .resize(displays.frames.len(), Default::default());

            // Wall time since the last tick that reached the Engine, not
            // since the last loop turn. `SnapshotAssembler` caps a long gap
            // so a skipped read or slept machine does not slingshot.
            let elapsed_ms = u32::try_from(last_tick.elapsed().as_millis()).unwrap_or(u32::MAX);
            last_tick = Instant::now();

            // Hit-test in shared space, not an overlay's: every overlay is
            // handed the same sprite in its own coordinates, so one answer
            // instead of one per window that could disagree.
            let cursor_at = (
                cursor_points.x.round() as i32,
                cursor_points.y.round() as i32,
            );

            // Drop a dismissed Instance before hit-testing: a pointer left
            // behind still counts as a gesture, and a mid-drag dismiss would
            // hold every other character's presses for as long as the button stayed down.
            lives.retain(|live| roster.get(&live.id).is_some());

            // Last tick's art is the one being hit-tested. A Character nobody
            // can see is not there to press, so the click reaches the window underneath.
            let visible = rules.lock().is_ok_and(|rules| rules.presence().visible);

            let pressed: Vec<bool> = lives
                .iter()
                .map(|live| {
                    visible
                        && live.drawn_last.as_ref().is_some_and(|last| {
                            live.character
                                .draw(
                                    last.animation,
                                    last.animation_ms,
                                    last.variant_draw,
                                    last.facing,
                                )
                                .is_some_and(|art| {
                                    art.mask
                                        .hit(&last.rect, cursor_at.0, cursor_at.1, art.mirrored)
                                })
                        })
                })
                .collect();

            // One cursor, several sprites, at most one gesture. Decided
            // across every Instance first: two overlapping sprites would
            // both be picked up by one press.
            let gesturing = lives.iter().position(|live| live.pointer.gesturing());
            let target = press_target(&pressed, gesturing);

            // Last tick's click-through decided whether the overlay could
            // hear this press. Passing through drops a lost pointerup. Must
            // not consult the session poll: that poll misses a swallowed press.
            let on_overlay =
                display_index_for((cursor_points.x, cursor_points.y), &displays.frames);
            if !visible {
                platform::overlay_passes_clicks_through();
            } else if let Some(index) = on_overlay {
                if ignoring.get(index).copied().flatten() == Some(true) {
                    platform::overlay_passes_clicks_through();
                }
            } else {
                platform::overlay_passes_clicks_through();
            }
            // A consuming read — once per tick, nowhere else. A second read
            // eats the edge (#182).
            let buttons = platform::buttons_down();
            let held = buttons.primary;
            let secondary_held = buttons.secondary;
            let button_edge = match (button_was_down, held) {
                (false, true) => Some("down"),
                (true, false) => Some("up"),
                _ => None,
            };
            button_was_down = held;
            if tracing_clicks {
                if let Some(edge) = button_edge {
                    let sprite = target
                        .and_then(|index| lives.get(index))
                        .and_then(|live| live.drawn_last.as_ref())
                        .map(|last| format!("({},{})", last.rect.x, last.rect.y))
                        .or_else(|| {
                            lives.first().and_then(|live| {
                                live.drawn_last.as_ref().map(|last| {
                                    format!("untargeted({},{})", last.rect.x, last.rect.y)
                                })
                            })
                        })
                        .unwrap_or_else(|| "none".to_string());
                    eprintln!(
                        "click: {edge} hits={pressed:?} target={target:?} \
                         cursor=({:.0},{:.0})->({},{}) scale={:.1} \
                         visible={visible} sprite={sprite}",
                        cursor.x, cursor.y, cursor_at.0, cursor_at.1, cursor_scale,
                    );
                }
            }

            // Drain every menu signal this tick: a click and the close that
            // follows arrive together, and one-per-frame would leave the
            // Instance held for a tick after the menu was already gone.
            let mut chosen: Vec<String> = Vec::new();
            let mut menu_closed = false;
            loop {
                match menu_signals.try_recv() {
                    Ok(MenuSignal::Chose(id)) => chosen.push(id),
                    Ok(MenuSignal::Closed) => menu_closed = true,
                    Err(mpsc::TryRecvError::Empty) => break,
                    // Channel gone means nobody can pop a menu. End the hold:
                    // Verb::Menu every tick forever never moves again.
                    Err(mpsc::TryRecvError::Disconnected) => {
                        menu_closed = true;
                        break;
                    }
                }
            }

            // Which Instance a click belongs to is decided by which one's menu
            // carries the id, not by which one the cursor is over: the sprite is
            // free to have walked out from under its own menu.
            let picked: Vec<(InstanceId, menu::MenuAction)> = chosen
                .iter()
                .filter_map(|id| {
                    lives.iter().find_map(|live| {
                        live.menu_hold
                            .as_ref()
                            .and_then(|hold| hold.actions.get(id))
                            .map(|action| (live.id.clone(), action.clone()))
                    })
                })
                .collect();

            for (id, action) in &picked {
                apply_menu_action(
                    action.clone(),
                    &mut roster,
                    &mut lives,
                    &mut slots,
                    id,
                    &rules,
                    &settings,
                    &settings_path,
                    &characters,
                    &mut config,
                    &director,
                    &inspect,
                    &app,
                    cursor_points,
                    &displays.frames,
                    &displays.usable_frames,
                );
            }
            let mut menu_acted = !picked.is_empty();

            // Tray clicks have no menu_hold: the same ids land here, and the
            // first Instance is the one they apply to when nobody's menu is open.
            if picked.is_empty() {
                for id in &chosen {
                    if let Some(action) = tray_actions.get(id).cloned() {
                        let target = lives
                            .first()
                            .map(|live| live.id.clone())
                            .unwrap_or_default();
                        apply_menu_action(
                            action,
                            &mut roster,
                            &mut lives,
                            &mut slots,
                            &target,
                            &rules,
                            &settings,
                            &settings_path,
                            &characters,
                            &mut config,
                            &director,
                            &inspect,
                            &app,
                            cursor_points,
                            &displays.frames,
                            &displays.usable_frames,
                        );
                        menu_acted = true;
                    }
                }
            }
            if menu_acted {
                // Sprite and tray share this persist so a rename survives restart (#375).
                remember_instances(&roster, &settings, &settings_path);
            }

            let mut settings_ops = false;
            let mut reload_chat = false;
            while let Ok(op) = ops.try_recv() {
                settings_ops = true;
                match op {
                    SettingsOp::Spawn { character, name } => {
                        spawn_live(
                            &mut roster,
                            &mut lives,
                            &characters,
                            &character,
                            name,
                            &config,
                            &director,
                        );
                    }
                    SettingsOp::Dismiss { id } => {
                        roster.dismiss(&id);
                        lives.retain(|live| live.id != id);
                        slots.abandon(&id);
                        session_log::forget(&app, &id);
                        close_chat(&app, &id);
                    }
                    SettingsOp::SwitchAll { character } => {
                        if let Some(loaded) = characters.get(&character).cloned() {
                            let ids: Vec<_> = lives.iter().map(|live| live.id.clone()).collect();
                            for id in ids {
                                switch_instance(
                                    &mut roster,
                                    &mut lives,
                                    &mut slots,
                                    &id,
                                    Arc::clone(&loaded),
                                    &config,
                                    &director,
                                    &app,
                                );
                                let (chat_ui, chat_appearance) = settings
                                    .lock()
                                    .ok()
                                    .map(|s| (s.chat_ui.clone(), s.chat_appearance))
                                    .unwrap_or_else(|| {
                                        ("minimal".to_string(), ChatAppearance::System)
                                    });
                                if let Ok(inspect) = inspect.lock() {
                                    push_chat_opening(
                                        &app,
                                        &roster,
                                        &id,
                                        &inspect,
                                        &characters,
                                        &chat_ui,
                                        chat_appearance,
                                    );
                                }
                            }
                        } else {
                            eprintln!("settings: no Character named {character}");
                        }
                    }
                    SettingsOp::Retarget {
                        settings,
                        enabled,
                        proactive_allowed,
                        configured,
                    } => {
                        // What every live `Pace` was built from, and the only
                        // way to tell an edited wake interval from a Retarget
                        // that changed the host and left the interval alone.
                        let was_first = config.ambient_first;
                        // Nothing could answer a moment ago, so the session
                        // starting here is the first one rather than a
                        // replacement. Read before `config` is rebuilt.
                        let first_connection = !config.configured && configured;
                        let lost_configuration = config.configured && !configured;
                        director = settings;
                        config = model::config_from(&director);
                        config.enabled = enabled;
                        config.proactive_allowed = proactive_allowed;
                        config.configured = configured;
                        // If we lost configuration, clear the pending message since
                        // nothing can answer anymore.
                        if lost_configuration {
                            pending_first_connection_message = false;
                        }
                        if let Ok(mut inspect) = inspect.lock() {
                            inspect.enabled = config.enabled;
                            inspect.configured = config.configured;
                            inspect.proactive_wakes = config.proactive_allowed;
                            // A Retarget is how the endpoint moves, so it is
                            // also how the Chat header stops naming the old
                            // one (#474).
                            inspect.model = director.model.clone();
                            inspect.host = model::host_of(&director.base_url);
                        }
                        let interval_moved = config.ambient_first != was_first;
                        for live in &mut lives {
                            // `Pace` took the interval at spawn. Back to
                            // `first` with a rebuilt config: the character on a
                            // two-hour wait is the one whose owner asked for shorter.
                            if interval_moved {
                                live.pace = paced(&config, &live.character);
                            }
                            // Completer target changed, not Character. A Wake
                            // still on the wire would propose against the old
                            // host and session; drop it and open a new turn.
                            completer::retarget_model(
                                &mut slots,
                                &live.id,
                                &mut live.model,
                                live.character.behaviors.keys().cloned(),
                                live.character.name.clone(),
                                &director,
                                configured,
                            );
                            // On first connection, defer the "something can answer now"
                            // message until after the first successful wake. Otherwise
                            // the banner appears even when harness spawn fails moments
                            // later, scrolling the chat away from the connection error.
                            if first_connection {
                                pending_first_connection_message = true;
                            } else {
                                session_log::new_session(
                                    &app,
                                    &live.id,
                                    "settings changed what answers",
                                );
                            }
                        }
                    }
                    SettingsOp::ReloadChat => reload_chat = true,
                    SettingsOp::NewSession => {
                        for live in &mut lives {
                            // Same Completer, same Character, same Blank AI:
                            // the only thing thrown away is the conversation
                            // the Instance is in (#679).
                            replace_session(&mut slots, live, &director, config.configured);
                            session_log::new_session(&app, &live.id, "a new session was started");
                        }
                    }
                    SettingsOp::ChatUIChanged { chat_ui } => {
                        for live in &lives {
                            let _ = app.emit_to(chat_label(&live.id), CHAT_UI_EVENT, &chat_ui);
                        }
                    }
                    SettingsOp::ChatAppearanceChanged { chat_appearance } => {
                        for live in &lives {
                            let _ = app.emit_to(
                                chat_label(&live.id),
                                CHAT_APPEARANCE_EVENT,
                                &chat_appearance,
                            );
                        }
                    }
                }
                remember_instances(&roster, &settings, &settings_path);
            }
            publish_instances(&roster, &instance_rows);
            // After publish so the Settings roster reads the post-switch
            // InstanceRow, not the one from last tick. Menu-driven switches
            // never set `settings_ops`. #375.
            if settings_ops || menu_acted {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    platform::refresh_settings(&handle);
                });
            }

            // A Chat line sets `addressed` and `happened` the way a Poke does.
            // No call starts here: the frame loop can never wait on one.
            while let Ok(msg) = chat.try_recv() {
                let line = match msg {
                    ChatMsg::Said(line) => line,
                    ChatMsg::InboundWake(wake) => {
                        // Inbound wake triggers a Director wake so the speech participates
                        // in Pace and reaches Chat/bubble/Behaviors.
                        if let Some(live) = lives.iter_mut().find(|live| live.id == wake.instance) {
                            live.addressed = true;
                            live.happened = Happened::Proactive;
                            let _ = app.emit_to(
                                chat_label(&wake.instance),
                                CHAT_EVENT,
                                super::ChatReply {
                                    said: Some(wake.speech),
                                    busy: false,
                                    reacting_to: Some("inbound wake".to_string()),
                                    you: false,
                                    thought: false,
                                    at: Some(
                                        SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .map_or(0, |since| since.as_millis() as u64),
                                    ),
                                    error: None,
                                    failure: None,
                                    superseded_by: None,
                                    streaming: false,
                                },
                            );
                        }
                        continue;
                    }
                    ChatMsg::Listening(id) => {
                        if let Some(live) = lives.iter_mut().find(|live| live.id == id) {
                            live.status_last = None;
                        }
                        names_sent = None;
                        continue;
                    }
                    // A saved Instance Prompt. Already inside the bound, which
                    // `chat_prompt` is where a refusal can still be read.
                    ChatMsg::Wrote(written) => {
                        if !roster.set_prompt(&written.instance, written.text.clone()) {
                            eprintln!("chat: no Instance {} to write for", written.instance);
                            continue;
                        }
                        // The id this text is keyed to is persisted with it, or
                        // the next launch mints another and loses it (ADR-0012).
                        remember_instances(&roster, &settings, &settings_path);

                        if let Some(live) =
                            lives.iter_mut().find(|live| live.id == written.instance)
                        {
                            // The Character Prompt is the opening turn; this
                            // session opened without these words. Takes
                            // effect at the next wake, not by re-asking (ADR-0012).
                            replace_session(&mut slots, live, &director, config.configured);
                        }

                        // Same session replacement a Character switch uses:
                        // the held turns go, the open surface is told, and
                        // the Action Log keeps the boundary (#476, ADR-0012).
                        session_log::new_session(
                            &app,
                            &written.instance,
                            "the Instance Prompt changed",
                        );
                        // A prompt layer changing is exactly what a user needs
                        // to find later. `chars` rather than the body, as the
                        // `prompt` event already does (#435).
                        crate::action_log::append(
                            &fidget_core::memory::data_dir(),
                            "instance-prompt",
                            serde_json::json!({
                                "instance": written.instance,
                                "chars": written.text.chars().count(),
                            }),
                        );
                        let (chat_ui, chat_appearance) = settings
                            .lock()
                            .ok()
                            .map(|s| (s.chat_ui.clone(), s.chat_appearance))
                            .unwrap_or_else(|| ("minimal".to_string(), ChatAppearance::System));
                        if let Ok(inspect) = inspect.lock() {
                            push_chat_opening(
                                &app,
                                &roster,
                                &written.instance,
                                &inspect,
                                &characters,
                                &chat_ui,
                                chat_appearance,
                            );
                        }
                        continue;
                    }
                };
                let Some(live) = lives.iter_mut().find(|live| live.id == line.instance) else {
                    // Dismissed between the send and this drain. Answered
                    // anyway, so if this beats the window closing it stops a
                    // caret rather than leaving it spinning.
                    eprintln!("chat: no Instance {} to speak to", line.instance);
                    let _ = app.emit_to(
                        chat_label(&line.instance),
                        CHAT_EVENT,
                        ChatReply {
                            said: None,
                            busy: false,
                            reacting_to: None,
                            you: false,
                            thought: false,
                            at: None,
                            error: None,
                            failure: None,
                            superseded_by: None,
                            streaming: false,
                        },
                    );
                    continue;
                };

                // Answer an unaskable line here rather than park it:
                // `happened` is one slot only a wake clears. Displays
                // asleep is not this case: a line taken first is asked when they wake.
                let askable = config.enabled && live.model.is_some();
                if !askable {
                    let _ = app.emit_to(
                        chat_label(&live.id),
                        CHAT_EVENT,
                        ChatReply {
                            said: None,
                            busy: false,
                            reacting_to: None,
                            you: false,
                            thought: false,
                            at: None,
                            error: None,
                            failure: None,
                            superseded_by: None,
                            streaming: false,
                        },
                    );
                    continue;
                }

                // A second line in the same tick has nowhere to go:
                // `happened` is one slot. Refused rather than overwrite;
                // a wake already on the wire is ADR-0016, not this case.
                if matches!(live.happened, Happened::Chat(_)) {
                    let _ = app.emit_to(
                        chat_label(&line.instance),
                        CHAT_EVENT,
                        ChatReply {
                            said: None,
                            busy: true,
                            reacting_to: None,
                            you: false,
                            thought: false,
                            at: None,
                            error: None,
                            failure: None,
                            superseded_by: None,
                            streaming: false,
                        },
                    );
                    continue;
                }
                let at = SystemTime::now();
                session_log::remember_you(&app, &live.id, &line.text, at);
                // The Chat surface draws a line it typed itself. A quick
                // message is typed on the overlay, so an open window is told
                // or the question only appears the next time Chat opens.
                if line.echo {
                    let at_ms = at
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |since| since.as_millis() as u64);
                    let _ = app.emit_to(
                        chat_label(&live.id),
                        CHAT_EVENT,
                        ChatReply {
                            said: Some(line.text.clone()),
                            busy: false,
                            reacting_to: None,
                            you: true,
                            thought: false,
                            at: Some(at_ms),
                            error: None,
                            failure: None,
                            superseded_by: None,
                            streaming: false,
                        },
                    );
                }
                live.addressed = true;
                live.happened = Happened::Chat(line.text);
            }

            // A `tools/call` from the attached Harness. Here because this is
            // where the `Roster` and `ExpressionHandle` are — the seam #470
            // was missing. A queued proposal reaches the screen on the next tick.
            while let Ok(call) = mcp.try_recv() {
                let excluded = settings
                    .lock()
                    .map(|settings| settings.excluded_applications.clone())
                    .unwrap_or_default();
                answer_tool_call(call, &mut roster, assembler.source(), excluded, &displays);
            }

            {
                let installed: Vec<String> = characters.keys().cloned().collect();
                let current = lives
                    .first()
                    .map(|live| live.character.name.clone())
                    .unwrap_or_default();
                let id = lives
                    .first()
                    .map(|live| live.id.clone())
                    .unwrap_or_default();
                let settings_now = settings.lock().ok().map(|s| s.clone()).unwrap_or_default();
                mcp_resources::publish_excluded(&settings_now.excluded_applications);
                let rules_now = rules.lock().ok();
                let description = describe_menu(
                    &installed,
                    &current,
                    &roster,
                    &id,
                    &settings_now,
                    rules_now.as_deref().unwrap_or(&HideRules::default()),
                );
                let notice = crate::names_hint::live();
                if notice.armed() {
                    let push = notice.publish(&settings_now);
                    if names_sent != Some(push.hint) {
                        names_sent = Some(push.hint);
                        for live in &lives {
                            let _ =
                                app.emit_to(chat_label(&live.id), crate::names_hint::EVENT, push);
                        }
                    }
                }
                if let Some(description) = menu::replace_if_changed(&mut last_menu, description) {
                    tray_actions = description.actions.clone();
                    let handle = app.clone();
                    let generation = Arc::clone(&quit_generation);
                    let _ = app.run_on_main_thread(move || {
                        // Bump on this thread, immediately before set_menu: the
                        // teardown click muda fires is then the previous
                        // generation, so a real Quit cannot land in the hop.
                        let next_quit = generation.fetch_add(1, Ordering::SeqCst) + 1;
                        if let Some(state) = handle.try_state::<TrayHandle>() {
                            if let Ok(guard) = state.0.lock() {
                                if let Some(icon) = guard.as_ref() {
                                    if let Err(why) =
                                        tray::refresh(icon, &handle, &description, next_quit)
                                    {
                                        eprintln!("tray: {why}");
                                    }
                                }
                            }
                        }
                    });
                }
            }

            if let Ok(settings) = settings.lock() {
                config.proactive_allowed = settings.proactive_wakes;
                config.apply_switch(settings.director_enabled);
                let dnd = settings.do_not_disturb;
                // Reread every tick, like the flags above, so a mute in
                // Settings lands on the next frame and not the next launch.
                sound_allowed = settings.sound_allowed();
                if let Ok(mut inspect) = inspect.lock() {
                    inspect.enabled = config.enabled;
                    inspect.proactive_wakes = settings.proactive_wakes;
                }
                drop(settings);
                for (id, _) in roster.list() {
                    if let Some(instance) = roster.get_mut(&id) {
                        instance.set_do_not_disturb(dnd);
                    }
                }
            }

            if reload_chat {
                let (chat_ui, chat_appearance) = settings
                    .lock()
                    .ok()
                    .map(|s| (s.chat_ui.clone(), s.chat_appearance))
                    .unwrap_or_else(|| ("minimal".to_string(), ChatAppearance::System));
                if let Ok(mut inspect) = inspect.lock() {
                    inspect.harness = harness::attached().map(|session| session.inspect());
                    push_chat_openings(
                        &app,
                        &roster,
                        &inspect,
                        &characters,
                        &chat_ui,
                        chat_appearance,
                    );
                }
            }

            if menu_closed {
                for live in lives.iter_mut() {
                    live.menu_hold = None;
                }
            }

            // Asked for here, posted after this tick's frames are emitted.
            // The Menu cue is a one-tick pulse, and `popup_menu_at` shares the
            // main thread the webview needs — asking first starves the pulse (#507).
            let mut pending_menus = Vec::new();

            for (index, live) in lives.iter_mut().enumerate() {
                // Only the Instance the press belongs to is told the cursor is
                // over it. The rest still get time: a pointer that stopped
                // being told would measure the next gesture's velocity over the gap.
                live.verbs = live.pointer.update(
                    target == Some(index),
                    held,
                    secondary_held,
                    cursor_points,
                    elapsed_ms,
                );

                if tracing_clicks && !live.verbs.is_empty() {
                    eprintln!("verbs: {} {:?}", live.id, live.verbs);
                }

                // A menu already open holds the Instance still: Verb::Menu is
                // re-injected every tick. Nothing here waits — blocking would
                // stop every other Instance too.
                if let Some(hold) = live.menu_hold.as_mut() {
                    hold.elapsed += Duration::from_millis(u64::from(elapsed_ms));
                    if hold.elapsed >= MENU_HOLD_TIMEOUT {
                        eprintln!("menu: hold expired without a close, releasing {}", live.id);
                        live.menu_hold = None;
                    } else if !live.verbs.iter().any(|verb| matches!(verb, Verb::Menu)) {
                        live.verbs.push(Verb::Menu);
                    }
                } else if live.verbs.iter().any(|verb| matches!(verb, Verb::Menu)) {
                    // Same names the cache is keyed by. Folder stems (`trump`)
                    // are not Character names (`Trump`); looking up a stem
                    // would silently refuse the switch.
                    let installed: Vec<String> = characters.keys().cloned().collect();

                    // The Engine's own answer rather than a copy of it, so the
                    // checkbox cannot disagree with what the character is doing.
                    let settings_now = settings.lock().ok().map(|s| s.clone()).unwrap_or_default();
                    let rules_now = rules.lock().ok();
                    let description = describe_menu(
                        &installed,
                        &live.character.name,
                        &roster,
                        &live.id,
                        &settings_now,
                        rules_now.as_deref().unwrap_or(&HideRules::default()),
                    );

                    // The overlay the cursor is on, and where in it the cursor
                    // is. A menu is positioned in a window's coordinates, and
                    // there is one window per display.
                    let on_display =
                        display_index_for((cursor_points.x, cursor_points.y), &displays.frames);

                    match on_display.and_then(|index| {
                        displays
                            .frames
                            .get(index)
                            .map(|frame| (overlay_label(index), *frame))
                    }) {
                        Some((label, frame)) => {
                            let frame = drawn_frame(frame, platform::overlay_origin(&label));
                            let at = tauri::LogicalPosition::new(
                                cursor_points.x - frame.x,
                                cursor_points.y - frame.y,
                            );

                            // The description is owned Strings and bools, which
                            // is what lets it cross to the main thread. Posted
                            // after emit, not here: see `pending_menus`.
                            pending_menus.push((live.id.clone(), description, label, at));
                        }
                        None => eprintln!("menu: the cursor is on no known display"),
                    }
                }

                // Grab is on every held tick. Only the first tick of a hold is a
                // pick-up; the rest would otherwise name every dragging wake a Grab.
                let grab_started = live.verbs.iter().any(|verb| matches!(verb, Verb::Grab))
                    && live.last_state != Some(State::Dragged);
                if let Some(what) = touched(&live.verbs, grab_started, live.pointer.poke_settled())
                {
                    note_happened(&mut live.happened, what);
                }

                // #17: opening the surface is all the Shell adds to a Summon;
                // the verb stays on `live` for the Engine. Here because
                // `std::mem::take` empties the vec below.
                if live.verbs.iter().any(|verb| matches!(verb, Verb::Summon)) {
                    let instance = roster.get(&live.id);
                    let title = instance
                        .map(|instance| instance.name.clone())
                        .unwrap_or_else(|| live.character.name.clone());
                    open_chat(
                        &app,
                        &live.id,
                        title,
                        instance.map(|instance| instance.feet()),
                    );
                }
            }

            // The Director's clock is the same elapsed time the Engine is
            // given, so a stalled loop wakes it once, not in a burst. Firing
            // subtracts one interval rather than zeroing (see `SnapshotAssembler`).
            let elapsed = Duration::from_millis(u64::from(elapsed_ms));
            since_sense += elapsed;
            time_since_launch += elapsed;

            // First-run tour: 25s after launch, a Speech bubble on the first
            // Instance teaching the three gestures. Once, and only if the user
            // has not already Summoned and Do Not Disturb is off.
            if !tour_triggered && time_since_launch.as_secs() >= 25 && !lives.is_empty() {
                let should_show_tour = {
                    let settings_guard = settings.lock();
                    settings_guard
                        .as_ref()
                        .is_ok_and(|s| !s.first_run_tour_shown && !s.do_not_disturb)
                };

                if should_show_tour {
                    if let Some(first_live) = lives.first() {
                        let already_opened = app
                            .get_webview_window(&chat_label(&first_live.id))
                            .is_some();

                        if !already_opened {
                            if let Some(instance) = roster.get_mut(&first_live.id) {
                                let tour_message = "Double-click me any time to open Chat.\n\n\
                                     Click once to Poke and get a reaction.\n\n\
                                     Right-click to open the menu.";

                                instance.enqueue(BehaviorProposal {
                                    behavior: String::new(),
                                    dialogue: Some(tour_message.to_string()),
                                });
                            }

                            if let Ok(mut settings_guard) = settings.lock() {
                                settings_guard.first_run_tour_shown = true;
                                drop(settings_guard);
                                if let Err(why) =
                                    super::settings::flush_settings(&settings, &settings_path)
                                {
                                    eprintln!("settings: {why}");
                                }
                            }
                            eprintln!("tour: first-run gesture tour shown as Speech bubble");
                        }
                    }
                }
                tour_triggered = true;
            }

            // One reading of the user for every Instance, taken before any of
            // them is ticked so they all wake against the same desktop. A read
            // per character would be the same two AppKit calls bought N times.
            let sensed = if since_sense >= SENSE_INTERVAL {
                since_sense = since_sense.saturating_sub(SENSE_INTERVAL);
                let mut activity = desktop_sense.read(&activity_source, &SystemClock);
                if let Ok(settings) = settings.lock() {
                    withhold_names(
                        &mut activity,
                        crate::consent::usable(
                            crate::consent::CapabilityId::WindowNames,
                            crate::consent::live(),
                        ),
                        &DenyList {
                            excluded_applications: settings.excluded_applications.clone(),
                            filter_password_fields: true,
                        },
                    );
                }
                last_activity = Some(activity.clone());
                Some(activity)
            } else {
                None
            };
            let displays_asleep = last_activity
                .as_ref()
                .is_some_and(|activity| activity.displays_asleep);

            // Assembled once and handed to every Instance. Asking once per
            // Instance would poll the window server N times for one answer.
            let mut world = assembler.assemble(elapsed_ms, cursor_points, Vec::new());

            // Whole display frames, not the usable ones physics runs in: the
            // reserved strips are the difference between fullscreen and zoomed.
            // Rectangles only; `visibility` has no use for which window it is.
            let rects: Vec<_> = world.windows.iter().map(|window| window.rect).collect();
            let desktop = Desktop {
                fullscreen: fullscreen_displays(&rects, &displays.frames),
            };

            // Visibility before instance ticks so scheduler::mode gets the
            // real answer, not hardcoded true (#183).
            let (presence, hide_in_fullscreen) = rules
                .lock()
                .map(|mut rules| {
                    if let Some(change) = rules.update(&desktop) {
                        eprintln!(
                            "presence: {} over {}ms",
                            if change.visible { "shown" } else { "hidden" },
                            change.fade_ms,
                        );
                    }
                    (rules.presence(), rules.hide_in_fullscreen())
                })
                .unwrap_or((
                    Change {
                        visible,
                        fade_ms: 0,
                    },
                    false,
                ));

            // Teleport, as Bring does: a walk would parade the sprite across the app
            // it is leaving. A held sprite stays in the hand until let go.
            if hide_in_fullscreen && desktop.fullscreen.contains(&true) {
                let widths: Vec<(InstanceId, f64)> = lives
                    .iter()
                    .filter(|live| !live.pointer.grabbing())
                    .map(|live| (live.id.clone(), sprite_width(&live.character)))
                    .collect();
                stand_roster(&mut roster, &widths, |feet, widths| {
                    bring_off_fullscreen(
                        feet,
                        widths,
                        &displays.frames,
                        &displays.usable_frames,
                        &desktop,
                    )
                });
            }

            let mut placed: Vec<Placed> = Vec::with_capacity(lives.len());
            last_sprite_rects.clear();

            // The window list is re-read at the frame rate while any Instance is
            // riding a moving window. One riding character is reason enough: the
            // others cost nothing extra, the read being shared.
            let mut riding = false;

            // Idle means all visible instances are grounded/perched with no
            // behavior playing, asleep, or hidden (#183).
            let mut any_needs_active = false;
            let mut any_moving = false;

            // Whether the cursor is over any Instance's art. Click-through is
            // per overlay, so one sprite under the cursor is enough; the press
            // is then routed to that one Instance.
            let mut over_sprite = false;

            for live in lives.iter_mut() {
                let Some(instance) = roster.get_mut(&live.id) else {
                    continue;
                };

                live.since_wake += elapsed;
                live.since_state += elapsed;
                if !displays_asleep {
                    live.since_proactive += elapsed;
                }

                let mut proposal = None;
                let arrived = match slots.take(&live.id) {
                    Some(completer::Arrived::Answered(answered)) => Some(*answered),
                    // A line with no Behavior, so `talk` plays while it grows.
                    // The reply's own Behavior is applied when it lands. The same
                    // speech updates the Chat row so both surfaces stream together.
                    Some(completer::Arrived::Speech(line)) => {
                        proposal = Some(BehaviorProposal {
                            behavior: String::new(),
                            dialogue: Some(line.clone()),
                        });
                        let _ = app.emit_to(
                            chat_label(&live.id),
                            CHAT_EVENT,
                            super::ChatReply {
                                said: Some(line),
                                busy: false,
                                reacting_to: None,
                                you: false,
                                thought: false,
                                at: None,
                                error: None,
                                failure: None,
                                superseded_by: None,
                                streaming: true,
                            },
                        );
                        None
                    }
                    None => None,
                };
                let applied = arrived.is_some();

                let answering_chat = arrived
                    .as_ref()
                    .is_some_and(|answered| matches!(answered.context.happened, Happened::Chat(_)));

                // The session Director answered, as against a failed call that
                // left `fallback` running on static weights.
                let responded = arrived
                    .as_ref()
                    .is_some_and(|answered| matches!(answered.wake, Wake::Proposed(_)));

                // Read here because the `Context` is consumed just below and
                // the emit that needs it is further down still.
                let reacting_to = arrived
                    .as_ref()
                    .map(|answered| director::reacting_to(&answered.context.happened));

                // Beside the words rather than in them: a mark written into
                // the line would be spoken as the model's own and pushed back
                // into the session as its last turn (#610, `bubble.js`).
                let mut truncated = false;
                let mut parsed_dialogue = None;
                if let Some(completer::Answered {
                    wake,
                    context,
                    near_miss,
                    truncated: cut_off,
                }) = arrived
                {
                    truncated = cut_off;
                    // Here rather than in the worker: this is where core's
                    // parse result first reaches I/O. The Harness logs wakes
                    // that never got a reply, so every line here has a prompt to join.
                    harness::note_parsed(
                        &live.id,
                        &wake,
                        director::reactive(&context.happened),
                        near_miss.as_deref(),
                    );
                    if model::tracing() {
                        match &wake {
                            Wake::Proposed(parsed) if !parsed.behavior.is_empty() => eprintln!(
                                "director: {} parsed {}{}",
                                live.id,
                                parsed.behavior,
                                parsed
                                    .dialogue
                                    .as_deref()
                                    .map(|line| format!(" | {line}"))
                                    .unwrap_or_default(),
                            ),
                            Wake::Proposed(_) => {}
                            Wake::Failed => {
                                eprintln!("director: {} failed; Static fallback", live.id)
                            }
                        }
                    }
                    if let Wake::Proposed(ref parsed) = wake {
                        parsed_dialogue = parsed.dialogue.clone();
                    }
                    proposal = director::fallback(wake, &mut live.director, &context);
                    if model::tracing() {
                        match &proposal {
                            Some(playing) if playing.behavior.is_empty() => {
                                eprintln!(
                                    "director: {} saying {}",
                                    live.id,
                                    playing.dialogue.as_deref().unwrap_or("")
                                );
                            }
                            Some(playing) => eprintln!(
                                "director: {} playing {}{}",
                                live.id,
                                playing.behavior,
                                playing
                                    .dialogue
                                    .as_deref()
                                    .map(|line| format!(" | {line}"))
                                    .unwrap_or_default(),
                            ),
                            None => eprintln!("director: {} nothing to play", live.id),
                        }
                    }
                }

                if let Some(activity) = &sensed {
                    let due = director::due(
                        live.since_wake,
                        config.wake_every,
                        activity,
                        live.previous_idle,
                        live.since_state,
                        instance.do_not_disturb(),
                    );
                    live.previous_idle = activity.idle;

                    if due {
                        live.since_wake = live.since_wake.saturating_sub(config.wake_every);
                        live.since_state = Duration::ZERO;
                        // Static keeps the free life going. A session call in
                        // flight is the one exception: do not stack a weight pick
                        // on a proposal that is about to land.
                        if !slots.waiting(&live.id) && !applied {
                            proposal = live.director.propose(&Context {
                                activity: activity.clone(),
                                recent: live.recent.clone(),
                                personality: live.character.personality.clone(),
                                instance_prompt: instance.prompt().to_string(),
                                state: live.last_state.unwrap_or(State::Grounded),
                                happened: live.happened.clone(),
                                standing: String::new(),
                                front_title: None,
                            });
                        }
                    }
                }

                // The verbs are this Instance's alone, decided above. Taken
                // rather than cloned: the snapshot is reused across Instances,
                // and a verb left behind would be replayed next tick.
                // This Instance only. The snapshot is shared, and a
                // neighbour's caret must not stop this one.
                world.composing =
                    platform::overlay_composing().as_deref() == Some(live.id.as_str());
                world.verbs = std::mem::take(&mut live.verbs);
                world.poke_settled = live.pointer.poke_settled();
                world.proposal = proposal;
                live.qm = platform::qm_draft(&live.id);
                world.locomotion_frozen = walk_held(&live.speech, live.qm.as_ref(), Instant::now());

                let frame = instance.tick(&world);
                riding |= frame.riding;

                // The Engine decides whether the user addressed the character;
                // `touched` above only names what the wake says happened.
                if frame.addressed {
                    live.addressed = true;
                }

                // As well as the Speech bubble, not instead of it, and
                // addressed to one window. Every response, not only typed
                // lines: a bubble-only line is the split ADR-0008 prevents.
                if answering_chat {
                    live.chat_turn = false;
                }
                let unasked = responded && !answering_chat && frame.dialogue.is_some();
                // A failed wake is not a Director with nothing to propose,
                // and the Chat surface said it was (#514). Read here because
                // `note_parsed` already logged the same words.
                let error = (applied && !responded).then(harness::last_error).flatten();
                let failure = (applied && !responded)
                    .then(harness::turn_failure)
                    .flatten();
                if answering_chat || unasked {
                    let reacting_to = unasked.then(|| reacting_to.clone()).flatten();
                    // The mark goes into the remembered line once, here:
                    // the Chat surface draws the record. The bubble keeps
                    // the model's words; the parser has already read them (#610).
                    let dialogue =
                        chat_dialogue(parsed_dialogue.as_deref(), frame.dialogue.as_deref());
                    let remembered = dialogue.map(|line| director::marked(line, truncated));
                    session_log::remember_them(
                        &app,
                        &live.id,
                        remembered.clone(),
                        reacting_to.clone(),
                        SystemTime::now(),
                    );
                    let _ = app.emit_to(
                        chat_label(&live.id),
                        CHAT_EVENT,
                        ChatReply {
                            said: remembered,
                            busy: false,
                            reacting_to,
                            you: false,
                            thought: false,
                            at: None,
                            error,
                            failure,
                            superseded_by: None,
                            streaming: false,
                        },
                    );
                } else if applied {
                    // A wake with no line still thought what its window showed.
                    session_log::remember_thinking(&app, &live.id);
                }

                let became_perched = live.last_state.is_some()
                    && frame.state == State::Perched
                    && live.last_state != Some(State::Perched);
                if became_perched {
                    live.addressed = true;
                    note_happened(&mut live.happened, Happened::Perch);
                }

                if live.last_state != Some(frame.state) {
                    live.last_state = Some(frame.state);
                    live.since_state = Duration::ZERO;
                }
                live.last_position = frame.position;

                // Stay Active while moving, a multi-frame animation still
                // advancing, or idle_ms accruing toward sleep (#183).
                let behavior_playing = frame.playing_behavior.is_some();
                let needs_active_for_motion =
                    scheduler::mode(&frame, presence.visible, behavior_playing)
                        == scheduler::ScheduleMode::Active;

                let needs_active_for_animation =
                    if let Some(character) = characters.get(instance.character_name()) {
                        character
                            .animations
                            .get(frame.animation)
                            .is_some_and(|anim| {
                                anim.looping || {
                                    let current_frame = anim.frame_at(frame.animation_ms);
                                    current_frame + 1 < anim.frames.len()
                                }
                            })
                    } else {
                        false
                    };

                // idle_ms accrues when Grounded/Perched but not Asleep.
                // Keep Active while accruing so sleep-after happens on time.
                let needs_active_for_sleep_accrual =
                    matches!(frame.state, State::Grounded | State::Perched);

                any_moving |= scheduler::moving(&frame);
                if needs_active_for_motion
                    || needs_active_for_animation
                    || needs_active_for_sleep_accrual
                {
                    any_needs_active = true;
                }

                // After the tick so a Throw is already Falling, not still Dragged.
                // Cloned because the wake below resets `live.happened` before
                // the trace at the end of this Instance's turn reads it.
                let happened = live.happened.clone();
                let mut asking = false;
                let reactive_wake = if let (Some(model), Some(activity)) =
                    (&live.model, last_activity.as_ref())
                {
                    if director::session_due(
                        live.addressed,
                        live.since_proactive,
                        &live.pace,
                        activity.displays_asleep,
                        instance.do_not_disturb(),
                        config.proactive_allowed,
                    ) && config.enabled
                    {
                        let context = Context {
                            activity: activity.clone(),
                            recent: live.recent.clone(),
                            // The two authored layers, the package's and
                            // this Instance's own (ADR-0012). Read off the
                            // roster, which is where a save lands.
                            personality: live.character.personality.clone(),
                            instance_prompt: instance.prompt().to_string(),
                            state: frame.state,
                            happened: live.happened.clone(),
                            standing: assembler.standing_on(frame.position),
                            front_title: activity
                                .frontmost_application
                                .as_deref()
                                .and_then(|name| assembler.front_title(name)),
                        };
                        let was_addressed = live.addressed;
                        if live.addressed {
                            live.pace.after_reactive();
                        } else {
                            live.pace.after_ambient();
                        }
                        live.addressed = false;
                        live.happened = Happened::Proactive;
                        live.since_proactive = Duration::ZERO;
                        let line = instance
                            .whereabouts(&displays.frames, &displays.usable_frames)
                            .prompt_line();
                        // The panel shows this copy. The worker appends `line`
                        // after `request`, so an in-flight reply can still open
                        // the session before this wake is sent.
                        let mut payload = model.prompt(&context);
                        if !payload.ends_with('\n') {
                            payload.push('\n');
                        }
                        payload.push_str(&line);
                        // Read before the `Context` is handed to the slot,
                        // and applied only if the slot took the call.
                        let caret = cancelled_caret(live.chat_turn, &context.happened);
                        let chat_turn = matches!(context.happened, Happened::Chat(_));
                        let touched =
                            director::claim(&context.happened) == director::Claim::Interaction;
                        let cell = director::happened_cell(&context.happened);
                        match slots.wake_with_prompt(&live.id, Arc::clone(model), context, line) {
                            // The call on the wire is the truer one
                            // (ADR-0016). This wake is dropped, not queued:
                            // the bookkeeping above has already spent it.
                            completer::Woke::Dropped => false,
                            // A touch the character cannot answer yet points at
                            // the question. A Summon already opens Chat, a
                            // typed line is already in it, and a tick is nobody's.
                            completer::Woke::AwaitingUser => {
                                asking = touched;
                                false
                            }
                            completer::Woke::Started => {
                                // One panel for however many Instances are running,
                                // so the newest call is what it shows. #18 owns the
                                // panel; until then, last payload sent is the honest answer.
                                if let Ok(mut inspect) = inspect.lock() {
                                    inspect.last_payload = Some(payload);
                                    inspect.wake_secs = live.pace.wait().as_secs();
                                }
                                // Starting a call cancels the one before it
                                // (ADR-0016). Tell a typed line on the wire its
                                // caret is cancelled, not that nothing came back (#681),
                                // and name the wake that cancelled it (#890).
                                if let Some(note) = caret {
                                    let _ = app.emit_to(chat_label(&live.id), CHAT_EVENT, note);
                                }
                                live.chat_turn = chat_turn;
                                live.happened_last = Some(cell);
                                // The first successful wake after first connection proves
                                // the completer works. Emit the deferred "something can
                                // answer now" message now that we know it's true.
                                if pending_first_connection_message {
                                    session_log::new_session(
                                        &app,
                                        &live.id,
                                        "something can answer now",
                                    );
                                    pending_first_connection_message = false;
                                }
                                was_addressed
                            }
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if tracing_clicks && (frame.addressed || reactive_wake || !world.verbs.is_empty()) {
                    let skip = if reactive_wake {
                        "started"
                    } else if live.model.is_none() {
                        "no-model"
                    } else if last_activity.is_none() {
                        "no-activity-yet"
                    } else if instance.do_not_disturb() {
                        "dnd"
                    } else if last_activity
                        .as_ref()
                        .is_some_and(|activity| activity.displays_asleep)
                    {
                        "asleep"
                    } else {
                        "not-due"
                    };
                    eprintln!(
                        "session: {} addressed={} happened={happened:?} {skip}",
                        live.id, live.addressed,
                    );
                }

                let thinking = (reactive_wake || slots.thinking(&live.id))
                    && !live
                        .model
                        .as_ref()
                        .is_some_and(|model| model.awaiting_user());

                // What the user has seen is what the Engine played, not what
                // the Director asked for: a refused proposal never reaches
                // the screen, and suppressing it would silence a Behavior nobody watched.
                if let Some(played) = &frame.behavior {
                    director::remember(&mut live.recent, played.clone());
                    if tracing_frames {
                        eprintln!("director: {} {played}", live.id);
                    }
                }

                // A Behavior the State gate refused. Under the Director flag,
                // beside the near-miss line. Names the State, not the reason:
                // the Poke cooldown also refuses, on the sprite's feet (#374).
                if let Some(refused) = &frame.refused {
                    if model::tracing() {
                        eprintln!(
                            "director: {} {refused} refused in {:?}",
                            live.id, frame.state
                        );
                    }
                }

                // On change, not per tick: the loop turns at display rate.
                // Above `draw`, so a Character whose art will not draw still
                // says what its Engine was doing. A dash, not a blank, for Engine-own moments.
                if tracing_engine {
                    let now = Traced {
                        behavior: frame.playing_behavior.clone(),
                        primitive: frame.playing_primitive,
                        animation: frame.animation,
                        state: frame.state,
                    };
                    if live.traced_last.as_ref() != Some(&now) {
                        let at_ms = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |since| since.as_millis());
                        let primitive = match now.primitive {
                            Some(primitive) => format!("{primitive:?}"),
                            None => "-".to_string(),
                        };
                        eprintln!(
                            "engine: {at_ms} behavior({}) primitive({primitive}) animation({}) state({:?}) {}",
                            now.behavior.as_deref().unwrap_or("-"),
                            now.animation,
                            now.state,
                            live.id,
                        );
                        live.traced_last = Some(now);
                    }
                } else {
                    // Forgotten while off, so flipping the switch back on opens
                    // with a line instead of waiting for the next change.
                    live.traced_last = None;
                }

                // Pushed on change for the same reason. `proactive_coming` is
                // `session_due`'s proactive arm, so the bar counts down only
                // to a wake that is coming.
                let proactive_coming = config.enabled
                    && config.proactive_allowed
                    && live.model.is_some()
                    && !instance.do_not_disturb()
                    && !last_activity
                        .as_ref()
                        .is_some_and(|activity| activity.displays_asleep);
                let wake_ms = proactive_coming.then(|| {
                    live.pace
                        .wait()
                        .saturating_sub(live.since_proactive)
                        .as_millis() as u64
                });
                let status = ChatStatus {
                    behavior: frame.playing_behavior.clone(),
                    primitive: frame.playing_primitive,
                    animation: frame.animation,
                    state: frame.state,
                    happened: live.happened_last,
                    facing: frame.facing as i8,
                    thinking,
                };
                // Asks whether the deadline moved — a wake landing, the pace
                // growing under it — not whether it ran down, which it does
                // every tick and which no push has to say.
                let deadline_moved = match (wake_ms, live.status_wake_ms) {
                    (Some(now_ms), Some(was_ms)) => now_ms > was_ms,
                    (now_ms, was_ms) => now_ms.is_some() != was_ms.is_some(),
                };
                if deadline_moved || live.status_last.as_ref() != Some(&status) {
                    let _ = app.emit_to(
                        chat_label(&live.id),
                        CHAT_STATUS_EVENT,
                        ChatStatusPush {
                            status: &status,
                            wake_ms,
                        },
                    );
                    live.status_last = Some(status);
                    live.status_wake_ms = wake_ms;
                }

                // The Engine names an Animation and how long it has been
                // playing. Resolve here, not in the webview, so hit-test and
                // the frame the user sees stay the same one.
                let Some(drawn) = live.character.draw(
                    frame.animation,
                    frame.animation_ms,
                    frame.variant_draw,
                    frame.facing,
                ) else {
                    // No drawable Animation, which a validated package cannot
                    // be. Left out of `placed`; the webview reads absence as
                    // dismissal. Why nothing else in this loop may skip an Instance silently.
                    continue;
                };
                let scale = live.character.scale as i32;
                let (width, height) = (
                    drawn.frame_size.0 as i32 * scale,
                    drawn.frame_size.1 as i32 * scale,
                );

                // Placed once, in the space every display shares. Each overlay
                // is handed it in its own coordinates below.
                let sprite =
                    place_sprite((frame.position.x, frame.position.y), (width, height), scale);

                if tracing_frames {
                    // Unix milliseconds so a prop window and this loop share a
                    // clock. Only when tracing: the loop needs elapsed time,
                    // never the time of day.
                    let at_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |since| since.as_millis());

                    // Instance last so everything before it stands where it
                    // did when one character was all there was.
                    // scripts/verify-overlay.sh matches this prefix.
                    eprintln!(
                        "frame: {} {:?} pos({:.0},{:.0}) sprite({},{}) {}#{} {}",
                        at_ms,
                        frame.state,
                        frame.position.x,
                        frame.position.y,
                        sprite.x,
                        sprite.y,
                        frame.animation,
                        drawn.index,
                        live.id,
                    );
                }

                // Second hit-test, against the sprite about to be drawn.
                // A cursor that has just arrived must not spend a frame
                // passing clicks to the application underneath.
                over_sprite |= drawn
                    .mask
                    .hit(&sprite, cursor_at.0, cursor_at.1, drawn.mirrored);
                live.drawn_last = Some(Drawn {
                    rect: sprite,
                    animation: frame.animation,
                    animation_ms: frame.animation_ms,
                    variant_draw: frame.variant_draw,
                    facing: frame.facing,
                });

                let owner = bubble_owner((frame.position.x, frame.position.y), &displays.frames);
                let dialogue = super::carry_line(
                    &mut live.spoken,
                    frame.dialogue.as_deref(),
                    owner,
                    Instant::now(),
                );
                let overlay_drops_the_bubble = !presence.visible && presence.fade_ms == 0;
                speech::note_speech(
                    &mut live.speech,
                    Instant::now(),
                    dialogue.as_deref(),
                    asking,
                    presence.visible,
                    overlay_drops_the_bubble,
                );

                last_sprite_rects.push((sprite.x, sprite.y, width, height));

                placed.push(Placed {
                    id: live.id.clone(),
                    character: live.character.name.clone(),
                    sprite,
                    width,
                    height,
                    animation: drawn.animation.to_string(),
                    frame_index: drawn.index,
                    mirror: if drawn.mirrored { -1 } else { 1 },
                    dialogue,
                    thinking,
                    asking,
                    chatting: chat_is_up(&app, &live.id),
                    cue: frame.cue,
                    owner,
                    qm: live.qm.clone(),
                    mask: drawn.mask.clone(),
                });
            }

            assembler.poll_fast(riding);

            // Posted: only the main thread may build a window. Record success
            // in the closure, not when queued. Ignore an empty read: a failed
            // desktop read looks the same and tearing down costs two webviews.
            if !displays.frames.is_empty() && *covered.lock().unwrap() != displays.frames {
                let handle = app.clone();
                let frames = displays.frames.clone();
                let placed = Arc::clone(&covered);
                let _ = app.run_on_main_thread(move || match place_overlays(&handle, &frames) {
                    Ok(()) => *placed.lock().unwrap() = frames,
                    Err(why) => eprintln!("overlay: {why}"),
                });
            }

            // Click-through wherever no sprite is drawn, and everywhere while
            // hidden. Exception: a held Character — a drag that outruns the
            // art would drop the sprite in the user's hand.
            let holding = lives.iter().any(|live| live.pointer.grabbing());

            // Second exception: the speech bubble's "Open chat" control (#547),
            // drawn where the mask says no art. `cursor_at` is shared space;
            // the renderer reports overlay coordinates (plus the display origin).
            let over_control = on_overlay.is_some_and(|index| {
                displays.frames.get(index).is_some_and(|display| {
                    let drawn =
                        drawn_frame(*display, platform::overlay_origin(&overlay_label(index)));
                    platform::over_overlay_hotspot(
                        &overlay_label(index),
                        cursor_at.0 - drawn.x.round() as i32,
                        cursor_at.1 - drawn.y.round() as i32,
                    )
                })
            });

            let ignore = !(presence.visible && (over_sprite || over_control || holding));
            let mut flipped = false;

            #[cfg(not(unix))]
            {
                let mut held = std::mem::take(&mut trails);
                for instance in &placed {
                    let at = (instance.sprite.x, instance.sprite.y);
                    let (mask, mirrored, scale) =
                        (&instance.mask, instance.mirror < 0, instance.sprite.scale);
                    let trail = match held.remove(&instance.id) {
                        Some(trail) => trail.advance(at, mask, mirrored, scale),
                        None => DrawTrail::start(at, mask, mirrored, scale),
                    };
                    trails.insert(instance.id.clone(), trail);
                }
            }

            for (index, frame) in displays.frames.iter().enumerate() {
                let label = overlay_label(index);
                let Some(window) = app.get_webview_window(&label) else {
                    continue;
                };
                // Everything below speaks this overlay's own coordinates, so it
                // measures from where the overlay really is, not where it was asked to go.
                let display = &drawn_frame(*frame, platform::overlay_origin(&label));

                // Retried until it succeeds: GTK has no window handle until the
                // widget is realized (shown and ticked), and `window_handle`
                // must run on the GTK main thread.
                #[cfg(not(target_os = "macos"))]
                if !configured
                    .lock()
                    .unwrap()
                    .get(index)
                    .copied()
                    .unwrap_or(false)
                    && !configure_in_flight
                        .lock()
                        .unwrap()
                        .get(index)
                        .copied()
                        .unwrap_or(false)
                {
                    configure_in_flight.lock().unwrap()[index] = true;
                    let handle = app.clone();
                    let label_clone = label.clone();
                    let configured_clone = Arc::clone(&configured);
                    let configure_in_flight_clone = Arc::clone(&configure_in_flight);
                    let overlay_index = index;
                    let trace = tracing;
                    let _ = app.run_on_main_thread(move || {
                        configure_in_flight_clone.lock().unwrap()[overlay_index] = false;
                        if let Some(window) = handle.get_webview_window(&label_clone) {
                            match platform::configure_overlay(&window) {
                                Ok(()) => {
                                    configured_clone.lock().unwrap()[overlay_index] = true;
                                    if trace {
                                        eprintln!("overlay: {label_clone} EWMH configured");
                                    }
                                }
                                Err(e) => {
                                    static LOGGED: std::sync::atomic::AtomicBool =
                                        std::sync::atomic::AtomicBool::new(false);
                                    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                                        eprintln!(
                                            "overlay: {label_clone} EWMH config deferred: {e}"
                                        );
                                    }
                                }
                            }
                        }
                    });
                }

                // Every overlay is told about every Instance, including ones
                // nowhere near: that's what leaves a Character on a seam whole.
                // Addressed, not emitted to all: an untargeted listener would draw the last display's rects.
                let sprites = placed
                    .iter()
                    .map(|instance| SpritePlacement::new(instance, *display, index))
                    .collect();

                let placement = Placement {
                    sprites,
                    visible: presence.visible,
                    fade_ms: presence.fade_ms,
                    sound: sound_allowed,
                };

                // Skip a repeated instruction (#741): Tauri emit is JS eval
                // (CPU), not an interrupt; rAF in src/main.js is the wakeup half.
                // Resent after FRAME_RESEND so a webview that just began listening hears `visible`.
                let next = serde_json::to_string(&placement).ok();
                let repeat = next.as_ref().is_some_and(|next| {
                    last_frame[index]
                        .as_ref()
                        .is_some_and(|(sent, at)| sent == next && at.elapsed() < FRAME_RESEND)
                });
                if !repeat {
                    last_frame[index] = next.map(|next| (next, Instant::now()));
                    let _ = window.emit_to(&label, FRAME_EVENT, placement);
                }

                // Hidden sprites always idle. Track visibility for XI2 wake policy (#183).
                if index == 0 {
                    schedule_mode = if !presence.visible || !any_needs_active {
                        scheduler::ScheduleMode::Idle
                    } else {
                        scheduler::ScheduleMode::Active
                    };
                    moving = any_moving;
                    was_visible = presence.visible;
                }

                // Click-through is per-window. Every other overlay passes
                // clicks through, so a click on one display is never swallowed
                // by a sprite on another.
                let ignore = ignore || on_overlay != Some(index);

                // XShape carves the input region; Tauri must still receive
                // events. Marshal on the GTK main thread. Only set
                // ignore-cursor-events false after the mask applies, or the overlay is a click-eater.
                #[cfg(all(unix, not(target_os = "macos")))]
                {
                    if !ignore && presence.visible {
                        let sprite_on_overlay = placed.iter().find(|instance| {
                            let local = instance.sprite.in_overlay(*display);
                            local.x + instance.width > 0
                                && local.x < display.width as i32
                                && local.y + instance.height > 0
                                && local.y < display.height as i32
                        });

                        if let Some(instance) = sprite_on_overlay {
                            let local = instance.sprite.in_overlay(*display);
                            let (_width, _height, opaque) = instance.mask.raw();
                            // X11's shape clips clicks, not drawing, so drawn-only rects stay out.
                            let hotspots = fidget_core::overlay_region::clickable_rects(
                                &platform::overlay_rects_for(&label),
                            );
                            let mask_params = (
                                Some(opaque.to_vec()),
                                local.x,
                                local.y,
                                i32::from(instance.mirror),
                                instance.sprite.scale,
                                hotspots,
                            );

                            // `last_mask` skips an unchanged sprite. `local.x`
                            // and `local.y` change while walking, so this fires
                            // at motion rate either way; hotspots do not worsen it.
                            if last_mask.lock().unwrap().get(index) != Some(&mask_params)
                                && !mask_in_flight
                                    .lock()
                                    .unwrap()
                                    .get(index)
                                    .copied()
                                    .unwrap_or(false)
                            {
                                mask_in_flight.lock().unwrap()[index] = true;
                                let handle = app.clone();
                                let label_clone = label.clone();
                                let mask_clone = instance.mask.clone();
                                let sprite_x = local.x;
                                let sprite_y = local.y;
                                let sprite_mirror = i32::from(instance.mirror);
                                let sprite_scale = instance.sprite.scale;
                                let hotspots_clone = mask_params.5.clone();
                                let mask_applied_clone = Arc::clone(&mask_applied);
                                let last_mask_clone = Arc::clone(&last_mask);
                                let mask_in_flight_clone = Arc::clone(&mask_in_flight);
                                let mask_params_clone = mask_params.clone();
                                let overlay_index = index;
                                let trace = tracing;

                                let _ = app.run_on_main_thread(move || {
                                    mask_in_flight_clone.lock().unwrap()[overlay_index] = false;
                                    if let Some(window) = handle.get_webview_window(&label_clone) {
                                        match platform::update_input_region(
                                            &window,
                                            Some(&mask_clone),
                                            sprite_x,
                                            sprite_y,
                                            sprite_mirror,
                                            sprite_scale,
                                            &hotspots_clone,
                                            false,
                                        ) {
                                            Ok(()) => {
                                                mask_applied_clone.lock().unwrap()[overlay_index] =
                                                    true;
                                                last_mask_clone.lock().unwrap()[overlay_index] =
                                                    mask_params_clone;
                                                if trace {
                                                    eprintln!(
                                                        "overlay: {label_clone} input mask applied"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                static LOGGED: std::sync::atomic::AtomicBool =
                                                    std::sync::atomic::AtomicBool::new(false);
                                                if !LOGGED
                                                    .swap(true, std::sync::atomic::Ordering::Relaxed)
                                                {
                                                    eprintln!("overlay: {label_clone} update_input_region deferred: {e}");
                                                }
                                            }
                                        }
                                    }
                                });
                            }

                            if mask_applied
                                .lock()
                                .unwrap()
                                .get(index)
                                .copied()
                                .unwrap_or(false)
                                && ignoring[index] != Some(false)
                            {
                                flipped = true;
                                if platform::set_overlay_click_through(&window, false).is_ok() {
                                    ignoring[index] = Some(false);
                                }
                            }
                        } else {
                            let mask_params = (None, 0, 0, 1, 1, Vec::new());

                            if last_mask.lock().unwrap().get(index) != Some(&mask_params) {
                                let handle = app.clone();
                                let label_clone = label.clone();
                                let last_mask_clone = Arc::clone(&last_mask);
                                let mask_params_clone = mask_params.clone();
                                let overlay_index = index;

                                let _ = app.run_on_main_thread(move || {
                                    if let Some(window) = handle.get_webview_window(&label_clone) {
                                        if platform::update_input_region(
                                            &window,
                                            None,
                                            0,
                                            0,
                                            1,
                                            1,
                                            &[],
                                            true,
                                        )
                                        .is_ok()
                                        {
                                            last_mask_clone.lock().unwrap()[overlay_index] =
                                                mask_params_clone;
                                        }
                                    }
                                });
                            }

                            if ignoring[index] != Some(true) {
                                flipped = true;
                                if platform::set_overlay_click_through(&window, true).is_ok() {
                                    ignoring[index] = Some(true);
                                }
                            }
                        }
                    } else {
                        let mask_params = (None, 0, 0, 1, 1, Vec::new());

                        if last_mask.lock().unwrap().get(index) != Some(&mask_params) {
                            let handle = app.clone();
                            let label_clone = label.clone();
                            let last_mask_clone = Arc::clone(&last_mask);
                            let mask_params_clone = mask_params.clone();
                            let overlay_index = index;

                            let _ = app.run_on_main_thread(move || {
                                if let Some(window) = handle.get_webview_window(&label_clone) {
                                    if platform::update_input_region(
                                        &window,
                                        None,
                                        0,
                                        0,
                                        1,
                                        1,
                                        &[],
                                        true,
                                    )
                                    .is_ok()
                                    {
                                        last_mask_clone.lock().unwrap()[overlay_index] =
                                            mask_params_clone;
                                    }
                                }
                            });
                        }

                        if ignoring[index] != Some(true) {
                            flipped = true;
                            if platform::set_overlay_click_through(&window, true).is_ok() {
                                ignoring[index] = Some(true);
                            }
                        }
                    }
                }

                #[cfg(not(unix))]
                {
                    if presence.visible {
                        let sprite_on_overlay = placed.iter().find(|instance| {
                            let local = instance.sprite.in_overlay(*display);
                            local.x + instance.width > 0
                                && local.x < display.width as i32
                                && local.y + instance.height > 0
                                && local.y < display.height as i32
                        });

                        // No sprite here can still leave a bubble straddling the seam.
                        let art = sprite_on_overlay
                            .map(|instance| {
                                let local = instance.sprite.in_overlay(*display);
                                let offset =
                                    (local.x - instance.sprite.x, local.y - instance.sprite.y);
                                trails
                                    .get(&instance.id)
                                    .map(|trail| trail.clip_rects(offset))
                                    .unwrap_or_default()
                            })
                            .unwrap_or_default();
                        let mask_params = (art, platform::overlay_rects_for(&label));

                        let region_action =
                            decide_region_action(&mask_params.0, &mask_params.1, ignore);

                        if region_action == RegionAction::ApplyMask {
                            let action = decide_overlay_action(
                                last_mask.lock().unwrap().get(index),
                                &mask_params,
                                mask_in_flight
                                    .lock()
                                    .unwrap()
                                    .get(index)
                                    .copied()
                                    .unwrap_or(false),
                                toggle_in_flight
                                    .lock()
                                    .unwrap()
                                    .get(index)
                                    .copied()
                                    .unwrap_or(false),
                                applied_ignoring
                                    .lock()
                                    .unwrap()
                                    .get(index)
                                    .copied()
                                    .unwrap_or(None),
                                ignore,
                            );

                            if action == OverlayAction::ApplyMask {
                                mask_in_flight.lock().unwrap()[index] = true;
                                let handle = app.clone();
                                let label_clone = label.clone();
                                let mask_applied_clone = Arc::clone(&mask_applied);
                                let last_mask_clone = Arc::clone(&last_mask);
                                let applied_ignoring_clone = Arc::clone(&applied_ignoring);
                                let mask_in_flight_clone = Arc::clone(&mask_in_flight);
                                let mask_params_clone = mask_params.clone();
                                let overlay_index = index;
                                let trace = tracing;
                                let click_through = ignore;

                                let _ = app.run_on_main_thread(move || {
                                    mask_in_flight_clone.lock().unwrap()[overlay_index] = false;
                                    if let Some(window) = handle.get_webview_window(&label_clone) {
                                        let (art, rects) = &mask_params_clone;
                                        match platform::update_input_region(
                                            &window,
                                            Some(art.as_slice()),
                                            rects,
                                            click_through,
                                        ) {
                                            Ok(passes_clicks) => {
                                                mask_applied_clone.lock().unwrap()[overlay_index] =
                                                    true;
                                                last_mask_clone.lock().unwrap()[overlay_index] =
                                                    mask_params_clone;
                                                applied_ignoring_clone.lock().unwrap()[overlay_index] =
                                                    Some(passes_clicks);
                                                if trace {
                                                    eprintln!(
                                                        "overlay: {label_clone} input mask applied"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                static LOGGED: std::sync::atomic::AtomicBool =
                                                    std::sync::atomic::AtomicBool::new(false);
                                                if !LOGGED
                                                    .swap(true, std::sync::atomic::Ordering::Relaxed)
                                                {
                                                    eprintln!("overlay: {label_clone} update_input_region deferred: {e}");
                                                }
                                            }
                                        }
                                    }
                                });
                            } else if action == OverlayAction::ToggleOnly {
                                toggle_in_flight.lock().unwrap()[index] = true;
                                let handle = app.clone();
                                let label_clone = label.clone();
                                let applied_ignoring_clone = Arc::clone(&applied_ignoring);
                                let toggle_in_flight_clone = Arc::clone(&toggle_in_flight);
                                let overlay_index = index;
                                let trace = tracing;
                                let click_through = ignore;

                                let _ = app.run_on_main_thread(move || {
                                    toggle_in_flight_clone.lock().unwrap()[overlay_index] = false;
                                    if let Some(window) = handle.get_webview_window(&label_clone) {
                                        if platform::toggle_click_through_only(
                                            &window,
                                            click_through,
                                        )
                                        .is_ok()
                                        {
                                            applied_ignoring_clone.lock().unwrap()[overlay_index] =
                                                Some(click_through);
                                            if trace {
                                                eprintln!(
                                                    "overlay: {label_clone} click-through toggled"
                                                );
                                            }
                                        }
                                    }
                                });
                            }

                            let confirmed_ignoring = applied_ignoring
                                .lock()
                                .unwrap()
                                .get(index)
                                .copied()
                                .unwrap_or(None);
                            if confirmed_ignoring.is_some() && ignoring[index] != confirmed_ignoring
                            {
                                flipped = true;
                                ignoring[index] = confirmed_ignoring;
                            }
                        } else if region_action == RegionAction::ToggleClickThrough {
                            // Nothing to draw: pass clicks through, and forget the last region
                            // so whatever comes next rebuilds it.
                            let confirmed_ignoring = applied_ignoring
                                .lock()
                                .unwrap()
                                .get(index)
                                .copied()
                                .unwrap_or(None);

                            if confirmed_ignoring != Some(true)
                                && !toggle_in_flight
                                    .lock()
                                    .unwrap()
                                    .get(index)
                                    .copied()
                                    .unwrap_or(false)
                            {
                                toggle_in_flight.lock().unwrap()[index] = true;
                                let handle = app.clone();
                                let label_clone = label.clone();
                                let applied_ignoring_clone = Arc::clone(&applied_ignoring);
                                let toggle_in_flight_clone = Arc::clone(&toggle_in_flight);
                                let last_mask_clone = Arc::clone(&last_mask);
                                let overlay_index = index;

                                let _ = app.run_on_main_thread(move || {
                                    toggle_in_flight_clone.lock().unwrap()[overlay_index] = false;
                                    if let Some(window) = handle.get_webview_window(&label_clone) {
                                        if platform::toggle_click_through_only(&window, true)
                                            .is_ok()
                                        {
                                            applied_ignoring_clone.lock().unwrap()[overlay_index] =
                                                Some(true);
                                            last_mask_clone.lock().unwrap()[overlay_index] =
                                                (Vec::new(), Vec::new());
                                        }
                                    }
                                });
                            }

                            if confirmed_ignoring == Some(true) && ignoring[index] != Some(true) {
                                flipped = true;
                                ignoring[index] = Some(true);
                            }
                        } else {
                            // Already passing clicks: only catch up with a toggle that landed.
                            let confirmed_ignoring = applied_ignoring
                                .lock()
                                .unwrap()
                                .get(index)
                                .copied()
                                .unwrap_or(None);
                            if confirmed_ignoring == Some(true) && ignoring[index] != Some(true) {
                                flipped = true;
                                ignoring[index] = Some(true);
                            }
                        }
                    } else {
                        let confirmed_ignoring = applied_ignoring
                            .lock()
                            .unwrap()
                            .get(index)
                            .copied()
                            .unwrap_or(None);

                        if confirmed_ignoring != Some(true)
                            && !toggle_in_flight
                                .lock()
                                .unwrap()
                                .get(index)
                                .copied()
                                .unwrap_or(false)
                        {
                            toggle_in_flight.lock().unwrap()[index] = true;
                            let handle = app.clone();
                            let label_clone = label.clone();
                            let applied_ignoring_clone = Arc::clone(&applied_ignoring);
                            let toggle_in_flight_clone = Arc::clone(&toggle_in_flight);
                            let overlay_index = index;

                            let _ = app.run_on_main_thread(move || {
                                toggle_in_flight_clone.lock().unwrap()[overlay_index] = false;
                                if let Some(window) = handle.get_webview_window(&label_clone) {
                                    if platform::toggle_click_through_only(&window, true).is_ok() {
                                        applied_ignoring_clone.lock().unwrap()[overlay_index] =
                                            Some(true);
                                    }
                                }
                            });
                        }

                        if confirmed_ignoring == Some(true) && ignoring[index] != Some(true) {
                            flipped = true;
                            ignoring[index] = Some(true);
                        }
                    }
                }

                #[cfg(target_os = "macos")]
                {
                    // Only record the new state once the platform accepted it.
                    // Recording it regardless would latch a failed toggle
                    // forever, leaving click-through stuck.
                    if ignoring[index] != Some(ignore) {
                        flipped = true;
                        if platform::set_overlay_click_through(&window, ignore).is_ok() {
                            ignoring[index] = Some(ignore);
                        }
                    }
                }
            }

            for (id, description, label, at) in pending_menus {
                let description_actions = description.actions.clone();
                let handle = app.clone();
                let signals = menu_sender.clone();
                let quit_generation = quit_generation.load(Ordering::SeqCst);
                let posted = app.run_on_main_thread(move || {
                    if let Err(why) = menu::show(&handle, &description, &label, at, quit_generation)
                    {
                        eprintln!("menu: {why}");
                    }
                    // Sent whether or not the menu drew, and after it has
                    // closed if the popup is modal. A menu dismissed without
                    // a choice reports nothing anywhere else.
                    let _ = signals.send(MenuSignal::Closed);
                });

                match posted {
                    Ok(()) => {
                        if let Some(live) = lives.iter_mut().find(|live| live.id == id) {
                            live.menu_hold = Some(MenuHold {
                                actions: description_actions,
                                elapsed: Duration::ZERO,
                            });
                        }
                    }
                    // Never held on a menu that was never asked for.
                    Err(why) => {
                        eprintln!("menu: could not reach the main thread: {why}")
                    }
                }
            }

            ticks = ticks.wrapping_add(1);
            if counting_ticks {
                counted_ticks += 1;
                if counted_since.elapsed() >= Duration::from_secs(1) {
                    let at_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |since| since.as_millis());
                    eprintln!("cadence-ticks: {at_ms} {counted_ticks}");
                    counted_ticks = 0;
                    counted_since = Instant::now();
                }
            }
            if tracing && (flipped || ticks.is_multiple_of(120)) {
                eprintln!(
                    "hit-test: cursor({:.0},{:.0}) scale {:.1} -> point({},{}) \
                     on overlay {} {} click-through {}{}",
                    cursor.x,
                    cursor.y,
                    cursor_scale,
                    cursor_at.0,
                    cursor_at.1,
                    on_overlay.map_or(-1, |index| index as i32),
                    if over_sprite { "HIT " } else { "miss" },
                    if ignore { "on" } else { "OFF" },
                    if flipped { "  <- flipped" } else { "" },
                );
            }
        }
    });
}

/// Throw this Instance's conversation away and leave it ready to open a new
/// one on the same Completer. Both halves or neither (ADR-0012, #679). Action
/// Log and Chat stay with the caller: only it knows why the session was replaced.
fn replace_session(
    slots: &mut completer::Slots,
    live: &mut InstanceState,
    director: &model::DirectorSettings,
    configured: bool,
) {
    completer::retarget_model(
        slots,
        &live.id,
        &mut live.model,
        live.character.behaviors.keys().cloned(),
        live.character.name.clone(),
        director,
        configured,
    );
    if let Some(attached) = harness::attached() {
        attached.drop_conversation(&live.id);
    }
}

/// Dispatch one `tools/call` against the live Instances and answer it.
/// The seam #470 was missing: the MCP stub had no roster and no
/// `ExpressionHandle`, so `speak` came back `success: true` and changed nothing.
fn answer_tool_call(
    call: mcp_http::Call,
    roster: &mut Roster,
    source: &dyn WindowSource,
    excluded_applications: Vec<String>,
    displays: &platform::Displays,
) {
    let watched = crate::names_hint::live().watching(source);
    let live: Vec<InstanceInfo> = roster
        .list()
        .into_iter()
        .map(|(id, name)| InstanceInfo { id, name })
        .collect();
    let spots: Vec<FeetAt> = live
        .iter()
        .filter_map(|info| {
            let at = roster.get(&info.id)?.feet();
            Some(FeetAt {
                id: info.id.clone(),
                name: info.name.clone(),
                at,
            })
        })
        .collect();
    let mut context = DispatchContext {
        window_source: &watched,
        memory_path: fidget_core::memory::shared_path(),
        denylist: DenyList {
            excluded_applications,
            filter_password_fields: true,
        },
        roster: &live,
        expression: Some(roster),
        placement: PlacementQuery {
            frames: &displays.frames,
            usable: &displays.usable_frames,
            names: &displays.names,
            instances: &spots,
        },
    };
    let _ = call
        .reply
        .send(dispatch(&call.tool, call.arguments, &mut context));
}

/// What the wake says happened, from one tick of pointer input. Whether the
/// tick addressed the Director at all is `Frame::addressed`.
fn touched(verbs: &[Verb], grab_started: bool, poke_settled: bool) -> Option<Happened> {
    let any = |wanted: fn(&Verb) -> bool| verbs.iter().any(wanted);
    if any(|verb| matches!(verb, Verb::Throw { .. })) {
        Some(Happened::Throw)
    } else if grab_started {
        Some(Happened::Grab)
    } else if poke_settled || any(|verb| matches!(verb, Verb::Menu)) {
        Some(Happened::Poke)
    } else if any(|verb| matches!(verb, Verb::Summon)) {
        Some(Happened::Summon)
    } else {
        None
    }
}

/// Drop front and `before` names without WindowNames consent or when excluded
/// (ADR-0032). Repeated here: these names arrive by a different call than the
/// window walk, and exclusion can only match while the name is still present.
fn withhold_names(activity: &mut Activity, can_read_names: bool, denylist: &DenyList) {
    let denied = activity
        .frontmost_application
        .as_deref()
        .is_some_and(|name| !denylist.allows(name));
    if !can_read_names || denied {
        activity.frontmost_application = None;
    }
    activity
        .before
        .retain(|(name, _)| can_read_names && denylist.allows(name));
}

#[cfg(test)]
mod tests {
    use super::*;
    use fidget_core::character::{
        Animation, Behavior, Character, CursorReaction, Primitive, DEFAULT_MODEL_BASE,
        DEFAULT_MODEL_POWER, REQUIRED_ANIMATIONS,
    };
    use fidget_core::engine::Point;
    use fidget_core::window_source::{Capabilities, WorldGeometry};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn activity(frontmost: Option<&str>) -> Activity {
        Activity {
            frontmost_application: frontmost.map(String::from),
            before: vec![
                ("Safari".to_string(), std::time::Duration::from_secs(60)),
                ("Terminal".to_string(), std::time::Duration::from_secs(120)),
            ],
            weekday: 2,
            hour: 9,
            minute: 30,
            ..Activity::quiet()
        }
    }

    /// The application names are the other half of what ADR-0032 gates, and
    /// they arrive by their own call rather than through the window walk.
    #[test]
    fn application_names_survive_only_consent_and_the_exclusion_list() {
        let allowed = DenyList::default();
        let excluding_terminal = DenyList {
            excluded_applications: vec!["Terminal".to_string()],
            filter_password_fields: true,
        };

        let earlier = |read: &Activity| -> Vec<String> {
            read.before.iter().map(|(name, _)| name.clone()).collect()
        };

        let mut consented = activity(Some("Terminal"));
        withhold_names(&mut consented, true, &allowed);
        assert_eq!(consented.frontmost_application.as_deref(), Some("Terminal"));
        assert_eq!(earlier(&consented), ["Safari", "Terminal"]);

        let mut withheld = activity(Some("Terminal"));
        withhold_names(&mut withheld, false, &allowed);
        assert_eq!(withheld.frontmost_application, None);
        assert!(withheld.before.is_empty(), "no consent, no earlier names");

        let mut excluded = activity(Some("Terminal"));
        withhold_names(&mut excluded, true, &excluding_terminal);
        assert_eq!(excluded.frontmost_application, None);
        assert_eq!(
            earlier(&excluded),
            ["Safari"],
            "excluded wherever it appears"
        );
    }

    /// `FakeWindowSource` is `cfg(test)` inside core, so it is not visible
    /// here. A bare desktop is all this test needs: the sensing tools are
    /// covered against a described desktop in `dispatch`.
    struct EmptyDesktop;

    impl WindowSource for EmptyDesktop {
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }

        fn read(&self) -> WorldGeometry {
            WorldGeometry::default()
        }
    }

    fn character() -> Character {
        let animations = REQUIRED_ANIMATIONS
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    Animation {
                        frames: vec![format!("{name}-0.png")],
                        frame_size: (32, 32),
                        fps: 8,
                        looping: true,
                        variants: Vec::new(),
                        left_strip: None,
                        // core keeps its own default private, and this test
                        // only needs a Behavior something can pick.
                        weight: 10,
                    },
                )
            })
            .collect();
        let mut behaviors = BTreeMap::new();
        behaviors.insert(
            "wave".to_string(),
            Behavior {
                primitives: vec![Primitive::React],
                then: None,
                weight: 1,
                trigger: None,
            },
        );
        Character {
            name: "Buddy".to_string(),
            personality: String::new(),
            animations,
            behaviors,
            art: BTreeMap::new(),
            smooth: false,
            scale: 1,
            model_base: DEFAULT_MODEL_BASE,
            model_power: DEFAULT_MODEL_POWER,
            near_reaction: CursorReaction::default(),
            rush_reaction: CursorReaction::default(),
            source: None,
        }
    }

    /// The wiring #470 was missing: `answer_tool_call` builds context from
    /// the running app's Instances rather than stubs, so a `speak` from a
    /// Harness ends up on the Frame the bubble draws.
    #[test]
    fn a_tool_call_speaks_through_the_live_roster_onto_the_next_frame() {
        let dir = std::env::temp_dir().join(format!("fidget-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let mut roster = Roster::new();
        let id = roster.spawn(&character(), "Pip".to_string(), Point { x: 0.0, y: 0.0 });

        let (reply, answers) = mpsc::channel();
        answer_tool_call(
            mcp_http::Call {
                tool: "speak".to_string(),
                arguments: json!({"message": "hello from a Harness"}),
                reply,
            },
            &mut roster,
            &EmptyDesktop,
            Vec::new(),
            &platform::Displays::default(),
        );
        let result = answers
            .recv()
            .expect("the call is answered")
            .expect("dispatch succeeds");
        assert_eq!(result["success"], json!(true));

        let snapshot =
            SnapshotAssembler::new(EmptyDesktop).assemble(16, Point { x: 0.0, y: 0.0 }, Vec::new());
        let frame = roster.get_mut(&id).expect("still there").tick(&snapshot);
        assert_eq!(
            frame.dialogue.as_deref(),
            Some("hello from a Harness"),
            "the Speech bubble draws Frame::dialogue"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The Harness reads placement from the live Instances and the displays
    /// the frame loop already holds.
    #[test]
    fn a_whereabouts_call_reports_the_live_instance_on_its_display() {
        let mut roster = Roster::new();
        let id = roster.spawn(
            &character(),
            "Pip".to_string(),
            Point {
                x: 2000.0,
                y: 100.0,
            },
        );
        let frame = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let second = Rect {
            x: 1920.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let displays = platform::Displays {
            frames: vec![frame, second],
            usable_frames: vec![frame, second],
            names: vec![Some("Left".to_string()), None],
            ..platform::Displays::default()
        };

        let (reply, answers) = mpsc::channel();
        answer_tool_call(
            mcp_http::Call {
                tool: "whereabouts".to_string(),
                arguments: json!({}),
                reply,
            },
            &mut roster,
            &EmptyDesktop,
            Vec::new(),
            &displays,
        );
        let result = answers
            .recv()
            .expect("the call is answered")
            .expect("dispatch succeeds");

        assert_eq!(result["unit"], json!("points"));
        assert_eq!(result["displays"][0]["name"], json!("Left"));
        assert_eq!(result["displays"][1]["name"], json!(null));
        assert_eq!(result["displays"][1]["origin_x"], json!(1920.0));
        assert_eq!(result["displays"][1]["width"], json!(1920.0));
        assert_eq!(result["displays"][1]["height"], json!(1080.0));
        assert_eq!(result["instances"][0]["id"], json!(id));
        assert_eq!(result["instances"][0]["name"], json!("Pip"));
        assert_eq!(result["instances"][0]["on_display"], json!(1));
        assert_eq!(result["instances"][0]["other_displays"], json!([0]));
        assert_eq!(result["instances"][0]["x"], json!(80.0));
        assert_eq!(result["instances"][0]["y"], json!(100.0));
    }

    /// What the Director hears from a run of left-button states, one a tick.
    fn heard(held: &[bool]) -> Vec<Happened> {
        let mut pointer = fidget_core::input::Pointer::with_double_click_ms(500);
        held.iter()
            .filter_map(|&held| {
                let verbs = pointer.update(true, held, false, Point { x: 0.0, y: 0.0 }, 16);
                touched(&verbs, false, pointer.poke_settled())
            })
            .collect()
    }

    fn click() -> Vec<bool> {
        vec![true, false]
    }

    fn wait(ms: usize) -> Vec<bool> {
        vec![false; ms / 16]
    }

    #[test]
    fn a_double_click_wakes_the_director_with_the_summon_alone() {
        let gesture = [click(), wait(100), click(), wait(1000)].concat();
        assert_eq!(heard(&gesture), vec![Happened::Summon]);
    }

    #[test]
    fn a_lone_click_wakes_the_director_once_the_interval_has_passed() {
        assert_eq!(heard(&[click(), wait(400)].concat()), vec![]);
        assert_eq!(heard(&[click(), wait(600)].concat()), vec![Happened::Poke]);
    }

    #[test]
    fn two_clicks_farther_apart_than_the_interval_are_two_pokes() {
        let gesture = [click(), wait(700), click(), wait(700)].concat();
        assert_eq!(heard(&gesture), vec![Happened::Poke, Happened::Poke]);
    }

    #[test]
    fn test_decide_overlay_action_apply_mask() {
        let old_mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());
        let new_mask = (Some(vec![false]), 20, 20, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&old_mask), &new_mask, false, false, Some(false), false),
            OverlayAction::ApplyMask
        );
    }

    #[test]
    fn test_decide_overlay_action_toggle_only() {
        let mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&mask), &mask, false, false, Some(false), true),
            OverlayAction::ToggleOnly
        );
    }

    #[test]
    fn test_decide_overlay_action_nothing_when_mask_in_flight() {
        let old_mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());
        let new_mask = (Some(vec![false]), 20, 20, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&old_mask), &new_mask, true, false, Some(false), false),
            OverlayAction::Nothing
        );
    }

    #[test]
    fn test_decide_overlay_action_nothing_when_toggle_in_flight() {
        let mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&mask), &mask, false, true, Some(false), true),
            OverlayAction::Nothing
        );
    }

    #[test]
    fn test_decide_overlay_action_nothing_when_all_match() {
        let mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&mask), &mask, false, false, Some(false), false),
            OverlayAction::Nothing
        );
    }

    #[test]
    fn test_decide_overlay_action_retry_after_failed_toggle() {
        let mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&mask), &mask, false, false, Some(false), true),
            OverlayAction::ToggleOnly
        );

        assert_eq!(
            decide_overlay_action(Some(&mask), &mask, false, false, None, true),
            OverlayAction::ToggleOnly
        );
    }

    #[test]
    fn test_decide_overlay_action_in_flight_with_stale_click_through() {
        let old_mask: MaskParams = (Some(vec![true]), 10, 10, 1, 1, Vec::new());
        let new_mask = (Some(vec![false]), 20, 20, 1, 1, Vec::new());

        assert_eq!(
            decide_overlay_action(Some(&old_mask), &new_mask, true, false, Some(false), true),
            OverlayAction::Nothing
        );
    }

    /// Whether there is a region at all is `region_plan`'s call (core table);
    /// this only adds what an empty overlay still has to do about clicks.
    #[test]
    fn an_empty_overlay_passes_clicks_once() {
        let bubble = [fidget_core::overlay_region::OverlayRect {
            rect: [50, 60, 200, 80],
            clickable: false,
        }];
        assert_eq!(
            decide_region_action(&[], &bubble, true),
            RegionAction::ApplyMask,
            "a bubble alone keeps a region, even on a click-through overlay"
        );
        assert_eq!(
            decide_region_action(&[], &[], false),
            RegionAction::ToggleClickThrough
        );
        assert_eq!(
            decide_region_action(&[], &[], true),
            RegionAction::Nothing,
            "already passing clicks"
        );
    }

    #[test]
    fn test_cursor_near_sprite_within_inflated_rect() {
        let sprites = vec![(100, 100, 126, 128)];

        assert!(cursor_near_sprite(Some((100.0, 100.0)), &sprites));
        assert!(cursor_near_sprite(Some((113.0, 114.0)), &sprites));
        assert!(cursor_near_sprite(Some((52.0, 100.0)), &sprites));
        assert!(cursor_near_sprite(Some((274.0, 164.0)), &sprites));
    }

    #[test]
    fn test_cursor_near_sprite_far_away() {
        let sprites = vec![(100, 100, 126, 128)];

        assert!(!cursor_near_sprite(Some((50.0, 100.0)), &sprites));
        assert!(!cursor_near_sprite(Some((280.0, 164.0)), &sprites));
        assert!(!cursor_near_sprite(Some((113.0, 50.0)), &sprites));
        assert!(!cursor_near_sprite(Some((113.0, 280.0)), &sprites));
    }

    #[test]
    fn test_cursor_near_sprite_no_cursor() {
        let sprites = vec![(100, 100, 126, 128)];
        assert!(!cursor_near_sprite(None, &sprites));
    }

    #[test]
    fn test_cursor_near_sprite_multiple_sprites() {
        let sprites = vec![(100, 100, 126, 128), (500, 500, 126, 128)];

        assert!(cursor_near_sprite(Some((113.0, 114.0)), &sprites));
        assert!(cursor_near_sprite(Some((513.0, 514.0)), &sprites));
        assert!(!cursor_near_sprite(Some((300.0, 300.0)), &sprites));
    }

    #[test]
    fn chat_dialogue_prefers_parsed_from_harness() {
        assert_eq!(
            chat_dialogue(Some("harness said this"), Some("engine said this")),
            Some("harness said this"),
            "Chat shows Harness words even when Engine has different dialogue"
        );
    }

    #[test]
    fn chat_dialogue_falls_back_to_engine_frame_when_no_harness_parse() {
        assert_eq!(
            chat_dialogue(None, Some("engine said this")),
            Some("engine said this"),
            "Chat shows Engine dialogue when no Harness parse available"
        );
    }

    #[test]
    fn chat_dialogue_shows_harness_words_even_when_engine_dialogue_absent() {
        assert_eq!(
            chat_dialogue(Some("harness said this"), None),
            Some("harness said this"),
            "Chat shows Harness words even when Engine dialogue is absent"
        );
    }

    #[test]
    fn chat_dialogue_returns_none_when_both_absent() {
        assert_eq!(
            chat_dialogue(None, None),
            None,
            "Chat has no dialogue to show when both sources are None"
        );
    }
}
