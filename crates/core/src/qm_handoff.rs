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
/// `drag_latch`: when true, the pill was dismissed by drag/leave during this
/// ownership period, so it should follow even if backend state is cleared.
/// The latch means "closed by drag THIS crossing", not "was ever open".
pub fn plan_qm_handoff(
    old_owner: Option<usize>,
    new_owner: Option<usize>,
    qm: Option<&QmSnapshot>,
    drag_latch: bool,
) -> Option<QmHandoffPlan> {
    let new_owner = new_owner?;
    if old_owner == Some(new_owner) {
        return None;
    }
    let open = qm.map(|q| q.open).unwrap_or(false) || drag_latch;
    if !open {
        return None;
    }
    let (instance, text, focused) = {
        let q = qm?;
        (q.instance.clone(), q.text.clone(), q.focused)
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
    #[allow(clippy::type_complexity)]
    fn handoff_table() {
        let rows: &[(
            &str,
            Option<usize>,
            Option<usize>,
            Option<QmSnapshot>,
            bool,
            bool,
        )] = &[
            // name, old, new, qm, drag_latch, expect_some
            (
                "no owner change",
                Some(0),
                Some(0),
                Some(open("x", true)),
                true,
                false,
            ),
            (
                "draft follows (backend still open)",
                Some(0),
                Some(1),
                Some(open("hi", true)),
                false,
                true,
            ),
            (
                "closed by Send then owner change → no handoff",
                Some(0),
                Some(1),
                None,
                false,
                false,
            ),
            (
                "closed by drag then owner change → handoff",
                Some(0),
                Some(1),
                Some(open("", false)),
                true,
                true,
            ),
            (
                "slot cleared + drag latch → handoff (empty text OK)",
                Some(0),
                Some(1),
                Some(open("", false)),
                true,
                true,
            ),
            (
                "backend still open after focused drag",
                Some(0),
                Some(1),
                Some(open("hi", true)),
                false,
                true,
            ),
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
}
