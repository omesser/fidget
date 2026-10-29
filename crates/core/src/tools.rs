//! Tool types and shared logic for the MCP server: Expression (speak, play a
//! Behavior), Sensing (list windows), Memory (recall, remember) and Identity
//! (list Instances). No tool posts input events (ADR-0003); a denylist filters sensing.

use serde::{Deserialize, Serialize};
use std::io;

use crate::display::Whereabouts;
use crate::engine::{BehaviorProposal, Point};
use crate::memory::MemoryManifest;
use crate::window_source::Rect;

/// Tool result for the `speak` tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeakResult {
    pub success: bool,
    pub message: String,
    /// Why the Expression did not land, when `success` is false: the bool alone
    /// cannot tell "I said it" from "there was nobody to say it to", and a stdio
    /// Harness runs against an empty roster. Absent on success, keeping the old shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Tool result for the `play_behavior` tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayBehaviorResult {
    pub success: bool,
    pub behavior: String,
    /// Why the Expression did not land; see [`SpeakResult::reason`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Live handle for enqueueing Expression proposals onto Character Instances.
pub trait ExpressionHandle {
    /// Enqueue a BehaviorProposal onto the Instance with the given id.
    /// Returns true if that id was live and the proposal was queued.
    fn enqueue(&mut self, instance_id: &str, proposal: BehaviorProposal) -> bool;
}

/// Tool result for the `list_windows` tool.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Absent without the window-names consent, which covers owner and title
    /// alike.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListWindowsResult {
    pub windows: Vec<WindowInfo>,
}

/// Tool result for the `describe_screen` tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescribeScreenResult {
    pub description: String,
}

/// Tool result for the `recall` tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallResult {
    pub content: String,
}

/// Tool result for the `remember` tool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberResult {
    pub recorded: String,
}

/// Tool result for the `list_instances` tool. The three prompts are the
/// Character Prompt layers the Prompt tab shows, so a session that never
/// heard its opening can read them back (ADR-0012).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub app_instructions: String,
    #[serde(default)]
    pub personality_prompt: String,
    #[serde(default)]
    pub instance_prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListInstancesResult {
    pub instances: Vec<InstanceInfo>,
}

/// Configuration for what to exclude from sensing results.
#[derive(Clone, Debug, Default)]
pub struct DenyList {
    /// Application names to exclude from sensing results.
    pub excluded_applications: Vec<String>,
    /// Whether to filter out password fields (always true in practice).
    pub filter_password_fields: bool,
}

impl DenyList {
    pub fn allows(&self, application: &str) -> bool {
        !self
            .excluded_applications
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(application))
    }
}

/// Recall everything Memory holds.
pub fn recall(memory: &MemoryManifest) -> io::Result<RecallResult> {
    let content = memory.recall()?;
    Ok(RecallResult { content })
}

/// Remember one fact under a heading.
pub fn remember(memory: &MemoryManifest, heading: &str, fact: &str) -> io::Result<RememberResult> {
    let recorded = memory.remember(heading, fact)?;
    Ok(RememberResult { recorded })
}

pub fn list_instances(instances: &[InstanceInfo]) -> ListInstancesResult {
    ListInstancesResult {
        instances: instances.to_vec(),
    }
}

/// One Instance's feet, for the whereabouts report.
pub struct FeetAt {
    pub id: String,
    pub name: String,
    pub at: Point,
}

/// One connected display. Origin and size are in `WhereaboutsReport::unit`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConnectedDisplay {
    pub index: usize,
    pub name: Option<String>,
    pub origin_x: f64,
    pub origin_y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where one Instance is. `x` and `y` are already in that display's frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstanceWhere {
    pub id: String,
    pub name: String,
    pub on_display: Option<usize>,
    pub other_displays: Vec<usize>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// What `whereabouts` returns. A caller does not convert coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WhereaboutsReport {
    pub unit: String,
    pub displays: Vec<ConnectedDisplay>,
    pub instances: Vec<InstanceWhere>,
}

