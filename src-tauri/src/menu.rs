//! The context menu: what is in it, and how it reaches the screen.
//!
//! Split in two because the two halves have different constraints. What the
//! menu contains is a description made of owned Strings and bools — no native
//! handles, no platform types, `Send`. The frame-loop thread builds one of
//! those and can be tested doing it on any platform.
//!
//! Turning a description into a menu the window server draws is the other half,
//! and it can only happen on the main thread: the native objects behind it are
//! `Rc`-counted and not `Send`, so a worker thread cannot hold one even for as
//! long as it takes to hand it over. `show` therefore takes a description
//! rather than a menu, and is called from inside `run_on_main_thread`.
//!
//! Selections do not come back from the popup. It returns as soon as the menu
//! is on screen, and the click — if there is one — arrives later on the app's
//! menu event channel. `MenuAction` is looked up there, by id, through the
//! description's action table.

use std::collections::HashMap;

/// What a menu item does when it is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Character ▸ <name>. Switch to the named Character Package.
    SwitchCharacter(String),
    /// Spawn another fidget of the current Character.
    SpawnInstance,
    /// Quiet without hiding. Proposals stop; the Character stays on screen.
    ToggleDnd,
    /// Possess this Instance, or let it go.
    TogglePossession,
    /// Hide the Character instantly, same path as the hotkey.
    Hide,
    /// Move every on-screen Character onto the display the cursor is on.
    BringToThisDisplay,
    /// Open the settings window. Tray and sprite both reach it this way.
    OpenSettings,
    /// Open the Chat surface. The same action a Summon performs.
    Summon,
    /// Turn Caffeinate on or off. The same request the Chat button posts.
    ToggleCaffeinate,
    /// Leave. Ours, not `PredefinedMenuItem::quit`: that calls `terminate:`
    /// from inside the tray menu and deadlocks the overlay webviews.
    Quit,
}

/// Menu choices that yield the quick-message pill because the frame does not already show them.
///
/// Possession, Do Not Disturb, and Go away already change fields the overlay watches.
/// A Character switch changes the frame too, but the overlay only draws it.
pub fn quick_message_aside(action: &MenuAction) -> Option<&'static str> {
    match action {
        MenuAction::OpenSettings => Some("settings"),
        MenuAction::Summon => Some("chat"),
        MenuAction::SwitchCharacter(_) => Some("character"),
        MenuAction::SpawnInstance
        | MenuAction::ToggleDnd
        | MenuAction::TogglePossession
        | MenuAction::Hide
        | MenuAction::BringToThisDisplay
        | MenuAction::ToggleCaffeinate
        | MenuAction::Quit => None,
    }
}

/// Which surface is asking for the one menu. Bring to this display is the
/// only row that follows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuSurface {
    /// The menu bar icon.
    Tray,
    /// The sprite's right-click.
    Sprite,
}

/// Everything `describe` needs to draw one menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuSnapshot<'a> {
    pub surface: MenuSurface,
    pub installed: &'a [String],
    pub current_character: &'a str,
    pub instances: &'a [(String, String)],
    pub do_not_disturb: bool,
    /// The user possesses this Instance. The row's label follows it.
    pub possessed: bool,
    pub hidden: bool,
    pub caffeinate: CaffeinateRow,
}

/// The caffeinate row. `enabled` is false when every flag is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaffeinateRow {
    pub on: bool,
    pub enabled: bool,
}

/// One row of the menu, as data.
///
/// Ids are only on rows that can be chosen. A submenu is opened, a separator cannot be clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    /// A plain row. Disabled rows are still listed: the menu says what exists.
    Item {
        id: String,
        label: String,
        enabled: bool,
    },
    /// A row with a checkmark, which is how state is shown rather than told.
    Check {
        id: String,
        label: String,
        enabled: bool,
        checked: bool,
    },
    /// A row that opens another list.
    Submenu {
        label: String,
        items: Vec<MenuEntry>,
    },
}

/// The whole menu as data: the rows, and what the clickable ones do.
///
/// Everything here is owned so this crosses a thread boundary: built on the frame loop, drawn on the main thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuDescription {
    pub entries: Vec<MenuEntry>,
    pub actions: HashMap<String, MenuAction>,
}

const DND_ID: &str = "dnd";

