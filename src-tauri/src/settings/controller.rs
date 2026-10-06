//! What a gesture on the settings form means, with no window in it.
//!
//! `form.rs` draws the form; this returns the `Outcome` for a row id and value.

use crate::settings::form::{FormDescription, RowOperation};
use crate::settings::{AiDraft, SettingsPatch, SettingsView};

/// What the surface reports a user did, by row id.
///
/// `Pick` is separate from `SetText` because a list re-reports the value already shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    SetBool {
        id: String,
        value: bool,
    },
    SetText {
        id: String,
        value: String,
    },
    Pick {
        id: String,
        value: String,
    },
    /// A shortcut list, which writes the row below it rather than one of its
    /// own. `current` is what that row holds now, which only the surface can
    /// read (#670).
    Shortcut {
        id: String,
        value: String,
        current: String,
    },
    Press {
        id: String,
    },
}

/// What the surface must do about it.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The gesture named no write: an unknown row, a frozen one, or a value
    /// that is already what is shown.
    Nothing,
    /// Send it through `SettingsSession::apply`, then redraw. The page draws
    /// every tab switch from its cached snapshot, so a write it is not told
    /// about comes back undone on the next switch.
    Apply(SettingsPatch),
    /// Apply's two halves: write the patch if there is one, and only then take
    /// the AI tab back to live state. A locked Keychain fails the write,
    /// and resetting anyway would discard an edit nothing saved (#279).
    Commit(Option<SettingsPatch>),
    /// Back to live state, writing nothing.
    Reset,
    /// Stage the key delete: blank the key field and arm Apply, without
    /// touching the store (#279).
    ClearKey,
    /// Put `value` in row `id`, then apply `patch` — or, when there is none,
    /// let the row sit staged until Apply.
    Fill {
        id: &'static str,
        value: &'static str,
        patch: Option<SettingsPatch>,
    },
    /// A modal, a file, the clipboard, a spawn: the operations that need more
    /// than a patch, and that only the surface can perform.
    Run(RowOperation),
}

/// `draft` is the AI tab as the surface holds it, and carries the
/// `FormDescription` every decision here is taken against — the caller's, so
/// the row lookup and the frozen rule answer from one description.
pub fn handle(event: &Event, draft: &AiDraft<'_>, view: &SettingsView) -> Outcome {
    let description = draft.description;
    match event {
        Event::SetBool { id, value } => {
            if description.frozen(id) {
                return Outcome::Nothing;
            }
            match description.bool_write(id) {
                Some(_) if description.batched(id) => Outcome::Nothing,
                Some(field) => {
                    let mut patch = SettingsPatch::default();
                    patch.set_bool(field, *value);
                    Outcome::Apply(patch)
                }
                None => Outcome::Nothing,
            }
        }
        Event::SetText { id, value } => {
            // A batched row lives in the widgets until Apply. Writing here
            // would kill a Harness child the Cancel button still offers (#663).
            if description.batched(id) {
                return Outcome::Nothing;
            }
            text_patch(description, id, value).map_or(Outcome::Nothing, Outcome::Apply)
        }
        Event::Pick { id, value } => {
            // A list sends its action for a click on the item already
            // selected. Writing that back is a save and a redraw for nothing,
            // and on the source list it is lossy (#452).
            if view.popup_value(id).as_deref() == Some(value.as_str()) {
                return Outcome::Nothing;
            }
            if description.batched(id) {
                return Outcome::Nothing;
            }
            text_patch(description, id, value).map_or(Outcome::Nothing, Outcome::Apply)
        }
        Event::Shortcut { id, value, current } => shortcut(description, id, value, current),
        Event::Press { id } => match description.operations.get(id) {
            Some(RowOperation::Apply) => Outcome::Commit(draft.patch(view)),
            Some(RowOperation::Cancel) => Outcome::Reset,
            Some(RowOperation::ClearKey) => Outcome::ClearKey,
            // Carries no row, so a staged edit beside it is neither written
            // nor thrown away: this button is a session boundary and nothing
            // else (#679).
            Some(RowOperation::NewSession) => Outcome::Apply(SettingsPatch {
                new_session: true,
                ..SettingsPatch::default()
            }),
            Some(op) => Outcome::Run(op.clone()),
            None => Outcome::Nothing,
        },
    }
}