/// Displays, which one each Instance is on, and its feet in that display.
pub fn whereabouts(
    frames: &[Rect],
    usable: &[Rect],
    names: &[Option<String>],
    instances: &[FeetAt],
) -> WhereaboutsReport {
    let displays = frames
        .iter()
        .enumerate()
        .map(|(index, frame)| ConnectedDisplay {
            index,
            name: names.get(index).and_then(Clone::clone),
            origin_x: frame.x,
            origin_y: frame.y,
            width: frame.width,
            height: frame.height,
        })
        .collect::<Vec<_>>();
    let every: Vec<usize> = (0..displays.len()).collect();
    let instances = instances
        .iter()
        .map(|spot| {
            let here = Whereabouts::locate(spot.at, frames, usable);
            let on_display = here.current().map(|display| display.index());
            let place = here.placement();
            InstanceWhere {
                id: spot.id.clone(),
                name: spot.name.clone(),
                on_display,
                other_displays: every
                    .iter()
                    .copied()
                    .filter(|index| Some(*index) != on_display)
                    .collect(),
                x: place.map(|place| place.x()),
                y: place.map(|place| place.y()),
            }
        })
        .collect();

    WhereaboutsReport {
        unit: "points".to_string(),
        displays,
        instances,
    }
}

mod helpers {
    use crate::engine::BehaviorProposal;
    use crate::tools::{DenyList, ExpressionHandle, InstanceInfo};
    use crate::window_source::{WindowRect, WindowSource};

    /// Denylist-filtered windows from the source, shared by `list_windows` and
    /// `describe_screen`.
    pub fn filtered_windows_snapshot(
        source: &dyn WindowSource,
        denylist: &DenyList,
    ) -> Vec<WindowRect> {
        let geometry = source.snapshot();
        geometry
            .windows
            .into_iter()
            // Without names consent there is no owner to match, so an exclusion
            // cannot apply and the window stays in as a rectangle. The list
            // exists to hide the name, and the consent has already withheld it.
            .filter(|w| {
                w.owner
                    .as_deref()
                    .is_none_or(|owner| denylist.allows(owner))
            })
            .collect()
    }

    /// Resolve the target Instance and enqueue, for `speak` and `play_behavior`.
    /// The `Err` is the whole answer a Harness gets about why the Expression
    /// landed nowhere, so it names the reason rather than a code.
    pub fn enqueue_expression(
        instance_id: Option<&str>,
        roster: &[InstanceInfo],
        expression: Option<&mut dyn ExpressionHandle>,
        proposal: BehaviorProposal,
    ) -> Result<(), String> {
        let target_id = match resolve_target_instance(instance_id, roster) {
            TargetResolution::Resolved(id) => id,
            TargetResolution::NoInstances => {
                return Err(
                    "No Character Instance is running, so nothing changed on screen".to_string(),
                );
            }
            TargetResolution::AmbiguousTarget => {
                return Err(
                    "Several Character Instances are running; name one with instance_id. \
                     Call list_instances to find each one's instance_id."
                        .to_string(),
                );
            }
        };

        // No handle is no way to reach the Instance the roster just named, so
        // the proposal lands nowhere however live that Instance is. The stdio
        // path runs this way today, which is why it cannot report success.
        let Some(handle) = expression else {
            return Err(
                "No live connection to the running app, so nothing changed on screen".to_string(),
            );
        };

        // The roster is a snapshot, so an Instance can retire between the
        // resolution above and this enqueue. Saying so is the same honesty the
        // empty roster gets: the proposal reached nobody either way.
        if !handle.enqueue(&target_id, proposal) {
            return Err(format!(
                "Character Instance {target_id} is no longer running, so nothing changed on screen"
            ));
        }

        Ok(())
    }

    /// Target resolution result for Expression tools.
    enum TargetResolution {
        Resolved(String),
        NoInstances,
        AmbiguousTarget,
    }

    /// Target resolution against roster for both speak and play_behavior.
    /// `dispatch` already refused an id the roster does not carry.
    fn resolve_target_instance(
        instance_id: Option<&str>,
        roster: &[InstanceInfo],
    ) -> TargetResolution {
        match instance_id {
            Some(id) => TargetResolution::Resolved(id.to_string()),
            None => match roster.len() {
                0 => TargetResolution::NoInstances,
                1 => TargetResolution::Resolved(roster[0].id.clone()),
                _ => TargetResolution::AmbiguousTarget,
            },
        }
    }
}