const POSSESSION_ID: &str = "possession";

/// The row while this Instance is possessed. One string, so a later
/// label is one edit. "Release" already means letting go of a Grab.
const POSSESSED_LABEL: &str = "Release Possession";

const HIDE_ID: &str = "hide";

const BRING_ID: &str = "bring-to-display";

const CHAT_ID: &str = "chat";

const CAFFEINATE_ID: &str = "caffeinate";

const SETTINGS_ID: &str = "settings";

const SPAWN_ID: &str = "spawn";

/// The id of Quit. Owned here so the tray event hook and the action table
/// cannot drift onto different strings.
pub(crate) const QUIT_ID: &str = "quit";

/// Native id of Quit for one tray (or sprite) draw.
///
/// The native item uses this so replacing the tray mints a new id: muda can deliver the dropped item's teardown as a click.
pub(crate) fn quit_item_id(generation: u64) -> String {
    format!("{QUIT_ID}:{generation}")
}

/// Whether `id` is Quit on the menu that is showing now.
pub(crate) fn is_live_quit(id: &str, generation: u64) -> bool {
    id == quit_item_id(generation)
}

/// The id prefix for a Character row, so `character:bmo` cannot collide with a
/// package that happens to be called `hide`.
const CHARACTER_PREFIX: &str = "character:";

/// Describe the menu. Tray and sprite both call this.
///
/// Checkboxes report the Engine and settings rather than a copy that could drift.
pub fn describe(snapshot: MenuSnapshot<'_>) -> MenuDescription {
    let mut entries = Vec::new();
    let mut actions = HashMap::new();

    entries.push(MenuEntry::Item {
        id: CHAT_ID.to_string(),
        label: "Chat…".to_string(),
        enabled: true,
    });
    actions.insert(CHAT_ID.to_string(), MenuAction::Summon);

    // Omitted when nothing is installed rather than shown empty: an empty
    // submenu looks like a bug; no submenu says there is nothing to choose.
    if !snapshot.installed.is_empty() {
        let items = snapshot
            .installed
            .iter()
            .map(|name| {
                let id = format!("{CHARACTER_PREFIX}{name}");
                actions.insert(id.clone(), MenuAction::SwitchCharacter(name.clone()));
                MenuEntry::Check {
                    id,
                    label: name.clone(),
                    enabled: true,
                    checked: name == snapshot.current_character,
                }
            })
            .collect();

        entries.push(MenuEntry::Submenu {
            label: "Character".to_string(),
            items,
        });
    }

    // Instances ▸ — who is on screen, and New… to spawn another. Always
    // present: an empty list still has New…, which is how a dismissed last
    // character comes back without hunting settings.
    {
        let mut items: Vec<MenuEntry> = snapshot
            .instances
            .iter()
            .map(|(_, name)| MenuEntry::Item {
                id: format!("instance:{name}"),
                label: name.clone(),
                enabled: false,
            })
            .collect();
        items.push(MenuEntry::Item {
            id: SPAWN_ID.to_string(),
            label: "New…".to_string(),
            enabled: true,
        });
        actions.insert(SPAWN_ID.to_string(), MenuAction::SpawnInstance);
        entries.push(MenuEntry::Submenu {
            label: "Instances".to_string(),
            items,
        });
    }

    // The same Caffeinate the Chat button toggles.
    entries.push(MenuEntry::Check {
        id: CAFFEINATE_ID.to_string(),
        label: "Caffeinate".to_string(),
        enabled: snapshot.caffeinate.enabled,
        checked: snapshot.caffeinate.on,
    });
    actions.insert(CAFFEINATE_ID.to_string(), MenuAction::ToggleCaffeinate);

    entries.push(MenuEntry::Check {
        id: DND_ID.to_string(),
        label: "Do Not Disturb".to_string(),
        enabled: true,
        checked: snapshot.do_not_disturb,
    });
    actions.insert(DND_ID.to_string(), MenuAction::ToggleDnd);

    // Beside Do Not Disturb: a switch on this Instance, named by the action
    // it will take. A checkmark would hide which way the click goes.
    entries.push(MenuEntry::Item {
        id: POSSESSION_ID.to_string(),
        label: if snapshot.possessed {
            POSSESSED_LABEL.to_string()
        } else {
            "Possess".to_string()
        },
        enabled: true,
    });
    actions.insert(POSSESSION_ID.to_string(), MenuAction::TogglePossession);

    // The same flag the hotkey flips. The label says which way it is pointing
    // so a checkmark is not doing two jobs.
    entries.push(MenuEntry::Item {
        id: HIDE_ID.to_string(),
        label: if snapshot.hidden {
            "Come back".to_string()
        } else {
            "Go away".to_string()
        },
        enabled: true,
    });
    actions.insert(HIDE_ID.to_string(), MenuAction::Hide);

    // A sprite you can right-click is already on this display.
    if snapshot.surface == MenuSurface::Tray {
        entries.push(MenuEntry::Item {
            id: BRING_ID.to_string(),
            label: "Bring to this display".to_string(),
            enabled: true,
        });
        actions.insert(BRING_ID.to_string(), MenuAction::BringToThisDisplay);
    }

    entries.push(MenuEntry::Item {
        id: SETTINGS_ID.to_string(),
        label: "Settings…".to_string(),
        enabled: true,
    });
    actions.insert(SETTINGS_ID.to_string(), MenuAction::OpenSettings);

    // What the character can see lives in Settings, not the menu. A row here
    // would look like a prompt on right-click, which decision 9 refuses.

    entries.push(MenuEntry::Item {
        id: QUIT_ID.to_string(),
        label: "Quit".to_string(),
        enabled: true,
    });
    actions.insert(QUIT_ID.to_string(), MenuAction::Quit);

    MenuDescription { entries, actions }
}

