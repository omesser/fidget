//! Assembling the `WorldSnapshot` the Engine ticks on: reading the platform and
//! carrying its readings into the Engine's terms once per tick. Separate from the
//! loop in `main.rs` so the two cadences and the conversion test against a fake desktop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::engine::{Point, Verb, Window, WorldSnapshot};
use crate::window_source::{
    WindowSource, WorldGeometry, DOCK_PERCH_ID, POLL_INTERVAL, RIDE_POLL_INTERVAL,
};

/// The longest step of the world the Engine is ever told about. A slept machine
/// hands the loop minutes at once, and five minutes of gravity flings the sprite
/// off the desktop. One poll interval, because that is how old the geometry may be.
const MAX_ELAPSED_MS: u32 = POLL_INTERVAL.as_millis() as u32;

/// Reads the platform at its own cadence and assembles a snapshot per tick.
/// Geometry is read at the idle poll, or at the ride poll when the sprite is
/// holding a moving Perch, and reused on any tick that arrives sooner.
pub struct SnapshotAssembler<S> {
    source: Arc<S>,
    /// The last geometry read, reused until the next read replaces it.
    geometry: WorldGeometry,
    since_poll: Duration,
    /// How many times the window list has actually been read. Carried on the
    /// snapshot so the Engine can tell a reused rectangle from a new sample
    /// and coast a ride between polls.
    poll_generation: u64,
    /// A ride needs the frame rate; sitting and sleeping do not. The Shell
    /// flips this from the last Frame.
    fast: bool,
    side: Option<SidePoll>,
}

struct SidePoll {
    state: Arc<PollState>,
    published: Arc<Mutex<Published>>,
    join: Option<thread::JoinHandle<()>>,
}

/// `interval` is the condvar's mutex, so a ride's `immediate` flag is stored
/// while that mutex is held and cannot land between the check and the wait.
struct PollState {
    interval: Mutex<Duration>,
    /// A ride just started. The idle wait still has most of its interval left,
    /// and finishing it would be one more slow sample before the sprite moves.
    immediate: AtomicBool,
    stop: AtomicBool,
    wake: Condvar,
}

struct Published {
    geometry: WorldGeometry,
    generation: u64,
}

impl<S: WindowSource> SnapshotAssembler<S> {
    /// Starts already due for a read, so the first snapshot ever assembled
    /// describes a real desktop rather than an empty one, which is a world with
    /// no floor and nothing to land on.
    pub fn new(source: S) -> Self {
        Self {
            source: Arc::new(source),
            geometry: WorldGeometry::default(),
            since_poll: POLL_INTERVAL,
            poll_generation: 0,
            fast: false,
            side: None,
        }
    }

    /// The window source itself, for a reader that is not a tick: the sensing
    /// tools answer a Harness on demand, and `list_windows` wants the desktop
    /// now, not the one the last poll saw.
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Read at the ride cadence for as long as the sprite is holding on.
    /// Idle is the default so a sleeping character does not enumerate the
    /// desktop sixty times a second.
    pub fn poll_fast(&mut self, ride: bool) {
        if let Some(side) = &self.side {
            let mut interval = side.state.interval.lock().expect("poll interval");
            *interval = if ride {
                RIDE_POLL_INTERVAL
            } else {
                POLL_INTERVAL
            };
            if ride && !self.fast {
                side.state.immediate.store(true, Ordering::Release);
            }
            drop(interval);
            side.state.wake.notify_one();
            self.fast = ride;
            return;
        }
        if ride && !self.fast {
            // One immediate read, not a burst of whatever the idle clock
            // had left — that remainder is many ride intervals at once.
            self.since_poll = RIDE_POLL_INTERVAL;
        }
        self.fast = ride;
    }

    fn interval(&self) -> Duration {
        if self.fast {
            RIDE_POLL_INTERVAL
        } else {
            POLL_INTERVAL
        }
    }

    /// One tick's snapshot: `elapsed_ms` since the previous tick, and the
    /// cursor in the Engine's coordinate space.
    pub fn assemble(&mut self, elapsed_ms: u32, cursor: Point, verbs: Vec<Verb>) -> WorldSnapshot {
        let elapsed_ms = elapsed_ms.min(MAX_ELAPSED_MS);
        if let Some(side) = &self.side {
            let published = {
                let slot = side.published.lock().expect("published geometry");
                (slot.generation != self.poll_generation)
                    .then(|| (slot.generation, slot.geometry.clone()))
            };
            if let Some((generation, geometry)) = published {
                self.geometry = geometry;
                self.poll_generation = generation;
            }
            return world_snapshot(
                &self.geometry,
                cursor,
                elapsed_ms,
                verbs,
                self.poll_generation,
            );
        }
        let interval = self.interval();

        // The due check comes before the tick's own time is added, and a read
        // takes one interval off the clock rather than zeroing it: zeroing throws
        // away the overshoot and makes every read late by that remainder.
        if self.since_poll >= interval {
            self.since_poll = self.since_poll.saturating_sub(interval);
            self.geometry = self.source.snapshot();
            self.poll_generation = self.poll_generation.saturating_add(1);
        }
        self.since_poll += Duration::from_millis(u64::from(elapsed_ms));

        world_snapshot(
            &self.geometry,
            cursor,
            elapsed_ms,
            verbs,
            self.poll_generation,
        )
    }