/// Make the Character speak a line of dialogue.
pub(crate) fn speak(
    message: &str,
    instance_id: Option<&str>,
    roster: &[InstanceInfo],
    expression: Option<&mut dyn ExpressionHandle>,
) -> SpeakResult {
    if message.is_empty() {
        return SpeakResult {
            success: false,
            message: message.to_string(),
            reason: Some("The message is empty".to_string()),
        };
    }

    let proposal = BehaviorProposal {
        behavior: String::new(),
        dialogue: Some(message.to_string()),
    };

    let reason = helpers::enqueue_expression(instance_id, roster, expression, proposal).err();

    SpeakResult {
        success: reason.is_none(),
        message: message.to_string(),
        reason,
    }
}

/// Play a named Behavior.
pub(crate) fn play_behavior(
    behavior: &str,
    instance_id: Option<&str>,
    roster: &[InstanceInfo],
    expression: Option<&mut dyn ExpressionHandle>,
) -> PlayBehaviorResult {
    if behavior.is_empty() {
        return PlayBehaviorResult {
            success: false,
            behavior: behavior.to_string(),
            reason: Some("The behavior name is empty".to_string()),
        };
    }

    let proposal = BehaviorProposal {
        behavior: behavior.to_string(),
        dialogue: None,
    };

    let reason = helpers::enqueue_expression(instance_id, roster, expression, proposal).err();

    PlayBehaviorResult {
        success: reason.is_none(),
        behavior: behavior.to_string(),
        reason,
    }
}

/// List visible windows with bounds, and their names under consent.
pub(crate) fn list_windows(
    window_source: &dyn crate::window_source::WindowSource,
    denylist: &DenyList,
) -> ListWindowsResult {
    let windows = helpers::filtered_windows_snapshot(window_source, denylist);
    ListWindowsResult {
        windows: windows
            .into_iter()
            .map(|w| WindowInfo {
                owner: w.owner,
                title: w.title,
                x: w.bounds.x,
                y: w.bounds.y,
                width: w.bounds.width,
                height: w.bounds.height,
            })
            .collect(),
    }
}