/// The next tray draw, if this description is not the one already showing.
///
/// Settings can dismiss or spawn without a menu click, so "did a row change" is the gate.
pub fn replace_if_changed(
    previous: &mut Option<MenuDescription>,
    next: MenuDescription,
) -> Option<MenuDescription> {
    if previous.as_ref() == Some(&next) {
        None
    } else {
        *previous = Some(next.clone());
        Some(next)
    }
}

/// Build the native menu from a description.
///
/// Must be called on the main thread: constructors reach the window server, and the objects they return are not `Send`.
pub fn build(
    app: &tauri::AppHandle,
    description: &MenuDescription,
    quit_generation: u64,
) -> Result<tauri::menu::Menu<tauri::Wry>, tauri::Error> {
    use tauri::menu::{CheckMenuItem, Menu, MenuItem, Submenu};

    // Built one item at a time rather than with `with_items`, because the rows
    // are of three different types and a Vec of them needs boxing either way.
    let menu = Menu::new(app)?;
    let native_id = |id: &str| {
        if id == QUIT_ID {
            quit_item_id(quit_generation)
        } else {
            id.to_string()
        }
    };

    for entry in &description.entries {
        match entry {
            MenuEntry::Item { id, label, enabled } => {
                let item = MenuItem::with_id(app, native_id(id), label, *enabled, None::<&str>)?;
                menu.append(&item)?;
            }
            MenuEntry::Check {
                id,
                label,
                enabled,
                checked,
            } => {
                let item =
                    CheckMenuItem::with_id(app, id, label, *enabled, *checked, None::<&str>)?;
                menu.append(&item)?;
            }
            MenuEntry::Submenu { label, items } => {
                let submenu = Submenu::new(app, label, true)?;
                for item in items {
                    match item {
                        MenuEntry::Check {
                            id,
                            label,
                            enabled,
                            checked,
                        } => {
                            let child = CheckMenuItem::with_id(
                                app,
                                id,
                                label,
                                *enabled,
                                *checked,
                                None::<&str>,
                            )?;
                            submenu.append(&child)?;
                        }
                        MenuEntry::Item { id, label, enabled } => {
                            let child = MenuItem::with_id(app, id, label, *enabled, None::<&str>)?;
                            submenu.append(&child)?;
                        }
                        // One level is all the menu has. Nesting a submenu
                        // inside one is not something `describe` builds.
                        MenuEntry::Submenu { .. } => {}
                    }
                }
                menu.append(&submenu)?;
            }
        }
    }

    Ok(menu)
}