    /// Poll on a side thread so a slow window-list read cannot spend the tick.
    /// The tick copies the last finished sample and coasts; the first read is
    /// synchronous, so that sample has a floor. One read at a time.
    pub fn detach_poll(mut self) -> Self
    where
        S: Send + Sync + 'static,
    {
        if self.side.is_some() {
            return self;
        }
        let woke = Instant::now();
        let geometry = self.source.snapshot();
        let now = Instant::now();
        let deadline = next_poll_at(POLL_INTERVAL, woke, woke, now);
        self.geometry = geometry.clone();
        self.poll_generation = 1;

        let published = Arc::new(Mutex::new(Published {
            geometry,
            generation: 1,
        }));
        let state = Arc::new(PollState {
            interval: Mutex::new(POLL_INTERVAL),
            immediate: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            wake: Condvar::new(),
        });
        let source = Arc::clone(&self.source);
        let published_thread = Arc::clone(&published);
        let state_thread = Arc::clone(&state);
        let join = thread::Builder::new()
            .name("window-poll".into())
            .spawn(move || {
                poll_beside_the_tick(source, state_thread, published_thread, deadline);
            })
            .expect("window-poll thread");
        self.side = Some(SidePoll {
            state,
            published,
            join: Some(join),
        });
        self
    }

    /// What the sprite's feet are on, for the Director. Titles need Screen
    /// Recording; the owner name is what the window server gives for free.
    pub fn standing_on(&self, feet: Point) -> String {
        describe_standing(feet, &self.geometry)
    }

    /// The title of `application`'s frontmost titled window, for the Director.
    pub fn front_title(&self, application: &str) -> Option<String> {
        front_title(application, &self.geometry)
    }
}

/// Windows run frontmost first, so the first titled window `application` owns
/// is the one in front. Without the window-names consent no window has an
/// owner to match, so this is None by construction.
fn front_title(application: &str, geometry: &WorldGeometry) -> Option<String> {
    geometry
        .windows
        .iter()
        .filter(|window| window.owner.as_deref() == Some(application))
        .find_map(|window| window.title.clone().filter(|title| !title.is_empty()))
}

impl<S> Drop for SnapshotAssembler<S> {
    fn drop(&mut self) {
        let Some(side) = self.side.take() else {
            return;
        };
        {
            let _interval = side.state.interval.lock().expect("poll interval");
            side.state.stop.store(true, Ordering::Release);
        }
        side.state.wake.notify_one();
        if let Some(join) = side.join {
            let _ = join.join();
        }
    }
}

/// Counts as a moving tick, so a late return shortens the next wait.
fn next_poll_at(interval: Duration, deadline: Instant, woke: Instant, now: Instant) -> Instant {
    crate::scheduler::next_tick(interval, deadline, woke, now, true)
}

fn poll_beside_the_tick<S: WindowSource>(
    source: Arc<S>,
    state: Arc<PollState>,
    published: Arc<Mutex<Published>>,
    mut deadline: Instant,
) {
    loop {
        let forced = {
            let mut interval = state.interval.lock().expect("poll interval");
            loop {
                if state.stop.load(Ordering::Acquire) {
                    return;
                }
                if state.immediate.swap(false, Ordering::AcqRel) {
                    break true;
                }
                let now = Instant::now();
                if now >= deadline {
                    break false;
                }
                let remaining = deadline - now;
                // A kernel sleep this short returns about 48 ms late on the
                // Actions macOS runner, past one tick, so the deadline stays stale.
                // Only this slack spins. An idle wait still sleeps.
                if remaining <= RIDE_POLL_INTERVAL {
                    drop(interval);
                    spin_until(deadline, &state);
                    interval = state.interval.lock().expect("poll interval");
                    continue;
                }
                let (next, result) = state
                    .wake
                    .wait_timeout(interval, remaining)
                    .expect("poll interval");
                interval = next;
                if result.timed_out() {
                    break false;
                }
            }
        };
        // A ride just started. The idle deadline is still in the future;
        // pacing the next wait from it would skip the ride.
        let woke = Instant::now();
        if forced {
            deadline = woke;
        }
        let geometry = source.snapshot();
        {
            let mut slot = published.lock().expect("published geometry");
            slot.generation = slot.generation.saturating_add(1);
            slot.geometry = geometry;
        }
        let now = Instant::now();
        let interval = *state.interval.lock().expect("poll interval");
        deadline = next_poll_at(interval, deadline, woke, now);
    }
}

/// Burn `deadline` on this thread. `spin_loop` pauses without a kernel timer,
/// so a few milliseconds stay a few milliseconds.
fn spin_until(deadline: Instant, state: &PollState) {
    while Instant::now() < deadline {
        if state.stop.load(Ordering::Acquire) || state.immediate.load(Ordering::Acquire) {
            return;
        }
        std::hint::spin_loop();
    }
}

/// Name the surface under `feet`. A Perch is a window (owner, not title); the
/// usable floor is the Dock's top, or, when the Dock's true bounds are known, the
/// Dock is its own surface and the floor runs beside it; a display's side is an edge.
pub fn describe_standing(feet: Point, geometry: &WorldGeometry) -> String {
    if let Some(dock) = &geometry.dock {
        if feet.y == dock.y && feet.x >= dock.x && feet.x <= dock.x + dock.width {
            return "the top of the Dock".to_string();
        }
    }
    for window in geometry
        .windows
        .iter()
        .filter(|window| perch_eligible(window.layer))
    {
        let bounds = &window.bounds;
        if feet.y == bounds.y && feet.x >= bounds.x && feet.x <= bounds.x + bounds.width {
            let owner = window.owner.as_deref().map(str::trim).unwrap_or_default();
            return if owner.is_empty() {
                "a window".to_string()
            } else {
                format!("a {owner} window")
            };
        }
    }
    for display in &geometry.usable_frames {
        let right = display.x + display.width;
        let bottom = display.y + display.height;
        if feet.y == bottom && feet.x >= display.x && feet.x <= right {
            // With the Dock's bounds known the floor reaches the display's
            // own bottom edge, so "above the Dock" would name the wrong
            // side — and only on the display that actually holds the Dock.
            return match &geometry.dock {
                Some(dock) if crate::window_source::centered_in(dock, *display) => {
                    "the display floor, beside the Dock".to_string()
                }
                Some(_) => "the display floor".to_string(),
                None => "the display floor, above the Dock".to_string(),
            };
        }
        if feet.y >= display.y && feet.y <= bottom && (feet.x == display.x || feet.x == right) {
            return "the screen edge".to_string();
        }
    }
    "nothing".to_string()
}

