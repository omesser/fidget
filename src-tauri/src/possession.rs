//! One possessed Instance's keyboard. The frame loop owns the session. The
//! webview only enqueues key and focus edges, so the two threads never share
//! the `Possession`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use fidget_core::possession::{Arrow, Intent, Key, Possession};

/// A blur shorter than this is the focus request's own false-then-true pair,
/// not the user leaving. Longer, and the keyups will never arrive.
pub const BLUR_SETTLE: Duration = Duration::from_millis(80);

/// Why the session ended. Each path clears the same switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    Escape,
    /// The Release Possession row.
    Menu,
    Hide,
    Quit,
    FullscreenHide,
    Removed,
}

/// The Instance the keys move, and the overlay that holds them.
pub struct Session {
    pub instance: String,
    pub window: String,
    keys: Possession,
    paused: bool,
    blurred_at: Option<Instant>,
    /// The press began while paused, so its click resumes and is not a Poke.
    /// A drag that follows still grabs.
    pub swallow_click: bool,
    /// True on the tick the session starts, so the overlay can play its cue.
    pub cue: bool,
}

impl Session {
    pub fn start(instance: impl Into<String>, window: impl Into<String>) -> Self {
        Self {
            instance: instance.into(),
            window: window.into(),
            keys: Possession::default(),
            paused: false,
            blurred_at: None,
            swallow_click: false,
            cue: true,
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn apply(&mut self, edge: &Edge, now: Instant) {
        let window = match edge {
            Edge::Key { window, .. } | Edge::Focus { window, .. } => window,
        };
        if window != &self.window {
            return;
        }
        match edge {
            Edge::Key { name, down, .. } => {
                if self.paused {
                    return;
                }
                if let Some(key) = parse_key(name) {
                    self.keys.key(key, *down);
                }
            }
            Edge::Focus { focused, .. } => {
                if *focused {
                    self.blurred_at = None;
                    self.paused = false;
                } else if self.blurred_at.is_none() {
                    self.blurred_at = Some(now);
                }
            }
        }
    }

    /// What the keys ask for this tick. A settled blur drops every held key
    /// before the tick, so a jump cannot launch into a pause.
    pub fn tick(&mut self, elapsed_ms: u32, now: Instant) -> Intent {
        if let Some(at) = self.blurred_at {
            if !self.paused && now.saturating_duration_since(at) >= BLUR_SETTLE {
                self.paused = true;
                self.keys.pause();
                self.blurred_at = None;
            }
        }
        self.keys.tick(elapsed_ms)
    }
}

/// A key or focus edge from one overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edge {
    Key {
        window: String,
        name: String,
        down: bool,
    },
    Focus {
        window: String,
        focused: bool,
    },
}

/// The names the overlay sends. Anything else is not a possession key.
pub fn parse_key(name: &str) -> Option<Key> {
    match name {
        "left" => Some(Key::Arrow(Arrow::Left)),
        "right" => Some(Key::Arrow(Arrow::Right)),
        "space" => Some(Key::Space),
        "escape" => Some(Key::Escape),
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
            Some(Key::Digit(name.as_bytes()[0] - b'0'))
        }
        _ => None,
    }
}

static EDGES: Mutex<Vec<Edge>> = Mutex::new(Vec::new());

pub fn push_key(window: &str, name: &str, down: bool) {
    push(Edge::Key {
        window: window.to_string(),
        name: name.to_string(),
        down,
    });
}

pub fn push_focus(window: &str, focused: bool) {
    push(Edge::Focus {
        window: window.to_string(),
        focused,
    });
}

fn push(edge: Edge) {
    if let Ok(mut edges) = EDGES.lock() {
        edges.push(edge);
    }
}

pub fn take_edges() -> Vec<Edge> {
    EDGES
        .lock()
        .ok()
        .map(|mut edges| std::mem::take(&mut *edges))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str, down: bool) -> Edge {
        Edge::Key {
            window: "overlay-0".into(),
            name: name.into(),
            down,
        }
    }

    fn focus(focused: bool) -> Edge {
        Edge::Focus {
            window: "overlay-0".into(),
            focused,
        }
    }

    #[test]
    fn arrows_space_escape_and_digits_are_the_possession_keys() {
        assert_eq!(parse_key("left"), Some(Key::Arrow(Arrow::Left)));
        assert_eq!(parse_key("right"), Some(Key::Arrow(Arrow::Right)));
        assert_eq!(parse_key("space"), Some(Key::Space));
        assert_eq!(parse_key("escape"), Some(Key::Escape));
        assert_eq!(parse_key("1"), Some(Key::Digit(1)));
        assert_eq!(parse_key("9"), Some(Key::Digit(9)));
        assert!(parse_key("0").is_none());
        assert!(parse_key("up").is_none());
        assert!(parse_key("a").is_none());
        assert!(parse_key("d").is_none());
    }

    #[test]
    fn a_digit_press_reaches_the_intent_once() {
        let mut session = Session::start("a", "overlay-0");
        let t0 = Instant::now();
        session.apply(&key("4", true), t0);
        assert_eq!(session.tick(16, t0).play, Some(4));
        session.apply(&key("4", true), t0);
        assert_eq!(session.tick(16, t0).play, None);
        session.apply(&key("4", false), t0);
        assert_eq!(session.tick(16, t0).play, None);
    }

    #[test]
    fn a_blur_that_returns_at_once_keeps_a_held_space() {
        let mut session = Session::start("a", "overlay-0");
        let t0 = Instant::now();
        session.apply(&key("space", true), t0);
        session.apply(&focus(false), t0);
        session.apply(&focus(true), t0 + Duration::from_millis(10));
        let intent = session.tick(16, t0 + Duration::from_millis(20));
        assert!(!session.paused());
        assert!(intent.charging.is_some());
        assert!(intent.launch.is_none());
    }

    #[test]
    fn a_blur_that_stays_pauses_and_drops_space_without_launching() {
        let mut session = Session::start("a", "overlay-0");
        let t0 = Instant::now();
        session.apply(&key("space", true), t0);
        session.tick(16, t0);
        session.apply(&focus(false), t0);
        let early = session.tick(16, t0 + Duration::from_millis(40));
        assert!(!session.paused());
        assert!(early.launch.is_none());
        assert!(early.charging.is_some());
        let later = session.tick(16, t0 + BLUR_SETTLE + Duration::from_millis(5));
        assert!(session.paused());
        assert!(later.charging.is_none());
        assert!(later.launch.is_none());
    }

    #[test]
    fn escape_asks_to_end_and_does_not_launch() {
        let mut session = Session::start("a", "overlay-0");
        let t0 = Instant::now();
        session.apply(&key("space", true), t0);
        session.tick(200, t0);
        session.apply(&key("escape", true), t0);
        let intent = session.tick(16, t0);
        assert!(intent.end_possession);
        assert!(intent.launch.is_none());
        assert!(intent.charging.is_none());
    }
}