fn shortcut(description: &FormDescription, id: &str, value: &str, current: &str) -> Outcome {
    let Some(shortcut) = description.shortcut(id) else {
        return Outcome::Nothing;
    };
    // Custom, and any title off the list, name nothing to write: the row below
    // is what a value this list cannot spell is.
    let Some(value) = (shortcut.value)(value) else {
        return Outcome::Nothing;
    };
    if current == value {
        return Outcome::Nothing;
    }
    Outcome::Fill {
        id: shortcut.row,
        value,
        // A batched row stages until Apply, which saves the AI switches, wake
        // interval, registration Harness, endpoint, and source together. An
        // unbatched row has no Apply and saves here. #279.
        patch: if description.batched(shortcut.row) {
            None
        } else {
            text_patch(description, shortcut.row, value)
        },
    }
}

/// `None` when the row writes nothing, is frozen, or the value is one
/// `set_text` refuses — all of which are the same answer to the surface.
fn text_patch(description: &FormDescription, id: &str, value: &str) -> Option<SettingsPatch> {
    if description.frozen(id) {
        return None;
    }
    let field = description.text_write(id)?;
    let mut patch = SettingsPatch::default();
    patch.set_text(field, value).then_some(patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model;
    use crate::settings::form;
    use crate::settings::Settings;
    use std::path::Path;

    const BASE_URL: &str = "https://api.openai.com";
    const MODEL: &str = "gpt-4o-mini";

    /// The AI tab's live state: a saved Base URL and Model, no key.
    fn director_view() -> SettingsView {
        SettingsView::from_parts(
            &Settings {
                director_base_url: BASE_URL.into(),
                director_model: MODEL.into(),
                ..Settings::default()
            },
            Path::new("/tmp/fidget/memory.md"),
            None,
            Vec::new(),
            Vec::new(),
            (false, String::new(), String::new()),
            None,
        )
    }

    fn press(id: &str) -> Event {
        Event::Press { id: id.into() }
    }

    /// Apply on an untouched AI tab must write nothing: a window that
    /// reads its own blank fields back as an edit would wipe the saved Base URL
    /// and Model.
    #[test]
    fn an_untouched_director_tab_applies_nothing() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            assert_eq!(
                handle(&press(form::APPLY_ID), &AiDraft::live(&description), &view),
                Outcome::Commit(None),
            );
        });
    }

    /// Clearing a field is a real edit, so the controller cannot tell an
    /// untouched tab from a cleared one. Only a window that draws before it
    /// reads itself back can; do not put a blank guard in `AiDraft::patch`.
    #[test]
    fn a_blank_field_is_an_edit_not_an_untouched_row() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let blank = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "", "director_model": "" }),
            );
            let Outcome::Commit(Some(patch)) = handle(&press(form::APPLY_ID), &blank, &view) else {
                panic!("a cleared pair of rows is a patch");
            };
            assert_eq!(patch.completer.director_base_url.as_deref(), Some(""));
            assert_eq!(patch.completer.director_model.as_deref(), Some(""));
        });
    }

    /// One typed row does not drag the other into the patch, which is what
    /// keeps an Apply from writing back stale text beside the edit (#279).
    #[test]
    fn apply_carries_only_the_row_that_changed() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let typed = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "https://api.x.ai" }),
            );
            let Outcome::Commit(Some(patch)) = handle(&press(form::APPLY_ID), &typed, &view) else {
                panic!("a typed URL is a patch");
            };
            assert_eq!(
                patch.completer.director_base_url.as_deref(),
                Some("https://api.x.ai")
            );
            assert!(patch.completer.director_model.is_none());
        });
    }

    #[test]
    fn cancel_writes_nothing() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let typed = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_model": "grok-4.6" }),
            );
            assert_eq!(
                handle(&press(form::CANCEL_ID), &typed, &view),
                Outcome::Reset,
            );
        });
    }

    /// #272: an exported variable owns the row, and `model::resolve` would
    /// give it the last word anyway — so the edit is refused here rather than
    /// written and then ignored.
    #[test]
    fn a_frozen_row_refuses_a_direct_write() {
        model::tests::with_env(None, Some("https://env.example"), None, || {
            let view = director_view();
            let description = form::describe();
            assert!(
                description.frozen(form::DIRECTOR_BASE_URL_ID),
                "precondition: the variable owns the URL row"
            );
            let event = Event::SetText {
                id: form::DIRECTOR_BASE_URL_ID.into(),
                value: "https://typed.example".into(),
            };
            assert_eq!(
                handle(&event, &AiDraft::live(&description), &view),
                Outcome::Nothing,
            );
        });
    }

    /// A frozen checkbox refuses writes, just like a frozen text row.
    #[test]
    #[cfg(target_os = "linux")]
    fn a_frozen_checkbox_refuses_writes() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            assert!(
                description.frozen(form::CAPTURABLE_ID),
                "precondition: capturable is frozen on Linux"
            );
            let event = Event::SetBool {
                id: form::CAPTURABLE_ID.into(),
                value: false,
            };
            assert_eq!(
                handle(&event, &AiDraft::live(&description), &view),
                Outcome::Nothing,
            );
        });
    }

    /// #452: re-picking what the list already shows is a save and a redraw for
    /// nothing, and on the source list it loses the current value.
    #[test]
    fn picking_the_value_already_shown_writes_nothing() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let shown = view
                .popup_value(form::CHARACTER_ID)
                .expect("the Character row is a list");
            let event = Event::Pick {
                id: form::CHARACTER_ID.into(),
                value: shown,
            };
            assert_eq!(
                handle(&event, &AiDraft::live(&description), &view),
                Outcome::Nothing,
            );
        });
    }

    /// A shortcut over a batched row fills the row and stops. Apply is what
    /// reaches the file, so a patch here would write half the tab behind the
    /// other half. #279.
    #[test]
    fn a_shortcut_over_a_batched_row_stages_rather_than_saves() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let shortcut = description
                .shortcut(form::DIRECTOR_BASE_URL_PICK_ID)
                .expect("the Base URL row has a shortcut list");
            let title = title_that_writes(&description, form::DIRECTOR_BASE_URL_PICK_ID);
            let event = Event::Shortcut {
                id: form::DIRECTOR_BASE_URL_PICK_ID.into(),
                value: title,
                current: String::new(),
            };
            let Outcome::Fill { id, patch, .. } =
                handle(&event, &AiDraft::live(&description), &view)
            else {
                panic!("a shortcut fills the row below it");
            };
            assert_eq!(id, shortcut.row);
            assert!(patch.is_none(), "a batched row waits for Apply");
        });
    }

    /// A title the list cannot spell — Custom — names nothing to write, so the
    /// row below keeps whatever it holds.
    #[test]
    fn a_shortcut_title_that_names_no_value_writes_nothing() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let event = Event::Shortcut {
                id: form::DIRECTOR_BASE_URL_PICK_ID.into(),
                value: "Custom".into(),
                current: String::new(),
            };
            assert_eq!(
                handle(&event, &AiDraft::live(&description), &view),
                Outcome::Nothing,
            );
        });
    }

    /// An id no description carries is not a silent no-op by accident: it is
    /// one on purpose, and the same answer a tag that lost its row gives.
    #[test]
    fn an_unknown_row_does_nothing() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let draft = AiDraft::live(&description);
            for event in [
                press("no_such_row"),
                Event::SetBool {
                    id: "no_such_row".into(),
                    value: true,
                },
                Event::SetText {
                    id: "no_such_row".into(),
                    value: "x".into(),
                },
            ] {
                assert_eq!(handle(&event, &draft, &view), Outcome::Nothing, "{event:?}");
            }
        });
    }

    /// A checkbox is an immediate row: the tick is a write, not a draft. The
    /// wire answer for that write is pinned in `main.rs` (#995); this is the
    /// decision it answers for.
    #[test]
    fn a_checkbox_tick_is_an_apply() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let draft = AiDraft::live(&description);
            let event = Event::SetBool {
                id: form::DND_ID.into(),
                value: true,
            };
            let mut expected = SettingsPatch::default();
            expected.set_bool(crate::settings::BoolField::DoNotDisturb, true);
            assert_eq!(handle(&event, &draft, &view), Outcome::Apply(expected));
        });
    }

    /// Marking a row batched in the form is all Apply needs to commit it.
    #[test]
    fn a_row_marked_batched_in_the_form_reaches_the_applied_patch() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let mut description = form::describe();
            let apply = |description: &FormDescription| {
                let draft = AiDraft::drawn(
                    description,
                    serde_json::json!({ form::DIRECTOR_TIMEOUT_SECS_ID: "90" }),
                );
                handle(&press(form::APPLY_ID), &draft, &view)
            };
            assert_eq!(
                apply(&description),
                Outcome::Commit(None),
                "an unbatched row saves on blur, so Apply leaves it alone"
            );
            for row in description
                .tabs
                .iter_mut()
                .flat_map(|tab| &mut tab.sections)
                .flat_map(|section| &mut section.rows)
            {
                if let form::FormRow::TextField { id, batched, .. } = row {
                    if id == form::DIRECTOR_TIMEOUT_SECS_ID {
                        *batched = true;
                    }
                }
            }
            let mut timeout = SettingsPatch::default();
            timeout.completer.director_timeout_secs = Some("90".into());
            assert_eq!(apply(&description), Outcome::Commit(Some(timeout)));
        });
    }

    /// A batched switch saves nothing on the tick. Cancel writes nothing and
    /// Apply commits the staged value.
    #[test]
    fn a_batched_checkbox_waits_for_apply() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let live = AiDraft::live(&description);
            for id in [
                form::DIRECTOR_ID,
                form::PROACTIVE_ID,
                form::PI_PROJECT_MCP_ID,
            ] {
                let event = Event::SetBool {
                    id: id.into(),
                    value: true,
                };
                assert_eq!(handle(&event, &live, &view), Outcome::Nothing, "{id}");
            }
            let toggled = AiDraft::drawn(
                &description,
                serde_json::json!({ "director": !view.director_enabled }),
            );
            assert_eq!(
                handle(&press(form::CANCEL_ID), &toggled, &view),
                Outcome::Reset
            );
            let Outcome::Commit(Some(patch)) = handle(&press(form::APPLY_ID), &toggled, &view)
            else {
                panic!("Apply commits the staged switch");
            };
            assert_eq!(
                patch.completer.director_enabled,
                Some(!view.director_enabled)
            );
            assert_eq!(
                handle(&press(form::APPLY_ID), &live, &view),
                Outcome::Commit(None)
            );
        });
    }

    /// New session carries no row, so a half-typed endpoint beside it survives
    /// the click (#679).
    #[test]
    fn a_new_session_touches_no_row_of_the_form() {
        model::tests::with_env(None, None, None, || {
            let view = director_view();
            let description = form::describe();
            let typed = AiDraft::drawn(
                &description,
                serde_json::json!({ "director_base_url": "https://half.typed" }),
            );
            let Outcome::Apply(patch) = handle(&press(form::NEW_SESSION_ID), &typed, &view) else {
                panic!("new session is a patch of its own");
            };
            assert!(patch.new_session);
            assert!(patch.completer.director_base_url.is_none());
        });
    }

    /// #663: a source pick stages. Apply is what reaches the file, so a
    /// patch here would kill the child Cancel is supposed to keep.
    #[test]
    fn picking_the_completer_source_stages_rather_than_saves() {
        model::tests::with_harness(None, || {
            let view = director_view();
            let description = form::describe();
            let event = Event::Pick {
                id: form::HARNESS_ID.into(),
                value: "Harness · opencode".into(),
            };
            assert_eq!(
                handle(&event, &AiDraft::live(&description), &view),
                Outcome::Nothing,
            );
            let typed = AiDraft::drawn(
                &description,
                serde_json::json!({ "harness": "Harness · opencode" }),
            );
            let Outcome::Commit(Some(patch)) = handle(&press(form::APPLY_ID), &typed, &view) else {
                panic!("Apply commits the staged source");
            };
            assert_eq!(patch.completer.harness.as_deref(), Some("opencode"));
            assert_eq!(
                handle(&press(form::CANCEL_ID), &typed, &view),
                Outcome::Reset,
            );
        });
    }

    #[test]
    fn a_batched_command_line_does_not_commit_on_blur() {
        model::tests::with_harness(None, || {
            let view = director_view();
            let description = form::describe();
            let event = Event::SetText {
                id: form::HARNESS_COMMAND_ID.into(),
                value: "hermes acp".into(),
            };
            assert_eq!(
                handle(&event, &AiDraft::live(&description), &view),
                Outcome::Nothing,
            );
        });
    }

    /// The first title on a shortcut list that names a value to write.
    fn title_that_writes(description: &FormDescription, id: &str) -> String {
        let shortcut = description.shortcut(id).expect("a shortcut list");
        description
            .sections()
            .flat_map(|section| section.rows.iter())
            .find_map(|row| match row {
                form::FormRow::Composite { controls, .. } => {
                    controls.iter().find_map(|control| match control {
                        form::CompositeControl::Popup {
                            id: popup, options, ..
                        } if popup == id => options
                            .iter()
                            .find(|title| (shortcut.value)(title).is_some())
                            .cloned(),
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("at least one title on the list names a value")
    }
}
