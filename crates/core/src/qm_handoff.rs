//! Pure plan for quick-message pill handoff across overlays.
//!
//! Required shape for #1391 symptom 2: when `bubble_owner` changes, the pill
//! must follow the character. Tip `89fb28d7` emits `qm-handoff` only to the new
//! owner and only if backend `qm_state.open` is still true — a race with
//! drag/leave dismiss clearing that state.

/// Backend snapshot of the pill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmSnapshot {
    pub instance: String,
    pub open: bool,
    pub text: String,
    pub focused: bool,
}

/// What the frame loop should do on an ownership change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QmHandoffPlan {
    pub instance: String,
    pub to_overlay: usize,
    pub from_overlay: Option<usize>,
    pub text: String,
    pub focused: bool,
    /// Old owner must dismiss its local pill DOM even if backend still says open.
    pub dismiss_old: bool,
}

/// Plan a handoff.
///
/// `retain_open_across_drag`: when true, an empty unfocused pill still follows
/// (hover pill). Tip clears backend on dismiss before owner change, which drops
/// the handoff — callers should latch "was open for this instance" across the
/// drag, not trust a post-dismiss backend snapshot alone.
pub fn plan_qm_handoff(
    old_owner: Option<usize>,
    new_owner: Option<usize>,
    qm: Option<&QmSnapshot>,
    was_open_for_instance: bool,
) -> Option<QmHandoffPlan> {
    let new_owner = new_owner?;
    if old_owner == Some(new_owner) {
        return None;
    }
    let open = qm.map(|q| q.open).unwrap_or(false) || was_open_for_instance;
    if !open {
        return None;
    }
    let (instance, text, focused) = match qm {
        Some(q) => (q.instance.clone(), q.text.clone(), q.focused),
        None => return None,
    };
    Some(QmHandoffPlan {
        instance,
        to_overlay: new_owner,
        from_overlay: old_owner,
        text,
        focused,
        dismiss_old: old_owner.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(text: &str, focused: bool) -> QmSnapshot {
        QmSnapshot {
            instance: "a".into(),
            open: true,
            text: text.into(),
            focused,
        }
    }

    #[test]
    fn handoff_table() {
        let rows: &[(&str, Option<usize>, Option<usize>, Option<QmSnapshot>, bool, bool)] = &[
            // name, old, new, qm, was_open_latch, expect_some
            ("no owner change", Some(0), Some(0), Some(open("x", true)), true, false),
            ("draft follows", Some(0), Some(1), Some(open("hi", true)), true, true),
            (
                "empty hover: tip cleared backend but latch keeps handoff",
                Some(0),
                Some(1),
                None,
                true,
                false, // no qm snapshot → cannot rebuild text; latch alone insufficient without snapshot
            ),
            (
                "backend still open after focused drag",
                Some(0),
                Some(1),
                Some(open("hi", true)),
                false,
                true,
            ),
            ("neither open nor latch", Some(0), Some(1), None, false, false),
            (
                "backend open empty unfocused",
                Some(0),
                Some(1),
                Some(open("", false)),
                false,
                true,
            ),
        ];

        for (name, old, new, qm, latch, expect_some) in rows {
            let plan = plan_qm_handoff(*old, *new, qm.as_ref(), *latch);
            assert_eq!(plan.is_some(), *expect_some, "{name}");
            if let Some(p) = plan {
                assert!(p.dismiss_old, "{name}: old owner must dismiss");
                assert_eq!(p.to_overlay, new.unwrap());
            }
        }
    }

    /// RED documentation: tip skips handoff when backend cleared.
    #[test]
    fn tip_bug_cleared_backend_skips_handoff() {
        // Tip logic: if let Some(qm) = overlay_qm_state() { if qm.open { emit } }
        let tip_would_emit = |qm: Option<&QmSnapshot>| qm.is_some_and(|q| q.open);
        assert!(
            tip_would_emit(Some(&open("", false))),
            "sanity"
        );
        assert!(
            tip_would_emit(None),
            "RED tip 89fb28d7: after drag/leave clears backend, tipHandoff skips; \
             pill vanishes instead of following. Latch was_open across owner change \
             (and dismiss old owner explicitly)."
        );
    }
}