/// The Engine's view of one moment. Both sides already speak points with y
/// growing downward, so this changes type without changing space; windows keep
/// their arrival order, which carries z-order. Only Perch-eligible windows go over.
fn world_snapshot(
    geometry: &WorldGeometry,
    cursor: Point,
    elapsed_ms: u32,
    verbs: Vec<Verb>,
    poll_generation: u64,
) -> WorldSnapshot {
    let mut windows: Vec<Window> = geometry
        .windows
        .iter()
        .filter(|w| perch_eligible(w.layer))
        .map(|w| Window {
            id: w.id,
            rect: w.bounds,
        })
        .collect();
    if let Some(dock) = &geometry.dock {
        // Frontmost, because the Dock draws above every application window:
        // a fall over the Dock lands on the Dock, never on a window hiding
        // underneath it.
        windows.insert(
            0,
            Window {
                id: DOCK_PERCH_ID,
                rect: *dock,
            },
        );
    }
    WorldSnapshot {
        displays: geometry.usable_frames.clone(),
        windows,
        cursor,
        elapsed_ms,
        verbs,
        poll_generation,
        // The frame loop writes `proposal` after assemble().
        ..WorldSnapshot::default()
    }
}

/// Whether a window at this level is somewhere the sprite may stand: only the
/// ordinary application level. Decided here rather than in the Engine because a
/// window level is a platform concept, and the Engine is handed a world of Perches.
///
/// Above 0 is furniture (menu bar 24, status items 25, Dock 20 reporting the whole
/// display) a sprite would land on and never leave, and our own overlay at 3, which
/// this alone keeps out of the world. Below 0 is the desktop picture and notifications.
fn perch_eligible(layer: i32) -> bool {
    layer == 0
}