/// Describe what is on screen (v1: window metadata only).
pub(crate) fn describe_screen(
    window_source: &dyn crate::window_source::WindowSource,
    denylist: &DenyList,
) -> DescribeScreenResult {
    let windows = helpers::filtered_windows_snapshot(window_source, denylist);
    let description = if windows.is_empty() {
        "No windows are visible.".to_string()
    } else {
        let mut parts = vec![format!("{} visible windows:", windows.len())];
        for window in &windows {
            let title_part = window
                .title
                .as_ref()
                .map(|t| format!(" ({})", t))
                .unwrap_or_default();
            parts.push(format!(
                "- {}{} at ({:.0}, {:.0}), size {:.0}x{:.0}",
                window.owner.as_deref().unwrap_or("window"),
                title_part,
                window.bounds.x,
                window.bounds.y,
                window.bounds.width,
                window.bounds.height
            ));
        }
        parts.join("\n")
    };
    DescribeScreenResult { description }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_returns_empty_string_when_memory_is_empty() {
        let dir = tempfile::tempdir().expect("temp dir is creatable");
        let memory = MemoryManifest::new(dir.path().join("memory.md"));

        let result = recall(&memory).expect("recall succeeds");

        assert_eq!(result.content, "");
    }

    #[test]
    fn denylist_match_is_case_insensitive() {
        let denylist = DenyList {
            excluded_applications: vec!["1Password".to_string()],
            filter_password_fields: true,
        };

        assert!(!denylist.allows("1password"));
        assert!(!denylist.allows("1Password"));
        assert!(!denylist.allows("1PASSWORD"));
        assert!(denylist.allows("Terminal"));
    }

    #[test]
    fn describe_screen_when_no_windows_are_visible_returns_message() {
        use crate::window_source::{Capabilities, FakeWindowSource, Rect, WorldGeometry};

        let source = FakeWindowSource {
            capabilities: Capabilities {
                window_geometry: true,
                absolute_positioning: true,
            },
            geometry: WorldGeometry {
                usable_frames: vec![Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }],
                windows: vec![],
                dock: None,
            },
        };
        let denylist = DenyList::default();

        let result = describe_screen(&source, &denylist);

        assert_eq!(result.description, "No windows are visible.");
    }

    #[test]
    fn list_windows_includes_titles_when_present() {
        use crate::window_source::{
            Capabilities, FakeWindowSource, Rect, WindowRect, WorldGeometry,
        };

        let source = FakeWindowSource {
            capabilities: Capabilities {
                window_geometry: true,
                absolute_positioning: true,
            },
            geometry: WorldGeometry {
                usable_frames: vec![Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }],
                windows: vec![
                    WindowRect {
                        id: 1,
                        bounds: Rect {
                            x: 10.0,
                            y: 20.0,
                            width: 800.0,
                            height: 600.0,
                        },
                        owner: Some("Terminal".to_string()),
                        title: Some("bash".to_string()),
                        layer: 0,
                    },
                    WindowRect {
                        id: 2,
                        bounds: Rect {
                            x: 100.0,
                            y: 200.0,
                            width: 1200.0,
                            height: 800.0,
                        },
                        owner: Some("Safari".to_string()),
                        title: None,
                        layer: 0,
                    },
                ],
                dock: None,
            },
        };

        let result = list_windows(&source, &DenyList::default());

        assert_eq!(result.windows.len(), 2);
        assert_eq!(result.windows[0].owner, Some("Terminal".to_string()));
        assert_eq!(result.windows[0].title, Some("bash".to_string()));
        assert_eq!(result.windows[1].owner, Some("Safari".to_string()));
        assert_eq!(result.windows[1].title, None);
    }

    /// Without the window-names consent both names stay behind and the tools
    /// still report geometry. `owner` is omitted from the JSON rather than sent
    /// empty, so a Harness reads an absent name as absent.
    #[test]
    fn a_nameless_window_reports_its_geometry_and_no_owner() {
        use crate::window_source::{
            Capabilities, FakeWindowSource, Rect, WindowRect, WorldGeometry,
        };

        let source = FakeWindowSource {
            capabilities: Capabilities {
                window_geometry: true,
                absolute_positioning: true,
            },
            geometry: WorldGeometry {
                usable_frames: vec![Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }],
                windows: vec![WindowRect {
                    id: 1,
                    bounds: Rect {
                        x: 10.0,
                        y: 20.0,
                        width: 800.0,
                        height: 600.0,
                    },
                    owner: None,
                    title: None,
                    layer: 0,
                }],
                dock: None,
            },
        };

        let listed = list_windows(&source, &DenyList::default());
        assert_eq!(listed.windows.len(), 1);
        assert_eq!(listed.windows[0].owner, None);
        assert_eq!(listed.windows[0].width, 800.0);
        assert_eq!(
            serde_json::to_string(&listed.windows[0]).expect("the window serializes"),
            r#"{"x":10.0,"y":20.0,"width":800.0,"height":600.0}"#
        );

        assert_eq!(
            describe_screen(&source, &DenyList::default()).description,
            "1 visible windows:\n- window at (10, 20), size 800x600"
        );

        // An exclusion matches a name, and there is no name to match, so the
        // rectangle stays. The name it would have hidden is withheld already.
        let excluding = DenyList {
            excluded_applications: vec!["Terminal".to_string()],
            filter_password_fields: true,
        };
        assert_eq!(list_windows(&source, &excluding).windows.len(), 1);
    }

    #[test]
    fn windows_without_titles_are_not_dropped() {
        use crate::window_source::{
            Capabilities, FakeWindowSource, Rect, WindowRect, WorldGeometry,
        };

        let source = FakeWindowSource {
            capabilities: Capabilities {
                window_geometry: true,
                absolute_positioning: true,
            },
            geometry: WorldGeometry {
                usable_frames: vec![Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }],
                windows: vec![
                    WindowRect {
                        id: 1,
                        bounds: Rect {
                            x: 10.0,
                            y: 20.0,
                            width: 800.0,
                            height: 600.0,
                        },
                        owner: Some("Finder".to_string()),
                        title: None,
                        layer: 0,
                    },
                    WindowRect {
                        id: 2,
                        bounds: Rect {
                            x: 100.0,
                            y: 200.0,
                            width: 1200.0,
                            height: 800.0,
                        },
                        owner: Some("System Preferences".to_string()),
                        title: None,
                        layer: 0,
                    },
                ],
                dock: None,
            },
        };

        let result = list_windows(&source, &DenyList::default());

        assert_eq!(
            result.windows.len(),
            2,
            "consent on + missing title keys must not drop windows"
        );
        assert!(result.windows[0].title.is_none());
        assert!(result.windows[1].title.is_none());
    }

    #[test]
    fn describe_screen_includes_titles_in_parentheses() {
        use crate::window_source::{
            Capabilities, FakeWindowSource, Rect, WindowRect, WorldGeometry,
        };

        let source = FakeWindowSource {
            capabilities: Capabilities {
                window_geometry: true,
                absolute_positioning: true,
            },
            geometry: WorldGeometry {
                usable_frames: vec![Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }],
                windows: vec![
                    WindowRect {
                        id: 1,
                        bounds: Rect {
                            x: 10.0,
                            y: 20.0,
                            width: 800.0,
                            height: 600.0,
                        },
                        owner: Some("Terminal".to_string()),
                        title: Some("bash".to_string()),
                        layer: 0,
                    },
                    WindowRect {
                        id: 2,
                        bounds: Rect {
                            x: 100.0,
                            y: 200.0,
                            width: 1200.0,
                            height: 800.0,
                        },
                        owner: Some("Safari".to_string()),
                        title: None,
                        layer: 0,
                    },
                ],
                dock: None,
            },
        };

        let result = describe_screen(&source, &DenyList::default());

        assert!(result.description.contains("2 visible windows"));
        assert!(result.description.contains("Terminal (bash)"));
        assert!(result.description.contains("Safari at"));
        assert!(!result.description.contains("Safari ()"));
    }

    #[test]
    fn whereabouts_names_the_display_the_feet_are_on_and_the_others() {
        let frames = [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            Rect {
                x: 1920.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
        ];
        let names = [Some("Built-in".to_string()), None];
        let instances = [FeetAt {
            id: "pip".to_string(),
            name: "Pip".to_string(),
            at: Point {
                x: 2000.0,
                y: 100.0,
            },
        }];

        let report = whereabouts(&frames, &frames, &names, &instances);

        assert_eq!(report.unit, "points");
        assert_eq!(report.displays.len(), 2);
        assert_eq!(report.displays[0].name.as_deref(), Some("Built-in"));
        assert_eq!(report.displays[0].origin_x, 0.0);
        assert_eq!(report.displays[0].width, 1920.0);
        assert_eq!(report.displays[0].height, 1080.0);
        assert_eq!(report.displays[1].name, None);
        assert_eq!(report.displays[1].origin_x, 1920.0);
        assert_eq!(report.displays[1].origin_y, 0.0);
        let here = &report.instances[0];
        assert_eq!(here.on_display, Some(1));
        assert_eq!(here.other_displays, vec![0]);
        assert_eq!(here.x, Some(80.0));
        assert_eq!(here.y, Some(100.0));
    }

    #[test]
    fn whereabouts_off_every_display_lists_them_all_as_other() {
        let frames = [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 1080.0,
            },
            Rect {
                x: 1200.0,
                y: 0.0,
                width: 1000.0,
                height: 1080.0,
            },
        ];
        let instances = [FeetAt {
            id: "pip".to_string(),
            name: "Pip".to_string(),
            at: Point {
                x: 1100.0,
                y: 100.0,
            },
        }];

        let report = whereabouts(&frames, &frames, &[], &instances);
        let here = &report.instances[0];

        assert_eq!(here.on_display, None);
        assert_eq!(here.other_displays, vec![0, 1]);
        assert_eq!(here.x, None);
        assert_eq!(here.y, None);
    }
}
