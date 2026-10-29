//! What a Harness turn asks the character to play, from tool-call news.
//! No model call: the timestamps are the stream's, and the Engine plays the
//! result itself.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use crate::character::Primitive;

/// How long a tool stays in progress before working plays. One play covers
/// every other call still open, so a burst shares it. Two seconds keeps a
/// call that finishes immediately from playing it.
pub const WORKING_AFTER: Duration = Duration::from_secs(2);

/// A beat the Engine plays during a Harness turn. Not a Behavior name: a
/// Character cannot declare a replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// A tool has been in progress for [`WORKING_AFTER`].
    Working,
    /// A tool reported `completed`.
    Succeeded,
    /// A tool reported `failed`.
    Failed,
    /// The turn ended.
    Payoff,
}

impl Moment {
    /// The Primitive the Engine plays. Talk and sit stay put — a walk keeps
    /// its speed and leaves the perch. Sit is the failed beat; the one
    /// reaction in the required set is the success and the payoff.
    pub fn primitive(self) -> Primitive {
        match self {
            Self::Working => Primitive::Talk,
            Self::Succeeded => Primitive::React,
            Self::Failed => Primitive::Sit,
            Self::Payoff => Primitive::React,
        }
    }
}

/// A change in one tool call's status. `pending` is not a change: the call
/// has not started, and a repeat of the same outcome is not one either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolChange {
    Running,
    Completed,
    Failed,
}

/// One tool-status change at a moment on the turn's clock.
pub struct Beat<'a> {
    pub at: Duration,
    pub id: &'a str,
    pub change: ToolChange,
}

/// Tool calls still open, and whether this stretch already played working.
#[derive(Default)]
pub struct TurnMotion {
    /// When each still-open call went in progress.
    running: BTreeMap<String, Duration>,
    /// Calls that already played an outcome, so a repeat update does not.
    finished: BTreeSet<String>,
    /// The stretch `working` already covered. A call that starts while
    /// another is still open shares that one play. Cleared when the next
    /// stretch begins, after every call has finished.
    covered_start: Option<Duration>,
}

impl TurnMotion {
    /// Status news at `at`. `at` is non-decreasing across a turn.
    pub fn update(&mut self, at: Duration, id: &str, change: ToolChange) -> Vec<Moment> {
        let mut plays = self.due(at);
        match change {
            ToolChange::Running => {
                if self.finished.contains(id) || self.running.contains_key(id) {
                    return plays;
                }
                if self.running.is_empty() {
                    self.covered_start = None;
                }
                self.running.insert(id.to_string(), at);
            }
            ToolChange::Completed | ToolChange::Failed => {
                if !self.finished.insert(id.to_string()) {
                    return plays;
                }
                self.running.remove(id);
                plays.push(if change == ToolChange::Completed {
                    Moment::Succeeded
                } else {
                    Moment::Failed
                });
            }
        }
        plays
    }

    /// Time passed with no status news. Plays working once it is due.
    pub fn poll(&mut self, at: Duration) -> Vec<Moment> {
        self.due(at)
    }

    /// The turn ended. A working that came due plays before the payoff.
    pub fn end(&mut self, at: Duration) -> Vec<Moment> {
        let mut plays = self.due(at);
        self.running.clear();
        self.covered_start = None;
        plays.push(Moment::Payoff);
        plays
    }

    /// When working would play if nothing else arrives. `None` once this
    /// stretch has played it, or while no call is in progress.
    pub fn next_working(&self) -> Option<Duration> {
        if self.covered_start.is_some() || self.running.is_empty() {
            return None;
        }
        let start = self.running.values().copied().min()?;
        Some(start.saturating_add(WORKING_AFTER))
    }

    fn due(&mut self, at: Duration) -> Vec<Moment> {
        let Some(due) = self.next_working() else {
            return Vec::new();
        };
        if at < due {
            return Vec::new();
        }
        self.covered_start = self.running.values().copied().min();
        vec![Moment::Working]
    }
}

