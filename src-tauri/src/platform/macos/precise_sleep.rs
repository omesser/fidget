//! A sleep that macOS does not coalesce, for the moving tick.
//!
//! `std::thread::sleep` gets timer slack, so a 16 ms moving tick lands up to
//! 4 ms late. A one-shot kqueue timer with `NOTE_CRITICAL` fires on time and
//! leaves the thread as it was. `THREAD_TIME_CONSTRAINT_POLICY` also fires on
//! time, but `THREAD_STANDARD_POLICY` never gives the thread its slack back,
//! so every still tick after the first move runs fast instead of coalesced.

use std::time::Duration;

/// Sleep `duration` with no timer slack. Falls back to `thread::sleep` if
/// the kqueue calls fail.
pub(crate) fn sleep_precisely(duration: Duration) {
    // SAFETY: plain syscalls on a descriptor this function owns and closes.
    let fired = unsafe {
        let kq = libc::kqueue();
        if kq < 0 {
            false
        } else {
            let timer = libc::kevent {
                ident: 0,
                filter: libc::EVFILT_TIMER,
                flags: libc::EV_ADD | libc::EV_ONESHOT,
                fflags: libc::NOTE_NSECONDS | libc::NOTE_CRITICAL,
                data: isize::try_from(duration.as_nanos())
                    .unwrap_or(isize::MAX)
                    .max(1),
                udata: std::ptr::null_mut(),
            };
            let mut event: libc::kevent = std::mem::zeroed();
            let n = libc::kevent(kq, &raw const timer, 1, &raw mut event, 1, std::ptr::null());
            libc::close(kq);
            n == 1 && event.flags & libc::EV_ERROR == 0
        }
    };
    if !fired {
        std::thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Instant;

    const TICK: Duration = Duration::from_millis(16);

    fn percentile(mut gaps: Vec<f64>, p: f64) -> f64 {
        gaps.sort_by(f64::total_cmp);
        gaps[((gaps.len() - 1) as f64 * p).round() as usize]
    }

    /// The frame loop's still tick, a tick from each wake. Gap p50 in ms.
    fn still_p50(ticks: usize) -> f64 {
        let mut last = Instant::now();
        let gaps = (0..ticks)
            .map(|_| {
                thread::sleep(TICK);
                let gap = last.elapsed().as_secs_f64() * 1e3;
                last = Instant::now();
                gap
            })
            .collect();
        percentile(gaps, 0.5)
    }

    /// The frame loop's moving tick, paced from the last deadline. Gap p90 in ms.
    fn moving_p90(ticks: usize, wait: impl Fn(Duration)) -> f64 {
        let mut deadline = Instant::now();
        let mut last = Instant::now();
        let gaps = (0..=ticks)
            .map(|_| {
                let now = Instant::now();
                deadline = (deadline + TICK).max(now);
                wait(deadline - now);
                let gap = last.elapsed().as_secs_f64() * 1e3;
                last = Instant::now();
                gap
            })
            .skip(1)
            .collect();
        percentile(gaps, 0.9)
    }

    /// Still p50 before, moving p90, still p50 after, on a fresh thread.
    fn stretch(wait: impl Fn(Duration) + Send + 'static) -> (f64, f64, f64) {
        thread::spawn(move || (still_p50(60), moving_p90(200, wait), still_p50(60)))
            .join()
            .unwrap()
    }

    /// Moving ticks land on the deadline, and the still ticks after them keep
    /// the coalesced ~20 ms a fresh thread gets, not the realtime ~16 ms.
    #[test]
    #[ignore = "wall-clock timing; shared CI runners miss 16.1 ms, so run by hand"]
    fn a_moving_stretch_is_on_time_and_the_still_ticks_after_it_stay_coalesced() {
        let (before, moving, after) = stretch(sleep_precisely);
        assert!(moving <= 16.1, "moving p90 {moving:.2} ms");
        assert!(
            after >= 18.5,
            "still p50 after {after:.2} ms, before {before:.2} ms"
        );
    }
}