/// Pop the described menu over `window`.
///
/// `position` is logical points from the window's top-left. Returns as soon as the menu is on screen; the selection arrives later on the menu event channel.
pub fn show(
    app: &tauri::AppHandle,
    description: &MenuDescription,
    window_label: &str,
    position: tauri::LogicalPosition<f64>,
    quit_generation: u64,
) -> Result<(), tauri::Error> {
    use tauri::Manager;

    let menu = build(app, description, quit_generation)?;
    let window = app
        .get_webview_window(window_label)
        .ok_or(tauri::Error::WindowNotFound)?;

    #[cfg(target_os = "macos")]
    crate::platform::deliver_pending_press();
    window.popup_menu_at(&menu, position)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(of: &[&str]) -> Vec<String> {
        of.iter().map(|name| (*name).to_string()).collect()
    }

    fn snapshot<'a>(
        installed: &'a [String],
        current: &'a str,
        do_not_disturb: bool,
    ) -> MenuSnapshot<'a> {
        MenuSnapshot {
            surface: MenuSurface::Sprite,
            installed,
            current_character: current,
            instances: &[],
            do_not_disturb,
            possessed: false,
            hidden: false,
            caffeinate: CaffeinateRow {
                on: false,
                enabled: true,
            },
        }
    }

    #[test]
    fn chat_settings_or_a_character_yields_the_pill() {
        assert_eq!(
            quick_message_aside(&MenuAction::OpenSettings),
            Some("settings")
        );
        assert_eq!(quick_message_aside(&MenuAction::Summon), Some("chat"));
        // The overlay draws the new Character but does not read it as a choice.
        assert_eq!(
            quick_message_aside(&MenuAction::SwitchCharacter("bmo".to_string())),
            Some("character")
        );
    }

    #[test]
    fn possession_dnd_and_go_away_ride_the_frame_instead() {
        assert_eq!(quick_message_aside(&MenuAction::TogglePossession), None);
        assert_eq!(quick_message_aside(&MenuAction::ToggleDnd), None);
        assert_eq!(quick_message_aside(&MenuAction::Hide), None);
        assert_eq!(quick_message_aside(&MenuAction::ToggleCaffeinate), None);
        assert_eq!(quick_message_aside(&MenuAction::SpawnInstance), None);
        assert_eq!(quick_message_aside(&MenuAction::BringToThisDisplay), None);
        assert_eq!(quick_message_aside(&MenuAction::Quit), None);
    }

    /// The description exists so it can cross to the main thread. A compile-time
    /// `Send` check: a native handle in a `MenuEntry` would make the menu unsendable.
    #[test]
    fn a_description_can_be_sent_to_another_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<MenuDescription>();
        assert_send::<MenuEntry>();
        assert_send::<MenuAction>();

        // And genuinely crosses one, so the bound is not just asserted.
        let installed = names(&["bmo"]);
        let description = describe(snapshot(&installed, "bmo", true));
        let moved = std::thread::spawn(move || description.entries.len());

        assert!(moved.join().expect("the thread panicked") > 0);
    }

    fn entry_with_id<'a>(description: &'a MenuDescription, id: &str) -> Option<&'a MenuEntry> {
        description.entries.iter().find(|entry| match entry {
            MenuEntry::Item { id: got, .. } | MenuEntry::Check { id: got, .. } => got == id,
            _ => false,
        })
    }

    fn top_rows(description: &MenuDescription) -> Vec<(String, Option<bool>)> {
        description
            .entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item { label, enabled, .. }
                | MenuEntry::Check { label, enabled, .. } => (label.clone(), Some(*enabled)),
                MenuEntry::Submenu { label, .. } => (label.clone(), None),
            })
            .collect()
    }

    #[test]
    fn sprite_and_tray_share_every_row_except_bring_to_this_display() {
        let installed = names(&["bmo"]);
        let sprite = describe(snapshot(&installed, "bmo", false));
        let expected = vec![
            ("Chat…".to_string(), Some(true)),
            ("Character".to_string(), None),
            ("Instances".to_string(), None),
            ("Caffeinate".to_string(), Some(true)),
            ("Do Not Disturb".to_string(), Some(true)),
            ("Possess".to_string(), Some(true)),
            ("Go away".to_string(), Some(true)),
            ("Settings…".to_string(), Some(true)),
            ("Quit".to_string(), Some(true)),
        ];
        assert_eq!(top_rows(&sprite), expected);
        let mut tray_snap = snapshot(&installed, "bmo", false);
        tray_snap.surface = MenuSurface::Tray;
        let mut tray_rows = expected.clone();
        tray_rows.insert(7, ("Bring to this display".to_string(), Some(true)));
        let tray = describe(tray_snap);
        assert_eq!(top_rows(&tray), tray_rows);
        assert_eq!(
            tray.actions.get("bring-to-display"),
            Some(&MenuAction::BringToThisDisplay)
        );
        assert_eq!(sprite.actions.get("bring-to-display"), None);
    }

    fn instance_items(description: &MenuDescription) -> Option<&Vec<MenuEntry>> {
        description.entries.iter().find_map(|entry| match entry {
            MenuEntry::Submenu { label, items } if label == "Instances" => Some(items),
            _ => None,
        })
    }

    fn instance_labels(description: &MenuDescription) -> Vec<&str> {
        instance_items(description)
            .into_iter()
            .flatten()
            .filter_map(|entry| match entry {
                MenuEntry::Item { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect()
    }

    fn character_items(description: &MenuDescription) -> Option<&Vec<MenuEntry>> {
        description.entries.iter().find_map(|entry| match entry {
            MenuEntry::Submenu { label, items } if label == "Character" => Some(items),
            _ => None,
        })
    }

    #[test]
    fn chat_is_listed_and_enabled() {
        let description = describe(snapshot(&[], "bmo", false));

        assert_eq!(
            entry_with_id(&description, "chat"),
            Some(&MenuEntry::Item {
                id: "chat".to_string(),
                label: "Chat…".to_string(),
                enabled: true,
            }),
            "Chat is present and enabled"
        );
        assert_eq!(
            description.actions.get("chat"),
            Some(&MenuAction::Summon),
            "and choosing it Summons"
        );
    }

    /// An empty submenu is a dead end that reads as a bug. Nothing installed is
    /// said by there being nothing to open.
    #[test]
    fn no_character_submenu_when_nothing_is_installed() {
        let description = describe(snapshot(&[], "bmo", false));

        assert!(
            character_items(&description).is_none(),
            "no Character submenu when nothing is installed"
        );
    }

    #[test]
    fn the_character_submenu_lists_every_installed_package() {
        let installed = names(&["bmo", "nim", "cat"]);
        let description = describe(snapshot(&installed, "bmo", false));

        let labels: Vec<&str> = character_items(&description)
            .expect("a submenu")
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Check { label, .. } => Some(label.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(labels, vec!["bmo", "nim", "cat"]);
    }

    #[test]
    fn only_the_current_character_is_check_marked() {
        let installed = names(&["bmo", "nim"]);
        let description = describe(snapshot(&installed, "nim", false));

        let checked: Vec<(&str, bool)> = character_items(&description)
            .expect("a submenu")
            .iter()
            .filter_map(|entry| match entry {
                MenuEntry::Check { label, checked, .. } => Some((label.as_str(), *checked)),
                _ => None,
            })
            .collect();

        assert_eq!(checked, vec![("bmo", false), ("nim", true)]);
    }

    /// The row names the switch. Possessed is not a checkmark: the label is
    /// the action, the way Go away becomes Come back.
    #[test]
    fn the_possession_row_says_possess_and_then_release() {
        let installed = names(&["bmo"]);
        let before = describe(snapshot(&installed, "bmo", false));
        assert_eq!(
            entry_with_id(&before, "possession"),
            Some(&MenuEntry::Item {
                id: "possession".to_string(),
                label: "Possess".to_string(),
                enabled: true,
            })
        );
        assert_eq!(
            before.actions.get("possession"),
            Some(&MenuAction::TogglePossession)
        );

        let mut possessed = snapshot(&installed, "bmo", false);
        possessed.possessed = true;
        let during = describe(possessed);
        assert_eq!(
            entry_with_id(&during, "possession"),
            Some(&MenuEntry::Item {
                id: "possession".to_string(),
                label: "Release Possession".to_string(),
                enabled: true,
            })
        );
        assert_eq!(
            during.actions.get("possession"),
            Some(&MenuAction::TogglePossession)
        );
    }

    /// The checkbox reports the Engine, so opening the menu twice around a
    /// toggle shows the toggle.
    #[test]
    fn the_dnd_checkbox_reports_the_engine() {
        let off = describe(snapshot(&[], "bmo", false));
        let on = describe(snapshot(&[], "bmo", true));

        assert_eq!(
            entry_with_id(&off, "dnd"),
            Some(&MenuEntry::Check {
                id: "dnd".to_string(),
                label: "Do Not Disturb".to_string(),
                enabled: true,
                checked: false,
            })
        );
        assert!(
            matches!(
                entry_with_id(&on, "dnd"),
                Some(MenuEntry::Check { checked: true, .. })
            ),
            "checked once the Engine is quiet"
        );
    }

    #[test]
    fn caffeinate_is_a_tick() {
        let description = describe(snapshot(&[], "bmo", false));
        assert_eq!(
            entry_with_id(&description, "caffeinate"),
            Some(&MenuEntry::Check {
                id: "caffeinate".to_string(),
                label: "Caffeinate".to_string(),
                enabled: true,
                checked: false,
            })
        );
        assert_eq!(
            description.actions.get("caffeinate"),
            Some(&MenuAction::ToggleCaffeinate)
        );

        let mut on = snapshot(&[], "bmo", false);
        on.caffeinate = CaffeinateRow {
            on: true,
            enabled: true,
        };
        assert!(matches!(
            entry_with_id(&describe(on), "caffeinate"),
            Some(MenuEntry::Check { checked: true, .. })
        ));
    }

    /// Consent lives in settings, not here. A menu row would look like a prompt on right-click, which decision 9 refuses.
    #[test]
    fn the_consent_row_is_absent_rather_than_disabled() {
        let installed = names(&["bmo"]);
        let description = describe(snapshot(&installed, "bmo", false));

        let mentions_seeing = description.entries.iter().any(|entry| match entry {
            MenuEntry::Item { label, .. } | MenuEntry::Check { label, .. } => label.contains("see"),
            MenuEntry::Submenu { label, .. } => label.contains("see"),
        });

        assert!(
            !mentions_seeing,
            "nothing claims to show what the character can see"
        );
    }

    /// Quit is a row we handle, not the platform's `terminate:`. That call
    /// deadlocks the overlay webviews when it runs from inside the tray menu.
    #[test]
    fn quit_maps_to_an_action() {
        let description = describe(snapshot(&[], "bmo", false));

        assert_eq!(
            entry_with_id(&description, "quit"),
            Some(&MenuEntry::Item {
                id: "quit".to_string(),
                label: "Quit".to_string(),
                enabled: true,
            })
        );
        assert_eq!(description.actions.get("quit"), Some(&MenuAction::Quit));
    }

    #[test]
    fn a_replaced_tray_quit_is_not_the_live_one() {
        assert_ne!(
            quit_item_id(1),
            quit_item_id(2),
            "each tray draw must mint a new Quit id"
        );
        assert!(
            is_live_quit(&quit_item_id(2), 2),
            "the showing menu's Quit is live"
        );
        assert!(
            !is_live_quit(&quit_item_id(1), 2),
            "the replaced menu's Quit must not leave"
        );
    }

    /// Two Instances, then one: the remaining character is still on the menu, and
    /// Quit is still Quit rather than hanging off a row that was dismissed.
    #[test]
    fn dismissing_one_of_two_instances_leaves_the_other_and_quit() {
        let installed = names(&["bmo", "trump"]);
        let two = [
            ("id-pip".to_string(), "Pip".to_string()),
            ("id-trump".to_string(), "trump".to_string()),
        ];
        let mut snap = snapshot(&installed, "bmo", false);
        snap.instances = &two;
        let before = describe(snap.clone());

        let one = [("id-trump".to_string(), "trump".to_string())];
        snap.instances = &one;
        let after = describe(snap);

        assert_eq!(instance_labels(&after), ["trump", "New…"]);
        assert_eq!(
            after.actions.get(QUIT_ID),
            Some(&MenuAction::Quit),
            "Quit survives a dismiss"
        );
        assert_eq!(
            before.actions.get(QUIT_ID),
            after.actions.get(QUIT_ID),
            "dismiss must not retarget Quit"
        );
    }

    fn clickable_ids(description: &MenuDescription) -> Vec<&String> {
        fn walk<'a>(entries: &'a [MenuEntry], out: &mut Vec<&'a String>) {
            for entry in entries {
                match entry {
                    MenuEntry::Item { id, enabled, .. } if *enabled => out.push(id),
                    MenuEntry::Check { id, enabled, .. } if *enabled => out.push(id),
                    MenuEntry::Submenu { items, .. } => walk(items, out),
                    _ => {}
                }
            }
        }
        let mut ids = Vec::new();
        walk(&description.entries, &mut ids);
        ids
    }

    /// The contract between the two halves: every row that can be chosen has an
    /// action under the same id the native item will report.
    #[test]
    fn every_clickable_row_maps_to_an_action() {
        let installed = names(&["bmo", "nim"]);
        let description = describe(snapshot(&installed, "bmo", true));

        for id in clickable_ids(&description) {
            assert!(
                description.actions.contains_key(id.as_str()),
                "{id} is enabled and has no action"
            );
        }

        assert_eq!(
            description.actions.get("character:nim"),
            Some(&MenuAction::SwitchCharacter("nim".to_string())),
            "and a Character row switches to its own Character"
        );
        assert_eq!(description.actions.get("dnd"), Some(&MenuAction::ToggleDnd));
        assert_eq!(description.actions.get("hide"), Some(&MenuAction::Hide));
        assert_eq!(
            description.actions.get("settings"),
            Some(&MenuAction::OpenSettings)
        );
        assert_eq!(description.actions.get("director"), None);
        assert_eq!(description.actions.get("memory"), None);
    }

    /// A package called `hide` must not become the Hide row.
    #[test]
    fn a_character_named_like_a_row_does_not_collide_with_it() {
        let installed = names(&["hide", "dnd", "quit"]);
        let description = describe(snapshot(&installed, "hide", false));

        assert_eq!(description.actions.get("hide"), Some(&MenuAction::Hide));
        assert_eq!(
            description.actions.get("character:hide"),
            Some(&MenuAction::SwitchCharacter("hide".to_string()))
        );
        assert_eq!(description.actions.get("dnd"), Some(&MenuAction::ToggleDnd));
        assert_eq!(description.actions.get("quit"), Some(&MenuAction::Quit));
        assert_eq!(
            description.actions.get("character:quit"),
            Some(&MenuAction::SwitchCharacter("quit".to_string()))
        );
    }

    /// Go away / Come back is the same flag as the hotkey, so the label has
    /// to say which way it is pointing.
    #[test]
    fn hide_says_come_back_when_the_character_is_away() {
        let installed = names(&["bmo"]);
        let mut away = snapshot(&installed, "bmo", false);
        away.hidden = true;
        let description = describe(away);

        assert_eq!(
            entry_with_id(&description, "hide"),
            Some(&MenuEntry::Item {
                id: "hide".to_string(),
                label: "Come back".to_string(),
                enabled: true,
            })
        );
    }

    /// The tray only redraws when this says the description changed. A
    /// dismiss that does not pass through a menu click still has to.
    #[test]
    fn a_dismissed_instance_is_a_menu_that_must_be_pushed() {
        let installed = names(&["bmo"]);
        let two = [
            ("id-nim".to_string(), "Nim".to_string()),
            ("id-bmo".to_string(), "BMO".to_string()),
        ];
        let mut snap = snapshot(&installed, "bmo", false);
        snap.instances = &two;
        let before = describe(snap.clone());

        let one = [("id-nim".to_string(), "Nim".to_string())];
        snap.instances = &one;
        let after = describe(snap);

        assert_eq!(instance_labels(&before), ["Nim", "BMO", "New…"]);
        assert_eq!(instance_labels(&after), ["Nim", "New…"]);

        let mut last = Some(before);
        let pushed = replace_if_changed(&mut last, after);
        assert!(pushed.is_some(), "a dismissed Instance is a different menu");
        assert_eq!(
            instance_labels(pushed.as_ref().expect("pushed")),
            ["Nim", "New…"]
        );
    }

    #[test]
    fn an_unchanged_menu_is_not_pushed_again() {
        let installed = names(&["bmo"]);
        let first = describe(snapshot(&installed, "bmo", false));
        let again = describe(snapshot(&installed, "bmo", false));
        let mut last = Some(first);
        assert!(
            replace_if_changed(&mut last, again).is_none(),
            "the same rows are not a rebuild"
        );
    }
}