/// `beats` in time order, then `end` when the turn closes. Working is stamped
/// at the moment it comes due, not at the next status change.
pub fn replay(beats: &[Beat<'_>], end: Option<Duration>) -> Vec<(Duration, Moment)> {
    let mut motion = TurnMotion::default();
    let mut out = Vec::new();
    let mut index = 0;
    loop {
        if let Some(due) = motion.next_working() {
            let next = beats.get(index).map(|beat| beat.at);
            let before_next = next.is_none_or(|next| due <= next);
            let before_end = end.is_none_or(|end| due <= end);
            if before_next && before_end {
                out.extend(motion.poll(due).into_iter().map(|moment| (due, moment)));
                continue;
            }
        }
        if let Some(beat) = beats.get(index) {
            index += 1;
            if let Some(end) = end.filter(|end| beat.at > *end) {
                out.extend(motion.end(end).into_iter().map(|moment| (end, moment)));
                break;
            }
            out.extend(
                motion
                    .update(beat.at, beat.id, beat.change)
                    .into_iter()
                    .map(|moment| (beat.at, moment)),
            );
            continue;
        }
        if let Some(end) = end {
            out.extend(motion.end(end).into_iter().map(|moment| (end, moment)));
        }
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn beat(at: u64, id: &str, change: ToolChange) -> Beat<'_> {
        Beat {
            at: ms(at),
            id,
            change,
        }
    }

    #[test]
    fn a_long_running_tool_plays_working_then_succeeded_and_the_turn_pays_off() {
        let beats = [
            beat(0, "t1", ToolChange::Running),
            beat(5_000, "t1", ToolChange::Completed),
        ];
        assert_eq!(
            replay(&beats, Some(ms(5_000))),
            vec![
                (ms(2_000), Moment::Working),
                (ms(5_000), Moment::Succeeded),
                (ms(5_000), Moment::Payoff),
            ]
        );
    }

    #[test]
    fn a_failed_tool_plays_failed_and_not_succeeded() {
        let beats = [
            beat(0, "t1", ToolChange::Running),
            beat(4_000, "t1", ToolChange::Failed),
        ];
        let plays = replay(&beats, Some(ms(4_000)));
        assert_eq!(
            plays,
            vec![
                (ms(2_000), Moment::Working),
                (ms(4_000), Moment::Failed),
                (ms(4_000), Moment::Payoff),
            ]
        );
        assert!(!plays.iter().any(|(_, moment)| *moment == Moment::Succeeded));
    }

    #[test]
    fn a_burst_inside_the_window_plays_working_once() {
        let beats = [
            beat(0, "a", ToolChange::Running),
            beat(400, "b", ToolChange::Running),
            beat(800, "c", ToolChange::Running),
            beat(5_000, "a", ToolChange::Completed),
            beat(5_100, "b", ToolChange::Completed),
            beat(5_200, "c", ToolChange::Failed),
        ];
        let plays = replay(&beats, Some(ms(5_200)));
        let working: Vec<_> = plays
            .iter()
            .filter(|(_, moment)| *moment == Moment::Working)
            .collect();
        assert_eq!(working, vec![&(ms(2_000), Moment::Working)]);
        assert_eq!(
            plays
                .iter()
                .filter(|(_, moment)| *moment == Moment::Succeeded)
                .count(),
            2
        );
        assert_eq!(
            plays
                .iter()
                .filter(|(_, moment)| *moment == Moment::Failed)
                .count(),
            1
        );
    }

    #[test]
    fn a_tool_that_finishes_inside_the_window_does_not_play_working() {
        let beats = [
            beat(0, "a", ToolChange::Running),
            beat(100, "a", ToolChange::Completed),
            beat(200, "b", ToolChange::Running),
            beat(300, "b", ToolChange::Completed),
        ];
        assert_eq!(
            replay(&beats, Some(ms(300))),
            vec![
                (ms(100), Moment::Succeeded),
                (ms(300), Moment::Succeeded),
                (ms(300), Moment::Payoff),
            ]
        );
    }

    #[test]
    fn a_repeated_outcome_does_not_play_twice() {
        let beats = [
            beat(0, "t1", ToolChange::Running),
            beat(3_000, "t1", ToolChange::Completed),
            beat(3_100, "t1", ToolChange::Completed),
        ];
        assert_eq!(
            replay(&beats, Some(ms(3_100))),
            vec![
                (ms(2_000), Moment::Working),
                (ms(3_000), Moment::Succeeded),
                (ms(3_100), Moment::Payoff),
            ]
        );
    }
}