/// Where the sprite comes into the world: the middle of the first display. The
/// middle rather than the top edge because the art hangs above the feet, so a
/// drop from the very top falls its own height before any of it is on screen.
pub fn starting_position(geometry: &WorldGeometry) -> Point {
    geometry
        .usable_frames
        .first()
        .map_or(Point::default(), |display| Point {
            x: display.x + display.width / 2.0,
            y: display.y + display.height / 2.0,
        })
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;
    use std::time::Instant;

    use super::*;
    use crate::engine::{Engine, State, Window};
    use crate::window_source::{Capabilities, FakeWindowSource, Rect, WindowId, WindowRect};

    fn rect(x: f64, y: f64, width: f64, height: f64) -> crate::window_source::Rect {
        crate::window_source::Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn window(id: WindowId, owner: &str, bounds: crate::window_source::Rect) -> WindowRect {
        WindowRect {
            id,
            bounds,
            owner: Some(owner.to_string()),
            title: None,
            layer: 0,
        }
    }

    /// A window above the ordinary application level, at the layers a real macOS
    /// desktop reports: 3 a floating panel (our overlay), 20 the Dock, 24 the menu
    /// bar, 25 the status items, and a large negative one Notification Centre.
    fn elevated(
        id: WindowId,
        owner: &str,
        bounds: crate::window_source::Rect,
        layer: i32,
    ) -> WindowRect {
        WindowRect {
            id,
            bounds,
            owner: Some(owner.to_string()),
            title: None,
            layer,
        }
    }

    fn seeing_everything() -> Capabilities {
        Capabilities {
            window_geometry: true,
            absolute_positioning: true,
        }
    }

    /// A desktop that changes between reads: each read hands back the next
    /// geometry in the list, and the last one repeats for ever, so a test tells
    /// one read from the next by what came back.
    struct ChangingDesktop(RefCell<Vec<WorldGeometry>>);

    impl ChangingDesktop {
        fn of_display_widths(widths: &[f64]) -> Self {
            Self(RefCell::new(
                widths
                    .iter()
                    .map(|&width| WorldGeometry {
                        usable_frames: vec![rect(0.0, 0.0, width, 800.0)],
                        windows: Vec::new(),
                        dock: None,
                    })
                    .collect(),
            ))
        }
    }

    /// A desktop that reports how often it has been read: its one display is
    /// as wide as the number of reads so far, so a test counts reads by looking
    /// at the snapshot rather than at the fake.
    #[derive(Default)]
    struct CountingDesktop(Cell<f64>);

    impl WindowSource for CountingDesktop {
        fn capabilities(&self) -> Capabilities {
            seeing_everything()
        }

        fn read(&self) -> WorldGeometry {
            self.0.set(self.0.get() + 1.0);
            WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, self.0.get(), 800.0)],
                windows: Vec::new(),
                dock: None,
            }
        }
    }

    impl WindowSource for ChangingDesktop {
        fn capabilities(&self) -> Capabilities {
            seeing_everything()
        }

        fn read(&self) -> WorldGeometry {
            let mut remaining = self.0.borrow_mut();
            let geometry = remaining.first().cloned().unwrap_or_default();
            if remaining.len() > 1 {
                remaining.remove(0);
            }
            geometry
        }
    }

    #[test]
    fn a_snapshot_carries_the_platforms_displays_windows_and_cursor_to_the_engine() {
        let source = FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, 1920.0, 1080.0)],
                windows: vec![
                    window(1, "Terminal", rect(10.0, 20.0, 800.0, 600.0)),
                    window(2, "Finder", rect(30.0, 40.0, 500.0, 400.0)),
                ],
                dock: None,
            },
        };

        let snapshot =
            SnapshotAssembler::new(source).assemble(16, Point { x: 7.0, y: 9.0 }, Vec::new());

        assert_eq!(
            snapshot.displays,
            vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0
            }]
        );
        assert_eq!(
            snapshot.windows,
            vec![
                Window {
                    id: 1,
                    rect: Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 800.0,
                        height: 600.0
                    }
                },
                Window {
                    id: 2,
                    rect: Rect {
                        x: 30.0,
                        y: 40.0,
                        width: 500.0,
                        height: 400.0
                    }
                },
            ],
            "frontmost first, with the window server's own ids, in the order \
             the platform reported"
        );
        assert_eq!(snapshot.cursor, Point { x: 7.0, y: 9.0 });
        assert_eq!(snapshot.elapsed_ms, 16);
    }

    #[test]
    fn the_platform_is_read_at_the_poll_interval_and_reused_on_the_ticks_between() {
        // Three desktops, told apart by the width of their one display.
        let mut assembler = SnapshotAssembler::new(ChangingDesktop::of_display_widths(&[
            1000.0, 2000.0, 3000.0,
        ]));

        // 20ms ticks against a 100ms idle poll: five ticks per read.
        let widths: Vec<f64> = (0..15)
            .map(|_| {
                assembler
                    .assemble(20, Point::default(), Vec::new())
                    .displays[0]
                    .width
            })
            .collect();

        assert_eq!(
            widths,
            vec![
                1000.0, 1000.0, 1000.0, 1000.0, 1000.0, 2000.0, 2000.0, 2000.0, 2000.0, 2000.0,
                3000.0, 3000.0, 3000.0, 3000.0, 3000.0
            ],
            "the Engine ticks faster than the desktop is read"
        );

        let mut generations = SnapshotAssembler::new(ChangingDesktop::of_display_widths(&[
            1000.0, 2000.0, 3000.0,
        ]));
        let gens: Vec<u64> = (0..15)
            .map(|_| {
                generations
                    .assemble(20, Point::default(), Vec::new())
                    .poll_generation
            })
            .collect();
        assert_eq!(
            gens,
            vec![1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3],
            "a reused generation is a tick between polls"
        );
    }

    /// A ride is the only time the window list is worth reading at the frame
    /// rate. Sitting and sleeping stay on the idle poll.
    #[test]
    fn a_ride_reads_the_desktop_at_the_frame_rate() {
        let mut assembler = SnapshotAssembler::new(ChangingDesktop::of_display_widths(&[
            1000.0, 2000.0, 3000.0,
        ]));
        assembler.poll_fast(true);

        // 8ms ticks against a 16ms ride poll: two ticks per read.
        let widths: Vec<f64> = (0..6)
            .map(|_| assembler.assemble(8, Point::default(), Vec::new()).displays[0].width)
            .collect();
        assert_eq!(
            widths,
            vec![1000.0, 1000.0, 2000.0, 2000.0, 3000.0, 3000.0],
            "a ride polls as often as the Engine ticks"
        );
    }

    /// A tick's own work counts against its period. Sleeping a whole tick after
    /// 6ms of work is a 22ms period, a 45 Hz ride.
    #[test]
    fn a_ride_polls_sixty_times_a_second_though_each_tick_works_6ms() {
        let tick = Duration::from_millis(16);
        let work = Duration::from_millis(6);
        let mut assembler = SnapshotAssembler::new(ChangingDesktop::of_display_widths(&[1000.0]));
        assembler.poll_fast(true);

        let start = std::time::Instant::now();
        let (mut woke, mut deadline, mut last_tick) = (start, start, start);
        let mut polls = 0;
        loop {
            deadline = crate::scheduler::next_tick(tick, deadline, woke, woke + work, true);
            woke = deadline;
            if woke - start >= Duration::from_secs(1) {
                break;
            }
            let elapsed_ms = u32::try_from((woke - last_tick).as_millis()).unwrap();
            last_tick = woke;
            polls = assembler
                .assemble(elapsed_ms, Point::default(), Vec::new())
                .poll_generation;
        }
        assert_eq!(polls, 62, "one poll per 16ms tick");
    }

    #[test]
    fn a_seven_millisecond_poll_stays_on_the_frame_cadence_when_sleep_runs_late() {
        let polls = paced_polls(Duration::from_millis(7));
        assert!(
            (60..=65).contains(&polls),
            "ride polls in one second: {polls}"
        );
        let stretched = remainder_sleep_polls(Duration::from_millis(7));
        assert!(
            stretched < 58,
            "sleeping the remainder and keeping the lateness is the 55 Hz ride, got {stretched}"
        );
    }

    #[test]
    fn a_riding_tick_does_not_wait_on_the_window_poll() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hold = Arc::new(Hold {
            open: Mutex::new(false),
            cv: Condvar::new(),
        });
        let source = TimedDesktop {
            calls: Arc::clone(&calls),
            stamps: Arc::new(Mutex::new(Vec::new())),
            delay: Duration::ZERO,
            hold: Some(Arc::clone(&hold)),
            block_from: 1,
        };
        let mut assembler = SnapshotAssembler::new(source).detach_poll();
        let release = Release(hold);
        assembler.poll_fast(true);
        assert!(
            wait_until(|| calls.load(Ordering::SeqCst) >= 2, Duration::from_secs(1)),
            "the ride did not start a second read"
        );
        let tick = Instant::now();
        let snapshot = assembler.assemble(16, Point::default(), Vec::new());
        assert!(
            tick.elapsed() < Duration::from_millis(40),
            "the tick waited on the poll: {:?}",
            tick.elapsed()
        );
        assert_eq!(
            snapshot.poll_generation, 1,
            "an in-flight read is not a new sample"
        );
        for _ in 0..5 {
            assert_eq!(
                assembler
                    .assemble(16, Point::default(), Vec::new())
                    .poll_generation,
                1
            );
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a read still running is not started twice"
        );
        release.open();
        assert!(
            wait_until(
                || {
                    assembler
                        .assemble(16, Point::default(), Vec::new())
                        .poll_generation
                        >= 2
                },
                Duration::from_secs(1)
            ),
            "the finished read never reached the tick"
        );
    }

    #[test]
    fn a_ride_reads_at_once_instead_of_finishing_the_idle_wait() {
        let calls = Arc::new(AtomicUsize::new(0));
        let stamps = Arc::new(Mutex::new(Vec::new()));
        let mut assembler = SnapshotAssembler::new(TimedDesktop::counting(
            Arc::clone(&calls),
            Arc::clone(&stamps),
        ))
        .detach_poll();
        let asked = Instant::now();
        assembler.poll_fast(true);
        assert!(
            wait_until(
                || calls.load(Ordering::SeqCst) >= 2,
                Duration::from_millis(80)
            ),
            "the second read did not start within 80 ms of the ride"
        );
        let stamps = stamps.lock().expect("stamps");
        assert!(
            stamps[1].saturating_duration_since(asked) < Duration::from_millis(80),
            "second read lagged the ride by {:?}",
            stamps[1].saturating_duration_since(asked)
        );
    }

    /// The 6 ms is a spin, not `thread::sleep`. On the Actions macOS runner a
    /// short sleep returns about 48 ms late. Sleeping a whole extra interval
    /// after the read leaves a 22 ms gap on a precise clock.
    #[test]
    fn a_slow_ride_poll_still_starts_once_per_frame() {
        let stamps = Arc::new(Mutex::new(Vec::new()));
        let mut assembler = SnapshotAssembler::new(TimedDesktop {
            calls: Arc::new(AtomicUsize::new(0)),
            stamps: Arc::clone(&stamps),
            delay: Duration::from_millis(6),
            hold: None,
            block_from: usize::MAX,
        })
        .detach_poll();
        assembler.poll_fast(true);
        assert!(
            wait_until(
                || stamps.lock().expect("stamps").len() >= 12,
                Duration::from_secs(2)
            ),
            "the ride poll did not keep its cadence"
        );
        let median = median_gap(&stamps.lock().expect("stamps"));
        assert!(
            median >= Duration::from_millis(12) && median <= Duration::from_millis(19),
            "median gap between poll starts {median:?}"
        );
    }

    #[test]
    fn a_detached_poll_keeps_the_idle_interval_until_a_ride() {
        let stamps = Arc::new(Mutex::new(Vec::new()));
        let _assembler = SnapshotAssembler::new(TimedDesktop::counting(
            Arc::new(AtomicUsize::new(0)),
            Arc::clone(&stamps),
        ))
        .detach_poll();
        assert!(
            wait_until(
                || stamps.lock().expect("stamps").len() >= 4,
                Duration::from_secs(2)
            ),
            "the idle poll did not repeat"
        );
        // The Actions macOS runner wakes this 100 ms sleep up to 150 ms late, and
        // the next gap catches up short (229, 249 and 74 ms medians seen). So the
        // bound only separates idle from the 16 ms ride; the ride test pins the
        // deadline arithmetic.
        let median = median_gap(&stamps.lock().expect("stamps"));
        assert!(
            median >= Duration::from_millis(40),
            "median idle gap {median:?} is ride cadence"
        );
    }

    /// The desktop furniture is not somewhere to stand. Layers, bounds and
    /// owners here are copied from what a real macOS desktop reports.
    #[test]
    fn only_ordinary_application_windows_reach_the_engine() {
        let source = FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, 1920.0, 1080.0)],
                windows: vec![
                    elevated(1, "Control Center", rect(1264.0, 0.0, 39.0, 30.0), 25),
                    elevated(2, "Window Server", rect(0.0, 0.0, 1920.0, 30.0), 24),
                    elevated(3, "Dock", rect(0.0, 0.0, 1920.0, 1080.0), 20),
                    window(4, "Terminal", rect(0.0, 30.0, 1920.0, 952.0)),
                    elevated(
                        5,
                        "Notification Center",
                        rect(8.0, 38.0, 180.0, 180.0),
                        -2_147_483_601,
                    ),
                ],
                dock: None,
            },
        };

        let snapshot = SnapshotAssembler::new(source).assemble(16, Point::default(), Vec::new());

        assert_eq!(
            snapshot.windows,
            vec![Window {
                id: 4,
                rect: Rect {
                    x: 0.0,
                    y: 30.0,
                    width: 1920.0,
                    height: 952.0
                }
            }],
            "the menu bar, the Dock and the status items are not Perches"
        );
    }

    /// A window of ours is a window. Every Instance has a Chat surface, and
    /// nothing from here up knows or cares whose window it is.
    #[test]
    fn a_window_of_ours_that_is_not_the_overlay_is_a_perch_like_any_other() {
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, 1920.0, 1080.0)],
                windows: vec![window(7, "fidget", rect(700.0, 400.0, 420.0, 560.0))],
                dock: None,
            },
        });

        let snapshot = assembler.assemble(16, Point::default(), Vec::new());

        assert_eq!(
            snapshot.windows,
            vec![Window {
                id: 7,
                rect: rect(700.0, 400.0, 420.0, 560.0)
            }],
            "the Chat surface is somewhere to land"
        );
        assert_eq!(
            assembler.standing_on(Point { x: 900.0, y: 400.0 }),
            "a fidget window",
            "and somewhere the Director is told the character is standing"
        );
    }

    /// The overlay is the one window of ours the world must never see: it
    /// covers the display, so a sprite that could find a Perch on it would
    /// find one under its own feet and never fall again.
    #[test]
    fn our_own_overlay_is_never_a_perch() {
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, 1920.0, 1080.0)],
                windows: vec![elevated(1, "fidget", rect(0.0, 0.0, 1920.0, 1080.0), 3)],
                dock: None,
            },
        });

        let snapshot = assembler.assemble(16, Point::default(), Vec::new());

        assert!(
            snapshot.windows.is_empty(),
            "a floating panel is not a Perch: {:?}",
            snapshot.windows
        );
        assert_eq!(
            assembler.standing_on(Point { x: 960.0, y: 0.0 }),
            "nothing",
            "nor anywhere to be standing"
        );
    }

    /// What the layer filter is for, at the seam that shows it: a sprite let go
    /// at the very top of a display falls the whole way instead of coming to
    /// rest on the menu bar.
    #[test]
    fn a_sprite_dropped_at_the_top_of_a_display_falls_past_the_menu_bar() {
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, 1920.0, 1080.0)],
                windows: vec![elevated(
                    2,
                    "Window Server",
                    rect(0.0, 0.0, 1920.0, 30.0),
                    24,
                )],
                dock: None,
            },
        });
        let mut engine = Engine::new(Point { x: 960.0, y: 0.0 });

        let landed = (0..100)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("a hundred ticks produce a hundred frames");

        assert_eq!(landed.state, State::Grounded);
        assert_eq!(
            landed.position.y, 1080.0,
            "the bottom of the display, not the menu bar it started on"
        );
    }

    /// The Dock draws above the overlay, so a sprite resting at the display's
    /// bottom would sit behind it. The window list cannot say where the Dock's top
    /// is, so the fix is upstream: the Engine is handed the usable part of each display.
    #[test]
    fn a_sprite_comes_to_rest_on_the_usable_floor_rather_than_behind_the_dock() {
        // A 1920x1080 display reserving 30 points for the menu bar and 98 for the Dock.
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 30.0, 1920.0, 952.0)],
                windows: Vec::new(),
                dock: None,
            },
        });
        let mut engine = Engine::new(Point { x: 960.0, y: 40.0 });

        let landed = (0..100)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("a hundred ticks produce a hundred frames");

        assert_eq!(landed.state, State::Grounded);
        assert_eq!(
            landed.position.y, 982.0,
            "the Dock's top edge, not the display's bottom edge at 1080"
        );
    }

    /// The world the Shell hands over once the Dock's true bounds are known: the
    /// floor runs to the display's own bottom edge and the Dock is a Perch. A
    /// 1920x1080 display, the Dock's island 234 points in from either side.
    fn dock_aware_desktop() -> WorldGeometry {
        WorldGeometry {
            usable_frames: vec![rect(0.0, 30.0, 1920.0, 1050.0)],
            windows: Vec::new(),
            dock: Some(rect(234.0, 978.0, 1452.0, 92.0)),
        }
    }

    /// Over the Dock, the Dock is still what the sprite rests on, now carried by
    /// a Perch instead of a full-width floor.
    #[test]
    fn a_sprite_over_the_dock_rests_on_the_dock() {
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: dock_aware_desktop(),
        });
        let mut engine = Engine::new(Point { x: 960.0, y: 40.0 });

        let landed = (0..100)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("a hundred ticks produce a hundred frames");

        assert_eq!(landed.state, State::Perched, "the Dock is a Perch now");
        assert_eq!(
            landed.position.y, 978.0,
            "the Dock's own top edge, where the island actually is"
        );
    }

    /// The Dock does not stretch to the sides of the display, and a sprite
    /// beyond its real end must not stand on the full-width strip, walking on air.
    /// Beside the Dock the floor is the display's own bottom edge.
    #[test]
    fn a_sprite_beside_the_dock_falls_to_the_display_bottom() {
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: dock_aware_desktop(),
        });
        let mut engine = Engine::new(Point { x: 100.0, y: 40.0 });

        let landed = (0..100)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("a hundred ticks produce a hundred frames");

        assert_eq!(landed.state, State::Grounded);
        assert_eq!(
            landed.position.y, 1080.0,
            "the display's bottom edge, not the Dock-top strip hanging in air"
        );
    }

    /// The Director hears the difference too — and only on the display that
    /// actually holds the Dock. A second display's floor is nowhere near it.
    #[test]
    fn standing_names_the_dock_and_the_floor_beside_it() {
        let mut desktop = dock_aware_desktop();
        desktop
            .usable_frames
            .push(rect(1920.0, 33.0, 1728.0, 1084.0));

        assert_eq!(
            describe_standing(Point { x: 960.0, y: 978.0 }, &desktop),
            "the top of the Dock"
        );
        assert_eq!(
            describe_standing(
                Point {
                    x: 100.0,
                    y: 1080.0
                },
                &desktop
            ),
            "the display floor, beside the Dock"
        );
        assert_eq!(
            describe_standing(
                Point {
                    x: 2500.0,
                    y: 1117.0
                },
                &desktop
            ),
            "the display floor"
        );
    }

    /// A Dock that hides gives its strip back, and the sprite resting on it is
    /// standing on nothing. Resting is only ever resting on something, re-derived
    /// every tick; a sprite left hanging in the air is the failure nobody notices.
    #[test]
    fn a_reservation_that_disappears_drops_the_sprite_that_was_resting_on_it() {
        let dock = || WorldGeometry {
            usable_frames: vec![rect(0.0, 30.0, 1920.0, 952.0)],
            windows: Vec::new(),
            dock: None,
        };
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: dock(),
        });
        let mut engine = Engine::new(Point { x: 960.0, y: 40.0 });

        let resting = (0..100)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("a hundred ticks produce a hundred frames");
        assert_eq!(resting.position.y, 982.0, "on the Dock");
        assert_eq!(resting.state, State::Grounded);

        // The Dock hides, so the display is usable to its bottom edge.
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 30.0, 1920.0, 1050.0)],
                windows: Vec::new(),
                dock: None,
            },
        });

        let first = engine.tick(&assembler.assemble(20, Point::default(), Vec::new()));
        assert_eq!(
            first.state,
            State::Falling,
            "the strip it was standing on is gone, so it is in the air"
        );

        let landed = (0..100)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("a hundred ticks produce a hundred frames");
        assert_eq!(
            landed.position.y, 1080.0,
            "and falls the rest of the way to the bottom of the display"
        );
    }

    /// A slept machine or a suspended process hands the loop minutes of wall
    /// clock at once.
    #[test]
    fn a_tick_after_a_long_gap_cannot_teleport_the_sprite() {
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, 1000.0, 800.0)],
                windows: Vec::new(),
                dock: None,
            },
        });
        let mut engine = Engine::new(Point { x: 500.0, y: 400.0 });

        let snapshot = assembler.assemble(300_000, Point::default(), Vec::new());
        assert_eq!(
            snapshot.elapsed_ms, 100,
            "no more than one poll interval of world is ever integrated at once"
        );

        let frame = engine.tick(&snapshot);

        assert_eq!(frame.state, State::Falling, "still in the air");
        assert!(
            frame.position.y < 500.0,
            "five minutes of gravity in one tick would put it on the floor at \
             800; it is at {}",
            frame.position.y
        );
    }

    /// The cadence itself: the time a read consumes has to leave the remainder
    /// behind, or every read is late by whatever the tick overshot by and the
    /// desktop is read more slowly than `POLL_INTERVAL` says.
    #[test]
    fn the_platform_is_read_once_per_poll_interval_however_the_ticks_divide_it() {
        let mut assembler = SnapshotAssembler::new(CountingDesktop::default());

        // 16ms ticks divide 100ms unevenly, which is the case that drifts.
        let reads = (0..400)
            .map(|_| {
                assembler
                    .assemble(16, Point::default(), Vec::new())
                    .displays[0]
                    .width
            })
            .last()
            .expect("four hundred ticks produce four hundred snapshots");

        assert_eq!(
            reads, 64.0,
            "6400ms of ticks is sixty-four 100ms poll intervals"
        );
    }

    /// The platform reports the primary display first, and that is the one the
    /// sprite comes into the world on.
    #[test]
    fn a_sprite_starts_on_the_first_display_the_platform_reported() {
        let two_displays = WorldGeometry {
            usable_frames: vec![
                rect(0.0, 30.0, 1920.0, 952.0),
                rect(1920.0, 33.0, 1728.0, 1084.0),
            ],
            windows: Vec::new(),
            dock: None,
        };

        assert_eq!(
            starting_position(&two_displays),
            Point { x: 960.0, y: 506.0 },
            "the middle of the first display, not the second"
        );
    }

    /// The circuit this module closes: geometry from the platform becomes a
    /// world the sprite falls through and lands in.
    #[test]
    fn a_sprite_ticked_from_a_platforms_geometry_lands_on_that_desktops_window() {
        let desktop = WorldGeometry {
            usable_frames: vec![rect(0.0, 0.0, 1000.0, 800.0)],
            // Spans the middle of the display, so the sprite starts above it.
            windows: vec![window(1, "Terminal", rect(400.0, 500.0, 300.0, 200.0))],
            dock: None,
        };
        let start = starting_position(&desktop);
        assert_eq!(
            start,
            Point { x: 500.0, y: 400.0 },
            "the middle of the display it was told about"
        );

        let mut engine = Engine::new(start);
        let mut assembler = SnapshotAssembler::new(FakeWindowSource {
            capabilities: seeing_everything(),
            geometry: desktop,
        });

        let landed = (0..50)
            .map(|_| engine.tick(&assembler.assemble(20, Point::default(), Vec::new())))
            .last()
            .expect("fifty ticks produce fifty frames");

        assert_eq!(landed.state, State::Perched);
        assert_eq!(landed.position.y, 500.0, "the window's top edge");
    }

    #[test]
    fn standing_names_the_window_owner_not_a_title() {
        let desktop = WorldGeometry {
            usable_frames: vec![rect(0.0, 30.0, 1920.0, 1050.0)],
            windows: vec![window(1, "Cursor", rect(100.0, 200.0, 800.0, 600.0))],
            dock: None,
        };
        assert_eq!(
            describe_standing(Point { x: 140.0, y: 200.0 }, &desktop),
            "a Cursor window"
        );
        assert_eq!(
            describe_standing(
                Point {
                    x: 960.0,
                    y: 1080.0
                },
                &desktop
            ),
            "the display floor, above the Dock"
        );
        assert_eq!(
            describe_standing(Point { x: 0.0, y: 400.0 }, &desktop),
            "the screen edge"
        );
    }

    /// A titleless palette in front of the document window must not hide the
    /// document's title, and another application's title is never borrowed.
    #[test]
    fn the_front_title_is_the_first_titled_window_the_application_owns() {
        let titled = |id, owner: &str, title: Option<&str>| WindowRect {
            title: title.map(String::from),
            ..window(id, owner, rect(0.0, 0.0, 100.0, 100.0))
        };
        let desktop = WorldGeometry {
            usable_frames: vec![rect(0.0, 30.0, 1920.0, 1050.0)],
            windows: vec![
                titled(1, "Safari", Some("PR #1354")),
                titled(2, "Code", None),
                titled(3, "Code", Some("main.rs — fidget")),
                titled(4, "Code", Some("lib.rs — fidget")),
            ],
            dock: None,
        };

        assert_eq!(
            front_title("Code", &desktop).as_deref(),
            Some("main.rs — fidget")
        );
        assert_eq!(front_title("Mail", &desktop), None);
    }

    #[test]
    fn furniture_is_not_named_as_a_perch() {
        let desktop = WorldGeometry {
            usable_frames: vec![rect(0.0, 30.0, 1920.0, 1050.0)],
            windows: vec![elevated(1, "Dock", rect(0.0, 1050.0, 1920.0, 80.0), 20)],
            dock: None,
        };
        assert_eq!(
            describe_standing(
                Point {
                    x: 960.0,
                    y: 1080.0
                },
                &desktop
            ),
            "the display floor, above the Dock"
        );
    }

    /// Quarter of the request, capped near 5 ms: the shape measured for
    /// `thread::sleep` on the baseline machine.
    fn late_sleep(requested: Duration) -> Duration {
        let over = Duration::from_secs_f64(requested.as_secs_f64() * 0.25);
        requested + over.min(Duration::from_millis(5))
    }

    fn paced_polls(work: Duration) -> u32 {
        let start = Instant::now();
        let woke = start;
        let mut now = start + work;
        let mut deadline = next_poll_at(RIDE_POLL_INTERVAL, woke, woke, now);
        let mut polls = 1u32;
        while now.duration_since(start) < Duration::from_secs(1) {
            let started = now + late_sleep(deadline.saturating_duration_since(now));
            if started.duration_since(start) >= Duration::from_secs(1) {
                break;
            }
            now = started + work;
            polls += 1;
            deadline = next_poll_at(RIDE_POLL_INTERVAL, deadline, started, now);
        }
        polls
    }

    fn remainder_sleep_polls(work: Duration) -> u32 {
        let start = Instant::now();
        let mut now = start;
        let mut polls = 0u32;
        while now.duration_since(start) < Duration::from_secs(1) {
            now += work;
            polls += 1;
            now += late_sleep(RIDE_POLL_INTERVAL.saturating_sub(work));
        }
        polls
    }

    struct Hold {
        open: Mutex<bool>,
        cv: Condvar,
    }

    struct Release(Arc<Hold>);

    impl Release {
        fn open(&self) {
            *self.0.open.lock().expect("hold") = true;
            self.0.cv.notify_all();
        }
    }

    impl Drop for Release {
        fn drop(&mut self) {
            self.open();
        }
    }

    struct TimedDesktop {
        calls: Arc<AtomicUsize>,
        stamps: Arc<Mutex<Vec<Instant>>>,
        delay: Duration,
        hold: Option<Arc<Hold>>,
        block_from: usize,
    }

    impl TimedDesktop {
        fn counting(calls: Arc<AtomicUsize>, stamps: Arc<Mutex<Vec<Instant>>>) -> Self {
            Self {
                calls,
                stamps,
                delay: Duration::ZERO,
                hold: None,
                block_from: usize::MAX,
            }
        }
    }

    impl WindowSource for TimedDesktop {
        fn capabilities(&self) -> Capabilities {
            seeing_everything()
        }

        fn read(&self) -> WorldGeometry {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            self.stamps.lock().expect("stamps").push(Instant::now());
            if !self.delay.is_zero() {
                let until = Instant::now() + self.delay;
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
            }
            if let Some(hold) = &self.hold {
                if n >= self.block_from {
                    let mut open = hold.open.lock().expect("hold");
                    while !*open {
                        open = hold.cv.wait(open).expect("hold");
                    }
                }
            }
            WorldGeometry {
                usable_frames: vec![rect(0.0, 0.0, n as f64 + 1.0, 800.0)],
                windows: Vec::new(),
                dock: None,
            }
        }
    }

    fn wait_until(mut pred: impl FnMut() -> bool, limit: Duration) -> bool {
        let start = Instant::now();
        while !pred() {
            if start.elapsed() > limit {
                return false;
            }
            thread::sleep(Duration::from_millis(1));
        }
        true
    }

    fn median_gap(stamps: &[Instant]) -> Duration {
        let mut gaps: Vec<Duration> = stamps
            .windows(2)
            .map(|pair| pair[1].saturating_duration_since(pair[0]))
            .collect();
        gaps.sort();
        gaps[gaps.len() / 2]
    }
}
