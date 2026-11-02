//! MCP tool dispatch: tool names and JSON arguments onto the handlers in
//! `tools`. Tested in-process, with no MCP transport, so it can live in core.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::num::NonZeroU32;
use std::path::PathBuf;

use crate::caffeinate::{CaffeinateHandle, Fact};
use crate::display::Placement;
use crate::engine::{Direction, Distance, Extent, MoveRequest, ARRIVAL_THRESHOLD, WALK_SPEED};
use crate::memory::MemoryManifest;
use crate::tools;
use crate::window_source::{Rect, WindowSource};

pub use crate::tools::{DenyList, ExpressionHandle, FeetAt, InstanceInfo};

/// The displays and feet a `whereabouts` call reports. Empty when the caller
/// has no desktop.
pub struct PlacementQuery<'a> {
    pub frames: &'a [Rect],
    pub usable: &'a [Rect],
    pub names: &'a [Option<String>],
    pub instances: &'a [FeetAt],
}

impl PlacementQuery<'static> {
    /// No displays and no Instances.
    pub fn empty() -> Self {
        Self {
            frames: &[],
            usable: &[],
            names: &[],
            instances: &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    UnknownTool,
    InvalidArguments,
    ExecutionFailed,
}

pub struct DispatchContext<'a, 'c> {
    pub window_source: &'a dyn WindowSource,
    pub memory_path: PathBuf,
    pub denylist: DenyList,
    pub roster: &'a [InstanceInfo],
    pub expression: Option<&'a mut dyn ExpressionHandle>,
    pub placement: PlacementQuery<'a>,
    /// Its own lifetime, because Caffeinate outlives this call's window lists.
    /// `None` where this build has no Caffeinate, and the tool errors.
    pub caffeinate: Option<&'c mut dyn CaffeinateHandle>,
}

/// Reborrow a taken handle for one call. `as_deref_mut` on `Option<&mut dyn>`
/// is stuck at the inner lifetime, so the handle could not be stored back.
fn as_expression_handle<'short>(
    expression: &'short mut Option<&mut dyn ExpressionHandle>,
) -> Option<&'short mut dyn ExpressionHandle> {
    match expression {
        Some(handle) => Some(&mut **handle),
        None => None,
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(
    arguments: Value,
    tool_name: &str,
) -> Result<T, DispatchError> {
    serde_json::from_value(arguments).map_err(|e| DispatchError {
        code: ErrorCode::InvalidArguments,
        message: format!("Invalid arguments for {}: {}", tool_name, e),
    })
}

/// The wire spelling of a walk's `direction`. Parsed here so the Engine's
/// `Direction` stays free of a JSON shape.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum DirectionArg {
    Left,
    Right,
}

impl From<DirectionArg> for Direction {
    fn from(arg: DirectionArg) -> Self {
        match arg {
            DirectionArg::Left => Self::Left,
            DirectionArg::Right => Self::Right,
        }
    }
}

/// The wire shape of a `move` call. Every field is optional because which ones
/// may come together is the parse's to say, in words a caller can act on.
#[derive(Deserialize)]
struct MoveArgs {
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    direction: Option<DirectionArg>,
    #[serde(default)]
    distance: Option<f64>,
    #[serde(default)]
    duration_ms: Option<u32>,
    #[serde(default)]
    to: Option<SpotArg>,
}

/// A spot as `whereabouts` reports one. `y` is required so that spot passes
/// back unchanged, and a walk never reads it.
#[derive(Clone, Copy, Deserialize)]
struct SpotArg {
    display: usize,
    x: f64,
    y: f64,
}

impl MoveArgs {
    /// One `MoveRequest`, or the sentence that says what to change. `frames`
    /// are the full frames `whereabouts` measures a spot from.
    fn request(&self, frames: &[Rect]) -> Result<MoveRequest, String> {
        let along = |heading: DirectionArg, extent| MoveRequest::Along {
            heading: heading.into(),
            extent,
        };
        let refuse = |why: &str| Err(why.to_string());
        match (self.direction, self.distance, self.duration_ms, self.to) {
            (Some(heading), Some(points), None, None) => match Distance::new(points) {
                Some(distance) => Ok(along(heading, Extent::Distance(distance))),
                None => refuse("distance must be more than 0 points."),
            },
            (Some(heading), None, Some(ms), None) => match NonZeroU32::new(ms) {
                Some(ms) => Ok(along(heading, Extent::Time { ms })),
                None => refuse("duration_ms must be more than 0."),
            },
            (None, None, None, Some(spot)) => Placement::at(spot.display, spot.x, spot.y)
                .engine_x(frames)
                .map(MoveRequest::To),
            (_, _, _, Some(_)) => {
                refuse("Pass either to, or direction with distance or duration_ms. Not both.")
            }
            (_, Some(_), Some(_), None) => refuse("Pass distance or duration_ms, not both."),
            (Some(_), None, None, None) => {
                refuse("Say how far. Pass distance or duration_ms with direction.")
            }
            (None, Some(_), None, None) | (None, None, Some(_), None) => {
                refuse("distance and duration_ms need a direction, left or right.")
            }
            (None, None, None, None) => {
                refuse("Say where. Pass direction with distance or duration_ms, or pass to.")
            }
        }
    }
}

fn unknown_instance(id: &str, roster: &[InstanceInfo]) -> String {
    if roster.is_empty() {
        return format!("No Character Instance has id {id}. No Character Instance is running.");
    }
    let running = roster
        .iter()
        .map(|info| format!("{} ({})", info.id, info.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "No Character Instance has id {id}. Running: {running}. \
         Pass your own instance_id from your prompt, or call list_instances to find it."
    )
}

pub fn dispatch(
    tool_name: &str,
    arguments: Value,
    context: &mut DispatchContext,
) -> Result<Value, DispatchError> {
    // One gate for every tool, so a tool added later refuses a stray id too.
    if let Some(given) = arguments
        .get("instance_id")
        .filter(|given| !given.is_null())
    {
        let Some(id) = given.as_str() else {
            return Err(DispatchError {
                code: ErrorCode::InvalidArguments,
                message: "instance_id must be a string".to_string(),
            });
        };
        if !context.roster.iter().any(|info| info.id == id) {
            return Err(DispatchError {
                code: ErrorCode::InvalidArguments,
                message: unknown_instance(id, context.roster),
            });
        }
    }
    match tool_name {
        "play_behavior" => {
            #[derive(Deserialize)]
            struct Args {
                behavior: String,
                #[serde(default)]
                instance_id: Option<String>,
                #[serde(default)]
                direction: Option<DirectionArg>,
            }
            let args: Args = parse_args(arguments, tool_name)?;

            let mut expression = context.expression.take();
            let result = tools::play_behavior(
                &args.behavior,
                args.direction.map(Direction::from),
                args.instance_id.as_deref(),
                context.roster,
                as_expression_handle(&mut expression),
            );
            context.expression = expression;

            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "move" => {
            let args: MoveArgs = parse_args(arguments, tool_name)?;
            let request =
                args.request(context.placement.frames)
                    .map_err(|message| DispatchError {
                        code: ErrorCode::InvalidArguments,
                        message,
                    })?;

            let mut expression = context.expression.take();
            let result = tools::move_instance(
                request,
                args.instance_id.as_deref(),
                context.roster,
                as_expression_handle(&mut expression),
            );
            context.expression = expression;

            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "list_windows" => {
            let result = tools::list_windows(context.window_source, &context.denylist);
            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "describe_screen" => {
            let result = tools::describe_screen(context.window_source, &context.denylist);
            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "recall" => {
            let memory = MemoryManifest::new(&context.memory_path);
            let result = tools::recall(&memory).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to recall: {}", e),
            })?;
            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "remember" => {
            #[derive(Deserialize)]
            struct Args {
                heading: String,
                fact: String,
            }
            let args: Args = parse_args(arguments, tool_name)?;
            let memory = MemoryManifest::new(&context.memory_path);
            let result =
                tools::remember(&memory, &args.heading, &args.fact).map_err(|e| DispatchError {
                    code: ErrorCode::ExecutionFailed,
                    message: format!("Failed to remember: {}", e),
                })?;
            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "list_instances" => {
            let result = tools::list_instances(context.roster);
            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "whereabouts" => {
            let result = tools::whereabouts(
                context.placement.frames,
                context.placement.usable,
                context.placement.names,
                context.placement.instances,
            );
            serde_json::to_value(&result).map_err(|e| DispatchError {
                code: ErrorCode::ExecutionFailed,
                message: format!("Failed to serialize result: {}", e),
            })
        }
        "caffeinate" => {
            #[derive(Deserialize)]
            struct Args {
                on: Option<bool>,
            }
            let args: Args = parse_args(arguments, tool_name)?;
            let Some(handle) = context.caffeinate.as_mut() else {
                return Err(DispatchError {
                    code: ErrorCode::ExecutionFailed,
                    message: "Caffeinate is not available on this platform.".to_string(),
                });
            };
            match handle.call(args.on) {
                Ok(Fact::Off) => Ok(serde_json::json!({ "on": false })),
                Ok(Fact::On(flags)) => {
                    Ok(serde_json::json!({ "on": true, "flags": flags.inhibitor_label() }))
                }
                Err(message) => Err(DispatchError {
                    code: ErrorCode::ExecutionFailed,
                    message,
                }),
            }
        }
        _ => Err(DispatchError {
            code: ErrorCode::UnknownTool,
            message: format!("Unknown tool: {}", tool_name),
        }),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// The tools in `tools`, and none that post input events.
pub fn list_tools() -> Vec<ToolInfo> {
    let mut tools = vec![
        ToolInfo {
            name: "play_behavior".to_string(),
            description: "Play a named Behavior. Pass direction to aim a walk left or right instead of the way the Character last faced.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "behavior": {
                        "type": "string",
                        "description": "The name of the Behavior to play"
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["left", "right"],
                        "description": "Which way a walk in the Behavior goes: left or right of the screen. Omit it to keep the heading the Character last had. A Behavior that does not walk ignores it."
                    }
                },
                "required": ["behavior"]
            }),
        },
        ToolInfo {
            name: "move".to_string(),
            description: format!(
                "Walk a Character Instance left or right along the display floor or window top it stands on. \
                 Pass direction with distance in points or duration_ms in milliseconds, at {WALK_SPEED} points per second. \
                 Or pass to with a spot as whereabouts reports one, and it walks to to.x on display to.display. \
                 to.y is required so that spot passes back unchanged, and y is ignored, since a walk never changes height. \
                 It answers at once with started true, or with started false and the reason, such as falling or being held. \
                 It does not wait to arrive. \
                 The walk stops within {ARRIVAL_THRESHOLD} points of where it was sent, at the edge of what it stands on, \
                 or at a grab, a fall or a poke, and nothing resumes it. \
                 It never crosses to another display. Call whereabouts to see where it stopped."
            ),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "direction": {
                        "type": "string",
                        "enum": ["left", "right"],
                        "description": "Which way to walk, left or right of the screen. Pass it with exactly one of distance or duration_ms, and leave it out when you pass to."
                    },
                    "distance": {
                        "type": "number",
                        "exclusiveMinimum": 0,
                        "description": "How far to walk, in points, from where the Character stands now. Pass this or duration_ms, not both."
                    },
                    "duration_ms": {
                        "type": "integer",
                        "minimum": 1,
                        "description": format!("How long to walk, in milliseconds, at {WALK_SPEED} points per second. Pass this or distance, not both.")
                    },
                    "to": {
                        "type": "object",
                        "description": "A spot to walk to, as whereabouts reports one. Pass it instead of direction.",
                        "properties": {
                            "display": {
                                "type": "integer",
                                "minimum": 0,
                                "description": "Which display, by the index whereabouts lists. A spot on another display than the Character's own is walked toward, and the walk stops at the edge of its own display."
                            },
                            "x": {
                                "type": "number",
                                "description": "Points from that display's left edge, as whereabouts reports an Instance's x."
                            },
                            "y": {
                                "type": "number",
                                "description": "Points from that display's top edge, as whereabouts reports an Instance's y. Required so a whereabouts spot passes back unchanged, and ignored, since a walk never changes height."
                            }
                        },
                        "required": ["display", "x", "y"]
                    }
                }
            }),
        },
        ToolInfo {
            name: "list_windows".to_string(),
            description: "List visible windows with bounds. The owning application and the title need the window-names consent".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "describe_screen".to_string(),
            description: "Describe what is on screen (v1: window metadata only)".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "recall".to_string(),
            description: "Recall everything Memory holds".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "remember".to_string(),
            description: "Remember one fact under a heading".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "heading": {
                        "type": "string",
                        "description": "The heading under which to record the fact"
                    },
                    "fact": {
                        "type": "string",
                        "description": "The fact to remember"
                    }
                },
                "required": ["heading", "fact"]
            }),
        },
        ToolInfo {
            name: "list_instances".to_string(),
            description: "List Character Instances: each one's instance_id, name, and the three layers of its Character Prompt (app_instructions, personality_prompt, instance_prompt). Read it when your session did not open with your instance_id or prompt.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "whereabouts".to_string(),
            description: "Where each Character Instance is. `displays` lists every connected display with its name when the platform has one, its origin, and its size. Each instance names the display it is on, the other displays, and its feet in that display. Coordinates are logical points, y downward. Origin and size are in that same unit.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolInfo {
            name: "caffeinate".to_string(),
            description: "Keep this computer awake until it is turned off or Fidget quits. Returns whether it is on and, when it is, what keeps the computer awake.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "on": {
                        "type": "boolean",
                        "description": "true keeps the computer awake and false lets it sleep. Omit it to read the state without changing it."
                    }
                }
            }),
        },
    ];
    for tool in &mut tools {
        tool.input_schema["properties"]["instance_id"] = serde_json::json!({
            "type": "string",
            "description": "Your Character Instance id, named in your prompt. play_behavior and move act on that Instance. Optional while only one Instance runs."
        });
    }
    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_source::{Capabilities, FakeWindowSource, Rect, WindowRect, WorldGeometry};
    use serde_json::json;

    fn window(owner: &str, x: f64, y: f64, width: f64, height: f64) -> WindowRect {
        WindowRect {
            id: 0,
            bounds: Rect {
                x,
                y,
                width,
                height,
            },
            owner: Some(owner.to_string()),
            title: None,
            layer: 0,
        }
    }

    fn test_context<'a>(
        temp: &tempfile::TempDir,
        source: &'a FakeWindowSource,
        roster: &'a [InstanceInfo],
    ) -> DispatchContext<'a, 'a> {
        DispatchContext {
            window_source: source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster,
            expression: None,
            placement: PlacementQuery::empty(),
            caffeinate: None,
        }
    }

    fn fake_source(windows: Vec<WindowRect>) -> FakeWindowSource {
        FakeWindowSource {
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
                windows,
                dock: None,
            },
        }
    }

    #[test]
    fn dispatch_list_windows_returns_list_windows_result() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![window("Terminal", 10.0, 20.0, 800.0, 600.0)]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({});
        let result = dispatch("list_windows", args, &mut context).expect("dispatch succeeds");

        let windows = result["windows"].as_array().expect("windows is array");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0]["owner"], "Terminal");
    }

    #[test]
    fn dispatch_describe_screen_returns_describe_screen_result() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![window("Safari", 30.0, 40.0, 1200.0, 800.0)]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({});
        let result = dispatch("describe_screen", args, &mut context).expect("dispatch succeeds");

        let description = result["description"]
            .as_str()
            .expect("description is string");
        assert!(description.contains("1 visible window"));
        assert!(description.contains("Safari"));
    }

    #[test]
    fn dispatch_recall_returns_recall_result() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let memory = MemoryManifest::new(temp.path().join("memory.md"));
        memory
            .remember("Facts", "The user likes coffee")
            .expect("remembering writes");

        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({});
        let result = dispatch("recall", args, &mut context).expect("dispatch succeeds");

        let content = result["content"].as_str().expect("content is string");
        assert!(content.contains("The user likes coffee"));
    }

    #[test]
    fn dispatch_remember_returns_remember_result() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({"heading": "Facts", "fact": "The user's name is Oded"});
        let result = dispatch("remember", args, &mut context).expect("dispatch succeeds");

        assert_eq!(result["recorded"], "- The user's name is Oded");
    }

    #[test]
    fn dispatch_list_instances_returns_list_instances_result() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let roster = [
            InstanceInfo {
                id: "inst-1".to_string(),
                name: "Clippy".to_string(),
                app_instructions: "Reply with behavior: <name>".to_string(),
                personality_prompt: "a paperclip".to_string(),
                instance_prompt: "Answer in haiku.".to_string(),
            },
            InstanceInfo {
                id: "inst-2".to_string(),
                name: "Ferris".to_string(),
                ..Default::default()
            },
        ];
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &roster);

        let args = json!({});
        let result = dispatch("list_instances", args, &mut context).expect("dispatch succeeds");

        let instances = result["instances"].as_array().expect("instances is array");
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0]["id"], "inst-1");
        assert_eq!(instances[0]["name"], "Clippy");
        assert_eq!(
            instances[0]["app_instructions"],
            "Reply with behavior: <name>"
        );
        assert_eq!(instances[0]["personality_prompt"], "a paperclip");
        assert_eq!(instances[0]["instance_prompt"], "Answer in haiku.");
        assert_eq!(instances[1]["instance_prompt"], "");
    }

    #[test]
    fn dispatch_unknown_tool_returns_error() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({});
        let result = dispatch("click_mouse", args, &mut context);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::UnknownTool);
        assert!(err.message.contains("click_mouse"));
    }

    #[test]
    fn dispatch_with_invalid_arguments_returns_error() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({"wrong_field": 42});
        let result = dispatch("play_behavior", args, &mut context);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArguments);
    }

    #[test]
    fn list_tools_includes_all_handler_tools() {
        let tools = list_tools();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();

        assert_eq!(names.len(), 9);
        assert!(
            !names.contains(&"speak"),
            "a Character says things only in its reply"
        );
        assert!(names.contains(&"play_behavior"));
        assert!(names.contains(&"move"));
        assert!(names.contains(&"list_windows"));
        assert!(names.contains(&"describe_screen"));
        assert!(names.contains(&"recall"));
        assert!(names.contains(&"remember"));
        assert!(names.contains(&"list_instances"));
        assert!(names.contains(&"whereabouts"));
        assert!(names.contains(&"caffeinate"));
    }

    struct ScriptedCaffeinate {
        fact: Fact,
        reads: usize,
        sets: Vec<bool>,
    }

    impl CaffeinateHandle for ScriptedCaffeinate {
        fn call(&mut self, on: Option<bool>) -> Result<Fact, String> {
            match on {
                None => self.reads += 1,
                Some(value) => {
                    self.sets.push(value);
                    self.fact = if value {
                        Fact::On(crate::caffeinate::Flags::all())
                    } else {
                        Fact::Off
                    };
                }
            }
            Ok(self.fact)
        }
    }

    #[test]
    fn caffeinate_reads_when_on_is_omitted_and_sets_when_it_is_present() {
        let mut caffeinate = ScriptedCaffeinate {
            fact: Fact::Off,
            reads: 0,
            sets: Vec::new(),
        };
        let call = |caffeinate: &mut ScriptedCaffeinate, args: serde_json::Value| {
            let temp = tempfile::tempdir().expect("temp dir is creatable");
            let source = fake_source(vec![]);
            let mut context = test_context(&temp, &source, &[]);
            context.caffeinate = Some(caffeinate);
            dispatch("caffeinate", args, &mut context).expect("dispatch")
        };
        let read = call(&mut caffeinate, json!({}));
        assert_eq!(read, json!({ "on": false }));
        assert_eq!(caffeinate.reads, 1);
        assert!(caffeinate.sets.is_empty());

        let on = call(&mut caffeinate, json!({ "on": true }));
        let again = call(&mut caffeinate, json!({ "on": true }));
        let off = call(&mut caffeinate, json!({ "on": false }));
        let flags = crate::caffeinate::Flags::all().inhibitor_label();
        assert_eq!(on, json!({ "on": true, "flags": flags }));
        assert_eq!(again, json!({ "on": true, "flags": flags }));
        assert_eq!(off, json!({ "on": false }));
        assert_eq!(caffeinate.sets, [true, true, false]);
    }

    #[test]
    fn caffeinate_without_a_handle_errors() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);
        let err = dispatch("caffeinate", json!({ "on": true }), &mut context).unwrap_err();
        assert_eq!(err.code, ErrorCode::ExecutionFailed);
        assert!(err.message.contains("not available"));
    }

    #[test]
    fn list_tools_includes_no_input_event_tools() {
        let tools = list_tools();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();

        assert!(!names.contains(&"click"));
        assert!(!names.contains(&"type"));
        assert!(!names.contains(&"press_key"));
        assert!(!names.contains(&"move_mouse"));
    }

    #[test]
    fn dispatch_list_windows_applies_denylist() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let denylist = DenyList {
            excluded_applications: vec!["1Password".to_string()],
            filter_password_fields: true,
        };
        let source = fake_source(vec![
            window("Terminal", 10.0, 20.0, 800.0, 600.0),
            window("1Password", 30.0, 40.0, 400.0, 300.0),
        ]);
        let mut context = test_context(&temp, &source, &[]);
        context.denylist = denylist;

        let args = json!({});
        let result = dispatch("list_windows", args, &mut context).expect("dispatch succeeds");

        let windows = result["windows"].as_array().expect("windows is array");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0]["owner"], "Terminal");
    }

    #[test]
    fn dispatch_describe_screen_applies_denylist() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let denylist = DenyList {
            excluded_applications: vec!["1Password".to_string()],
            filter_password_fields: true,
        };
        let source = fake_source(vec![
            window("Terminal", 10.0, 20.0, 800.0, 600.0),
            window("1Password", 30.0, 40.0, 400.0, 300.0),
        ]);
        let mut context = test_context(&temp, &source, &[]);
        context.denylist = denylist;

        let args = json!({});
        let result = dispatch("describe_screen", args, &mut context).expect("dispatch succeeds");

        let description = result["description"]
            .as_str()
            .expect("description is string");
        assert!(description.contains("Terminal"));
        assert!(!description.contains("1Password"));
    }

    #[test]
    fn dispatch_list_instances_with_empty_roster_returns_empty_list() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({});
        let result = dispatch("list_instances", args, &mut context).expect("dispatch succeeds");

        let instances = result["instances"].as_array().expect("instances is array");
        assert_eq!(instances.len(), 0);
    }

    #[test]
    fn list_windows_and_describe_screen_apply_denylist_consistently() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let denylist = DenyList {
            excluded_applications: vec!["Keychain Access".to_string(), "1Password".to_string()],
            filter_password_fields: true,
        };
        let source = fake_source(vec![
            window("Terminal", 10.0, 20.0, 800.0, 600.0),
            window("Keychain Access", 30.0, 40.0, 400.0, 300.0),
            window("Safari", 50.0, 60.0, 1200.0, 800.0),
            window("1Password", 70.0, 80.0, 300.0, 200.0),
        ]);
        let mut context = test_context(&temp, &source, &[]);
        context.denylist = denylist;

        let list_args = json!({});
        let list_result =
            dispatch("list_windows", list_args, &mut context).expect("dispatch succeeds");

        let describe_args = json!({});
        let describe_result =
            dispatch("describe_screen", describe_args, &mut context).expect("dispatch succeeds");

        let windows = list_result["windows"].as_array().expect("windows is array");
        let allowed_apps: Vec<String> = windows
            .iter()
            .map(|w| w["owner"].as_str().unwrap().to_string())
            .collect();

        let description = describe_result["description"]
            .as_str()
            .expect("description is string");

        assert_eq!(allowed_apps.len(), 2);
        assert!(allowed_apps.contains(&"Terminal".to_string()));
        assert!(allowed_apps.contains(&"Safari".to_string()));
        assert!(!allowed_apps.contains(&"Keychain Access".to_string()));
        assert!(!allowed_apps.contains(&"1Password".to_string()));

        assert!(description.contains("2 visible windows"));
        assert!(description.contains("Terminal"));
        assert!(description.contains("Safari"));
        assert!(!description.contains("Keychain Access"));
        assert!(!description.contains("1Password"));

        for app in &allowed_apps {
            assert!(
                description.contains(app),
                "describe_screen must mention all apps that list_windows returns: missing {}",
                app
            );
        }
    }

    fn test_character(name: &str) -> crate::character::Character {
        use crate::character::{
            Animation, Behavior, Character, CursorReaction, GravityScale, Primitive,
            DEFAULT_MODEL_BASE, DEFAULT_MODEL_POWER, DEFAULT_WEIGHT,
        };
        use std::collections::BTreeMap;

        let mut animations = BTreeMap::new();
        let required = [
            "idle", "walk", "fall", "land", "sit", "sleep", "react", "talk", "hold",
        ];
        for anim in required {
            animations.insert(
                anim.to_string(),
                Animation {
                    frames: vec![format!("{anim}-0.png")],
                    frame_size: (32, 32),
                    fps: 8,
                    looping: true,
                    variants: Vec::new(),
                    left_strip: None,
                    weight: DEFAULT_WEIGHT,
                },
            );
        }

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
            name: name.to_string(),
            personality: format!("A test character named {name}"),
            animations,
            behaviors,
            art: BTreeMap::new(),
            smooth: false,
            scale: 1,
            model_base: DEFAULT_MODEL_BASE,
            model_power: DEFAULT_MODEL_POWER,
            near_reaction: CursorReaction::default(),
            rush_reaction: CursorReaction::default(),
            gravity_scale: GravityScale::DEFAULT,
            source: None,
        }
    }

    fn test_snapshot() -> crate::engine::WorldSnapshot {
        use crate::engine::{Point, Rect, WorldSnapshot};

        WorldSnapshot {
            displays: vec![Rect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }],
            windows: vec![],
            cursor: Point { x: 100.0, y: 100.0 },
            pressed_at: None,
            verbs: vec![],
            poke_settled: false,
            elapsed_ms: 16,
            proposal: None,
            poll_generation: 0,
            composing: false,
            locomotion_frozen: false,
            possession: Default::default(),
        }
    }

    fn test_roster_with_grounded_instance(name: &str) -> (crate::roster::Roster, String) {
        use crate::engine::Point;
        let mut roster = crate::roster::Roster::new();
        let character = test_character(name);
        let id = roster.spawn(&character, name.to_string(), Point { x: 100.0, y: 100.0 });

        // Fifty ticks let a spawn reach Grounded before a proposal.
        let mut snapshot = test_snapshot();
        for _ in 0..50 {
            if let Some(instance) = roster.get_mut(&id) {
                instance.tick(&snapshot);
            }
            snapshot.elapsed_ms = 16; // ~60fps
        }

        (roster, id)
    }

    #[test]
    fn reused_dispatch_context_keeps_the_expression_handle() {
        use std::cell::RefCell;
        use std::rc::Rc;

        struct SharedRoster(Rc<RefCell<crate::roster::Roster>>);

        impl crate::tools::ExpressionHandle for SharedRoster {
            fn enqueue(
                &mut self,
                instance_id: &str,
                proposal: crate::engine::BehaviorProposal,
            ) -> bool {
                crate::tools::ExpressionHandle::enqueue(
                    &mut *self.0.borrow_mut(),
                    instance_id,
                    proposal,
                )
            }

            fn start_move(
                &mut self,
                instance_id: &str,
                request: crate::engine::MoveRequest,
            ) -> Option<Result<(), crate::engine::MoveRefusal>> {
                crate::tools::ExpressionHandle::start_move(
                    &mut *self.0.borrow_mut(),
                    instance_id,
                    request,
                )
            }
        }

        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let (roster, _) = test_roster_with_grounded_instance("TestBuddy");

        let shared = Rc::new(RefCell::new(roster));
        let roster_info = shared
            .borrow()
            .list()
            .into_iter()
            .map(|(id, name)| InstanceInfo {
                id,
                name,
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let mut expression = SharedRoster(Rc::clone(&shared));
        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(&mut expression),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        for behavior in ["wave", "spin"] {
            let played = dispatch("play_behavior", json!({"behavior": behavior}), &mut context)
                .expect("dispatch succeeds");
            assert_eq!(played["success"], true, "{behavior}: {played}");
        }
    }

    #[test]
    fn play_behavior_direction_reaches_the_proposal() {
        struct Captured(Option<crate::engine::BehaviorProposal>);

        impl crate::tools::ExpressionHandle for Captured {
            fn enqueue(
                &mut self,
                _instance_id: &str,
                proposal: crate::engine::BehaviorProposal,
            ) -> bool {
                self.0 = Some(proposal);
                true
            }

            fn start_move(
                &mut self,
                _instance_id: &str,
                _request: crate::engine::MoveRequest,
            ) -> Option<Result<(), crate::engine::MoveRefusal>> {
                None
            }
        }

        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let roster = [InstanceInfo {
            id: "inst-1".to_string(),
            name: "TestBuddy".to_string(),
            ..Default::default()
        }];
        let mut captured = Captured(None);
        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster,
            expression: Some(&mut captured),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        let played = dispatch(
            "play_behavior",
            json!({"behavior": "walk", "direction": "left"}),
            &mut context,
        )
        .expect("dispatch succeeds");
        assert_eq!(played["success"], true);
        drop(context);

        let proposal = captured.0.expect("the proposal reached the handle");
        assert_eq!(proposal.direction, Some(Direction::Left));
    }

    #[test]
    fn play_behavior_refuses_a_direction_it_does_not_know() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let result = dispatch(
            "play_behavior",
            json!({"behavior": "walk", "direction": "up"}),
            &mut context,
        );

        assert_eq!(
            result.unwrap_err().code,
            ErrorCode::InvalidArguments,
            "a direction is left or right and nothing else"
        );
    }

    #[test]
    fn play_behavior_schema_names_the_direction_it_takes() {
        let tools = list_tools();
        let play = tools
            .iter()
            .find(|tool| tool.name == "play_behavior")
            .expect("play_behavior is listed");

        assert_eq!(
            play.input_schema["properties"]["direction"]["enum"],
            json!(["left", "right"])
        );
        assert!(play.description.contains("direction"));
    }

    #[test]
    fn move_is_listed_with_every_field_described_and_y_ignored() {
        let tools = list_tools();
        let walk = tools
            .iter()
            .find(|tool| tool.name == "move")
            .expect("move is listed");

        for field in [
            "direction",
            "distance",
            "duration_ms",
            "to.display",
            "to.x",
            "to.y",
        ] {
            assert!(
                walk.description.contains(field),
                "the description names {field}: {}",
                walk.description
            );
        }
        assert!(
            walk.description.contains("y is ignored"),
            "{}",
            walk.description
        );

        let properties = &walk.input_schema["properties"];
        assert_eq!(properties["direction"]["enum"], json!(["left", "right"]));
        for field in ["direction", "distance", "duration_ms", "to"] {
            assert!(
                properties[field]["description"].is_string(),
                "{field} has a description"
            );
        }
        let spot = &properties["to"];
        for field in ["display", "x", "y"] {
            assert!(
                spot["properties"][field]["description"].is_string(),
                "to.{field} has a description"
            );
        }
        assert_eq!(
            spot["required"],
            json!(["display", "x", "y"]),
            "a whereabouts spot passes back whole"
        );
    }

    /// The ask end to end: the argument turns the Instance before its first
    /// step, so a right-facing sprite told to walk left goes left.
    #[test]
    fn play_behavior_direction_moves_the_instance_the_way_it_was_asked() {
        use crate::character::{Behavior, Primitive};
        use crate::engine::Point;

        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut roster = crate::roster::Roster::new();
        let mut character = test_character("Walker");
        character.behaviors.insert(
            "walk".to_string(),
            Behavior {
                primitives: vec![Primitive::Walk],
                then: None,
                weight: 0,
                trigger: None,
            },
        );
        let instance_id = roster.spawn(
            &character,
            "Walker".to_string(),
            Point { x: 500.0, y: 100.0 },
        );

        let mut snapshot = test_snapshot();
        let mut resting = None;
        for _ in 0..50 {
            resting = Some(roster.get_mut(&instance_id).unwrap().tick(&snapshot));
            snapshot.elapsed_ms = 16;
        }
        let resting = resting.expect("the sprite ticked");
        assert_eq!(resting.facing, 1.0, "it faced right before the ask");

        let roster_info = roster
            .list()
            .into_iter()
            .map(|(id, name)| InstanceInfo {
                id,
                name,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(&mut roster),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        let played = dispatch(
            "play_behavior",
            json!({"behavior": "walk", "direction": "left"}),
            &mut context,
        )
        .expect("dispatch succeeds");
        assert_eq!(played["success"], true);
        drop(context);

        let instance = roster.get_mut(&instance_id).expect("instance still runs");
        let asked = instance.tick(&test_snapshot());
        assert_eq!(asked.facing, -1.0, "the ask turns it before it walks");
        let walking = instance.tick(&test_snapshot());
        assert!(
            walking.position.x < asked.position.x,
            "told to walk left: {asked:?} -> {walking:?}"
        );
    }

    /// A Roster with one Instance landed at `x` on `world`'s floor, and the
    /// last Frame of its landing.
    fn landed_walker(
        x: f64,
        world: &crate::engine::WorldSnapshot,
    ) -> (crate::roster::Roster, String, crate::engine::Frame) {
        use crate::engine::Point;

        let mut roster = crate::roster::Roster::new();
        let id = roster.spawn(
            &test_character("Walker"),
            "Walker".to_string(),
            Point { x, y: 100.0 },
        );
        let mut landed = None;
        for _ in 0..100 {
            landed = Some(roster.get_mut(&id).unwrap().tick(world));
        }
        (roster, id, landed.expect("the sprite ticked"))
    }

    /// `move` on displays whose full frames are their usable ones.
    fn call_move(
        roster: &mut crate::roster::Roster,
        arguments: Value,
        frames: &[Rect],
    ) -> Result<Value, DispatchError> {
        call_move_on(roster, arguments, frames, frames)
    }

    /// `move` through `dispatch`, with the Roster as the handle, `frames` as
    /// the full frames `whereabouts` reports and `usable` as the floors the
    /// Engine walks, as the frame loop calls it.
    fn call_move_on(
        roster: &mut crate::roster::Roster,
        arguments: Value,
        frames: &[Rect],
        usable: &[Rect],
    ) -> Result<Value, DispatchError> {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let roster_info = roster
            .list()
            .into_iter()
            .map(|(id, name)| InstanceInfo {
                id,
                name,
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(roster),
            placement: PlacementQuery {
                frames,
                usable,
                names: &[],
                instances: &[],
            },
            caffeinate: None,
        };
        dispatch("move", arguments, &mut context)
    }

    /// The first Frame the Instance stands still in. A walk that never stops
    /// fails here rather than passing on wherever it had got to.
    fn tick_until_still(
        roster: &mut crate::roster::Roster,
        id: &str,
        world: &crate::engine::WorldSnapshot,
    ) -> crate::engine::Frame {
        for _ in 0..1_000 {
            let frame = roster.get_mut(id).expect("instance still runs").tick(world);
            if frame.velocity.x == 0.0 {
                return frame;
            }
        }
        panic!("still walking after 1000 ticks");
    }

    #[test]
    fn move_left_walks_the_distance_asked_and_keeps_its_height() {
        let world = test_snapshot();
        let (mut roster, id, landed) = landed_walker(500.0, &world);
        assert_eq!(landed.position.x, 500.0, "{landed:?}");
        assert_eq!(landed.facing, 1.0, "it faced right before the ask");

        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said, json!({"started": true}));

        let stopped = tick_until_still(&mut roster, &id, &world);
        assert!(
            (stopped.position.x - 300.0).abs() <= 30.0,
            "200 points left of 500: {stopped:?}"
        );
        assert_eq!(stopped.facing, -1.0, "{stopped:?}");
        assert_eq!(stopped.position.y, landed.position.y, "{stopped:?}");
    }

    #[test]
    fn move_right_walks_the_distance_asked_and_keeps_its_height() {
        let world = test_snapshot();
        let (mut roster, id, landed) = landed_walker(500.0, &world);

        let said = call_move(
            &mut roster,
            json!({"direction": "right", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said, json!({"started": true}));

        let stopped = tick_until_still(&mut roster, &id, &world);
        assert!(
            (stopped.position.x - 700.0).abs() <= 30.0,
            "200 points right of 500: {stopped:?}"
        );
        assert_eq!(stopped.facing, 1.0, "{stopped:?}");
        assert_eq!(stopped.position.y, landed.position.y, "{stopped:?}");
    }

    #[test]
    fn a_stalled_frame_does_not_carry_a_move_past_where_it_stops() {
        let world = test_snapshot();
        let (mut roster, id, _) = landed_walker(500.0, &world);

        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said, json!({"started": true}));

        let mut stalled = world.clone();
        stalled.elapsed_ms = 1000;
        let stopped = tick_until_still(&mut roster, &id, &stalled);
        assert!(
            (stopped.position.x - 300.0).abs() <= 30.0,
            "200 points left of 500: {stopped:?}"
        );
    }

    #[test]
    fn move_for_a_duration_walks_as_far_as_that_time_covers() {
        let world = test_snapshot();
        let (mut roster, id, landed) = landed_walker(500.0, &world);

        let said = call_move(
            &mut roster,
            json!({"direction": "right", "duration_ms": 1000}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said, json!({"started": true}));

        let stopped = tick_until_still(&mut roster, &id, &world);
        assert!(
            (stopped.position.x - 620.0).abs() <= 30.0,
            "one second is 120 points: {stopped:?}"
        );
        assert_eq!(stopped.position.y, landed.position.y, "{stopped:?}");
    }

    /// Display 1's usable floor starts 80 points in from its frame, as a Dock
    /// on its left would leave it, so the frame's origin and the floor's differ
    /// by more than the arrival band.
    fn side_by_side() -> ([Rect; 2], crate::engine::WorldSnapshot) {
        let display = |x, width| Rect {
            x,
            y: 0.0,
            width,
            height: 1080.0,
        };
        let frames = [display(0.0, 1920.0), display(1920.0, 1920.0)];
        let mut world = test_snapshot();
        world.displays = vec![display(0.0, 1920.0), display(2000.0, 1840.0)];
        (frames, world)
    }

    #[test]
    fn move_to_a_whereabouts_spot_stops_there_and_keeps_its_height() {
        let (frames, world) = side_by_side();
        let (mut roster, id, landed) = landed_walker(2620.0, &world);

        let said = call_move_on(
            &mut roster,
            json!({"to": {"display": 1, "x": 400, "y": 900}}),
            &frames,
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said, json!({"started": true}));

        let stopped = tick_until_still(&mut roster, &id, &world);
        assert!(
            (stopped.position.x - 2320.0).abs() <= 30.0,
            "400 points into the display whose frame starts at 1920: {stopped:?}"
        );
        assert_eq!(stopped.position.y, landed.position.y, "{stopped:?}");
    }

    #[test]
    fn move_refuses_a_spot_whereabouts_could_not_have_reported() {
        let (frames, world) = side_by_side();
        let (mut roster, _, _) = landed_walker(2620.0, &world);

        for spot in [
            json!({"display": 2, "x": 400, "y": 900}),
            json!({"display": 1, "x": 5000, "y": 900}),
            json!({"display": 1, "x": -10, "y": 900}),
            json!({"display": 1, "x": 400}),
        ] {
            let refused =
                call_move_on(&mut roster, json!({ "to": spot }), &frames, &world.displays);
            assert_eq!(
                refused.map_err(|error| error.code),
                Err(ErrorCode::InvalidArguments),
                "{spot}"
            );
        }
    }

    #[test]
    fn move_inside_the_arrival_band_does_not_start() {
        let world = test_snapshot();
        let (mut roster, id, landed) = landed_walker(500.0, &world);

        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 10}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], false, "{said}");
        assert!(said["reason"].is_string(), "{said}");

        let after = tick_until_still(&mut roster, &id, &world);
        assert_eq!(after.position.x, landed.position.x, "{after:?}");
    }

    #[test]
    fn a_walk_asked_of_a_drowsy_instance_does_not_end_asleep() {
        let world = test_snapshot();
        let (mut roster, id, _) = landed_walker(500.0, &world);
        let mut a_second = world.clone();
        a_second.elapsed_ms = 1000;
        for _ in 0..58 {
            roster.get_mut(&id).unwrap().tick(&a_second);
        }

        let said = call_move(
            &mut roster,
            json!({"direction": "right", "distance": 600}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said, json!({"started": true}));

        let stopped = tick_until_still(&mut roster, &id, &world);
        assert!(
            (stopped.position.x - 1100.0).abs() <= 30.0,
            "600 points right of 500: {stopped:?}"
        );
        assert_eq!(stopped.state, crate::engine::State::Grounded, "{stopped:?}");
    }

    /// `ticks` more Frames of `world`, and the last of them.
    fn tick_for(
        roster: &mut crate::roster::Roster,
        id: &str,
        world: &crate::engine::WorldSnapshot,
        ticks: usize,
    ) -> crate::engine::Frame {
        let instance = roster.get_mut(id).expect("instance still runs");
        (0..ticks)
            .map(|_| instance.tick(world))
            .last()
            .expect("at least one tick")
    }

    #[test]
    fn move_refuses_while_the_instance_is_falling() {
        use crate::engine::{Point, State};

        let world = test_snapshot();
        let mut roster = crate::roster::Roster::new();
        let id = roster.spawn(
            &test_character("Walker"),
            "Walker".to_string(),
            Point { x: 500.0, y: 100.0 },
        );
        let midair = tick_for(&mut roster, &id, &world, 1);
        assert_eq!(midair.state, State::Falling, "{midair:?}");

        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], false, "{said}");
        assert!(said["reason"].is_string(), "{said}");

        let landed = tick_for(&mut roster, &id, &world, 200);
        assert_eq!(landed.state, State::Grounded, "{landed:?}");
        assert_eq!(landed.position.x, 500.0, "nothing walks it once it lands");
    }

    #[test]
    fn move_refuses_while_the_user_holds_the_instance() {
        use crate::engine::{State, Verb, WorldSnapshot};

        let world = test_snapshot();
        let (mut roster, id, landed) = landed_walker(500.0, &world);
        let in_hand = WorldSnapshot {
            cursor: landed.position,
            verbs: vec![Verb::Grab],
            ..world.clone()
        };
        let held = tick_for(&mut roster, &id, &in_hand, 1);
        assert_eq!(held.state, State::Dragged, "{held:?}");

        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], false, "{said}");
        assert!(said["reason"].is_string(), "{said}");

        let let_go = tick_for(&mut roster, &id, &world, 200);
        assert_eq!(let_go.state, State::Grounded, "{let_go:?}");
        assert_eq!(
            let_go.position.x, 500.0,
            "nothing walks it once it is let go"
        );
    }

    #[test]
    fn every_refusal_names_its_cause() {
        use crate::engine::{Point, State, Verb, WorldSnapshot};
        use crate::roster::Roster;

        type Setup = fn(&mut Roster, &str, &WorldSnapshot);
        let causes: [(&str, Setup); 7] = [
            ("in the air", |roster, id, world| {
                let instance = roster.get_mut(id).unwrap();
                instance.stand_at(Point { x: 500.0, y: 100.0 });
                assert_eq!(instance.tick(world).state, State::Falling);
            }),
            ("holding", |roster, id, world| {
                let instance = roster.get_mut(id).unwrap();
                let grab = WorldSnapshot {
                    cursor: instance.feet(),
                    verbs: vec![Verb::Grab],
                    ..world.clone()
                };
                assert_eq!(instance.tick(&grab).state, State::Dragged);
            }),
            ("climbing", |roster, id, world| {
                let instance = roster.get_mut(id).unwrap();
                let hand = |cursor, verb| WorldSnapshot {
                    cursor,
                    verbs: vec![verb],
                    ..world.clone()
                };
                let aloft = Point {
                    x: 1500.0,
                    y: 500.0,
                };
                let throw = Verb::Throw {
                    velocity: Point { x: 2000.0, y: 0.0 },
                };
                instance.tick(&hand(instance.feet(), Verb::Grab));
                instance.tick(&hand(aloft, Verb::Grab));
                instance.tick(&hand(aloft, throw));
                let climbing = (0..100).any(|_| instance.tick(world).state == State::Climbing);
                assert!(climbing, "thrown at the wall, it climbs");
            }),
            ("asleep", |roster, id, world| {
                let a_second = WorldSnapshot {
                    elapsed_ms: 1000,
                    ..world.clone()
                };
                assert_eq!(tick_for(roster, id, &a_second, 61).state, State::Asleep);
            }),
            ("poked", |roster, id, world| {
                let poke = WorldSnapshot {
                    verbs: vec![Verb::Poke],
                    ..world.clone()
                };
                assert_eq!(tick_for(roster, id, &poke, 1).animation, "react");
            }),
            ("keyboard", |roster, id, _| {
                roster.get_mut(id).unwrap().set_possessed(true)
            }),
            ("Do Not Disturb", |roster, id, _| {
                roster.get_mut(id).unwrap().set_do_not_disturb(true)
            }),
        ];

        let world = test_snapshot();
        for (cause, setup) in causes {
            let (mut roster, id, _) = landed_walker(500.0, &world);
            setup(&mut roster, &id, &world);

            let said = call_move(
                &mut roster,
                json!({"direction": "left", "distance": 200}),
                &world.displays,
            )
            .expect("dispatch succeeds");
            assert_eq!(said["started"], false, "{cause}: {said}");
            let reason = said["reason"].as_str().expect("a refusal says why");
            assert!(reason.contains(cause), "{reason:?} does not say {cause:?}");
        }
    }

    #[test]
    fn a_move_the_user_cuts_short_never_resumes() {
        use crate::engine::{Verb, WorldSnapshot};
        use crate::roster::Roster;

        type Interruption = fn(&mut Roster, &str, &WorldSnapshot);
        let interruptions: [(&str, Interruption); 4] = [
            ("a grab and a release", |roster, id, world| {
                let held = WorldSnapshot {
                    cursor: roster.get_mut(id).unwrap().feet(),
                    verbs: vec![Verb::Grab],
                    ..world.clone()
                };
                tick_for(roster, id, &held, 10);
            }),
            ("a poke", |roster, id, world| {
                let poke = WorldSnapshot {
                    verbs: vec![Verb::Poke],
                    ..world.clone()
                };
                tick_for(roster, id, &poke, 1);
            }),
            ("Do Not Disturb on and off", |roster, id, world| {
                let instance = roster.get_mut(id).unwrap();
                instance.set_do_not_disturb(true);
                instance.tick(world);
                instance.set_do_not_disturb(false);
            }),
            ("possession on and off", |roster, id, world| {
                let instance = roster.get_mut(id).unwrap();
                instance.set_possessed(true);
                instance.tick(world);
                instance.set_possessed(false);
            }),
        ];

        let world = test_snapshot();
        let resumed: Vec<String> = interruptions
            .into_iter()
            .filter_map(|(interruption, interrupt)| {
                let (mut roster, id, _) = landed_walker(500.0, &world);
                let said = call_move(
                    &mut roster,
                    json!({"direction": "left", "distance": 200}),
                    &world.displays,
                )
                .expect("dispatch succeeds");
                assert_eq!(said["started"], true, "{said}");
                let under_way = tick_for(&mut roster, &id, &world, 20);
                assert_eq!(under_way.animation, "walk", "{under_way:?}");

                interrupt(&mut roster, &id, &world);
                let stopped_at = roster.get_mut(&id).unwrap().feet().x;
                let later = tick_for(&mut roster, &id, &world, 600);
                (later.position.x != stopped_at).then(|| {
                    format!(
                        "{interruption}, from x {stopped_at} to {}",
                        later.position.x
                    )
                })
            })
            .collect();
        assert!(resumed.is_empty(), "walked on after {resumed:#?}");
    }

    #[test]
    fn a_move_ends_when_the_instance_is_brought_somewhere_else() {
        use crate::engine::Point;

        let world = test_snapshot();
        let (mut roster, id, landed) = landed_walker(500.0, &world);
        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], true, "{said}");
        tick_for(&mut roster, &id, &world, 20);

        roster.get_mut(&id).unwrap().stand_at(Point {
            x: 900.0,
            y: landed.position.y,
        });
        let later = tick_for(&mut roster, &id, &world, 600);
        assert_eq!(later.position.x, 900.0, "it stays where it was brought");
    }

    #[test]
    fn a_move_walks_on_under_a_speech_bubble_and_looks_it() {
        use crate::engine::{BehaviorProposal, WorldSnapshot};
        use std::collections::BTreeSet;

        let world = test_snapshot();
        let (mut roster, id, _) = landed_walker(500.0, &world);
        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], true, "{said}");

        let bubble = WorldSnapshot {
            locomotion_frozen: true,
            ..world.clone()
        };
        let a_line = WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: String::new(),
                dialogue: Some("On my way.".to_string()),
                direction: None,
            }),
            ..bubble.clone()
        };
        let instance = roster.get_mut(&id).unwrap();
        let mut frame = instance.tick(&a_line);
        let mut drawn = BTreeSet::new();
        for _ in 0..1_000 {
            if frame.velocity.x == 0.0 {
                break;
            }
            drawn.insert(frame.animation);
            frame = instance.tick(&bubble);
        }
        assert_eq!(frame.velocity.x, 0.0, "still walking after 1000 ticks");
        assert!((frame.position.x - 300.0).abs() <= 30.0, "{frame:?}");
        assert_eq!(drawn, BTreeSet::from(["walk"]), "the art while it moved");
    }

    #[test]
    fn a_behavior_that_needs_the_feet_does_not_cut_a_move_short() {
        use crate::character::{Behavior, Primitive};
        use crate::engine::{BehaviorProposal, Point, State, WorldSnapshot};

        let mut character = test_character("Hopper");
        character.behaviors.insert(
            "hop".to_string(),
            Behavior {
                primitives: vec![Primitive::Jump],
                then: None,
                weight: 1,
                trigger: None,
            },
        );
        let world = test_snapshot();
        let mut roster = crate::roster::Roster::new();
        let id = roster.spawn(
            &character,
            "Hopper".to_string(),
            Point { x: 500.0, y: 100.0 },
        );
        tick_for(&mut roster, &id, &world, 100);
        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], true, "{said}");
        tick_for(&mut roster, &id, &world, 20);

        let a_hop = WorldSnapshot {
            proposal: Some(BehaviorProposal {
                behavior: "hop".to_string(),
                dialogue: None,
                direction: None,
            }),
            ..world.clone()
        };
        let asked = tick_for(&mut roster, &id, &a_hop, 1);
        assert_eq!(asked.refused.as_deref(), Some("hop"), "{asked:?}");

        let stopped = tick_until_still(&mut roster, &id, &world);
        assert_eq!(stopped.state, State::Grounded, "{stopped:?}");
        assert!((stopped.position.x - 300.0).abs() <= 30.0, "{stopped:?}");
    }

    #[test]
    fn do_not_disturb_sits_a_moving_instance_down_as_it_does_any_walk() {
        let world = test_snapshot();
        let (mut roster, id, _) = landed_walker(500.0, &world);
        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &world.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], true, "{said}");
        tick_for(&mut roster, &id, &world, 20);

        roster.get_mut(&id).unwrap().set_do_not_disturb(true);
        let quiet = tick_for(&mut roster, &id, &world, 1);
        assert_eq!(quiet.animation, "sit", "{quiet:?}");
        assert_eq!(quiet.velocity.x, 0.0, "{quiet:?}");
    }

    #[test]
    fn a_move_toward_another_display_stops_at_the_edge_of_this_one() {
        use crate::engine::State;

        let (frames, a_dock_between) = side_by_side();
        let abutting = crate::engine::WorldSnapshot {
            displays: frames.to_vec(),
            ..a_dock_between.clone()
        };
        let layouts = [
            ("a Dock between", a_dock_between, 2064.0),
            ("abutting", abutting, 1984.0),
        ];

        for (layout, world, inset_edge) in layouts {
            let (mut roster, id, _) = landed_walker(2620.0, &world);
            let said = call_move_on(
                &mut roster,
                json!({"to": {"display": 0, "x": 400, "y": 900}}),
                &frames,
                &world.displays,
            )
            .expect("dispatch succeeds");
            assert_eq!(said["started"], true, "{layout}: {said}");

            let stopped = tick_until_still(&mut roster, &id, &world);
            assert_eq!(stopped.state, State::Grounded, "{layout}: {stopped:?}");
            assert!(
                (stopped.position.x - inset_edge).abs() <= 30.0,
                "{layout}: {stopped:?}"
            );
        }
    }

    #[test]
    fn a_move_toward_an_edge_it_already_stands_past_never_turns_it_round() {
        let (frames, world) = side_by_side();
        let abutting = crate::engine::WorldSnapshot {
            displays: frames.to_vec(),
            ..world
        };
        let (mut roster, id, landed) = landed_walker(1930.0, &abutting);
        assert_eq!(landed.position.x, 1930.0, "{landed:?}");

        let said = call_move_on(
            &mut roster,
            json!({"to": {"display": 0, "x": 400, "y": 900}}),
            &frames,
            &abutting.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], true, "{said}");
        let later = tick_for(&mut roster, &id, &abutting, 100);
        assert_eq!(later.position.x, 1930.0, "{later:?}");
    }

    /// A window whose top is a Perch from x 300 to 900, 600 points down.
    fn a_perch() -> crate::engine::WorldSnapshot {
        crate::engine::WorldSnapshot {
            windows: vec![crate::engine::Window {
                id: 1,
                rect: Rect {
                    x: 300.0,
                    y: 600.0,
                    width: 600.0,
                    height: 200.0,
                },
            }],
            ..test_snapshot()
        }
    }

    #[test]
    fn a_move_past_the_end_of_its_perch_stops_on_it() {
        use crate::engine::State;

        let world = a_perch();
        for (direction, perch_end) in [("right", 900.0), ("left", 300.0)] {
            let (mut roster, id, landed) = landed_walker(600.0, &world);
            assert_eq!(landed.state, State::Perched, "{landed:?}");

            let said = call_move(
                &mut roster,
                json!({"direction": direction, "distance": 600}),
                &world.displays,
            )
            .expect("dispatch succeeds");
            assert_eq!(said["started"], true, "{said}");

            let stopped = tick_until_still(&mut roster, &id, &world);
            assert_eq!(stopped.state, State::Perched, "{direction}: {stopped:?}");
            assert_eq!(stopped.position.y, 600.0, "{direction}: {stopped:?}");
            assert!(
                (stopped.position.x - perch_end).abs() <= 30.0,
                "{direction}: {stopped:?}"
            );
        }
    }

    #[test]
    fn a_move_whose_perch_closes_falls_with_its_stride_and_stays_down() {
        use crate::engine::State;

        let perch = a_perch();
        let closed = test_snapshot();
        let (mut roster, id, _) = landed_walker(600.0, &perch);
        let said = call_move(
            &mut roster,
            json!({"direction": "left", "distance": 200}),
            &perch.displays,
        )
        .expect("dispatch succeeds");
        assert_eq!(said["started"], true, "{said}");
        tick_for(&mut roster, &id, &perch, 20);

        let lost = tick_for(&mut roster, &id, &closed, 1);
        assert_eq!(lost.state, State::Falling, "{lost:?}");
        let down = tick_for(&mut roster, &id, &closed, 200);
        assert_eq!(down.state, State::Grounded, "{down:?}");
        assert!(
            down.position.x < lost.position.x,
            "it fell with its stride: {down:?}"
        );
        let later = tick_for(&mut roster, &id, &closed, 300);
        assert_eq!(
            later.position.x, down.position.x,
            "nothing resumes the walk"
        );
    }

    #[test]
    fn move_refuses_a_distance_or_duration_that_is_not_positive() {
        let world = test_snapshot();
        let (mut roster, _, _) = landed_walker(500.0, &world);

        for arguments in [
            json!({"direction": "left", "distance": 0}),
            json!({"direction": "left", "distance": -200}),
            json!({"direction": "right", "duration_ms": 0}),
        ] {
            let refused = call_move(&mut roster, arguments.clone(), &world.displays);
            assert_eq!(
                refused.map_err(|error| error.code),
                Err(ErrorCode::InvalidArguments),
                "{arguments}"
            );
        }
    }

    #[test]
    fn move_refuses_a_half_filled_request_and_names_what_to_change() {
        let world = test_snapshot();
        let (mut roster, _, _) = landed_walker(500.0, &world);

        for (arguments, names) in [
            (
                json!({"direction": "left", "distance": 200, "duration_ms": 1000}),
                &["distance", "duration_ms"][..],
            ),
            (json!({"direction": "left"}), &["distance", "duration_ms"]),
            (json!({"distance": 200}), &["direction"]),
            (json!({"duration_ms": 1000}), &["direction"]),
            (json!({}), &["direction"]),
            (
                json!({"direction": "left", "to": {"display": 0, "x": 100, "y": 1080}}),
                &["to", "direction"],
            ),
        ] {
            let error = call_move(&mut roster, arguments.clone(), &world.displays)
                .expect_err("a half-filled request builds no walk");
            assert_eq!(error.code, ErrorCode::InvalidArguments, "{arguments}");
            for name in names {
                assert!(
                    error.message.contains(name),
                    "{arguments} names {name}: {}",
                    error.message
                );
            }
        }
    }

    #[test]
    fn move_says_why_nobody_was_reached_in_the_words_play_behavior_uses() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let one = [InstanceInfo {
            id: "inst-1".to_string(),
            name: "Walker".to_string(),
            ..Default::default()
        }];
        let two = [
            one[0].clone(),
            InstanceInfo {
                id: "inst-2".to_string(),
                name: "Runner".to_string(),
                ..Default::default()
            },
        ];
        let mut retired = crate::roster::Roster::new();

        let mut contexts = [
            test_context(&temp, &source, &[]),
            test_context(&temp, &source, &two),
            test_context(&temp, &source, &one),
            DispatchContext {
                expression: Some(&mut retired),
                ..test_context(&temp, &source, &one)
            },
        ];
        for context in &mut contexts {
            let moved = dispatch(
                "move",
                json!({"direction": "left", "distance": 200}),
                context,
            )
            .expect("dispatch succeeds");
            let played = dispatch("play_behavior", json!({"behavior": "wave"}), context)
                .expect("dispatch succeeds");
            assert_eq!(moved["started"], false, "{moved}");
            assert!(moved["reason"].is_string(), "{moved}");
            assert_eq!(moved["reason"], played["reason"]);
        }
    }

    #[test]
    fn play_behavior_with_one_instance_delivers_the_proposal_to_the_engine() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let (mut roster, instance_id) = test_roster_with_grounded_instance("TestBuddy");

        let roster_info = roster
            .list()
            .into_iter()
            .map(|(id, name)| InstanceInfo {
                id,
                name,
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(&mut roster),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        let args = json!({"behavior": "wave"});
        let result = dispatch("play_behavior", args, &mut context).expect("dispatch succeeds");

        assert_eq!(result["success"], true);
        assert_eq!(result["behavior"], "wave");
        drop(context);

        let snapshot = test_snapshot();
        let instance = roster
            .get_mut(&instance_id)
            .expect("instance still in roster");
        let frame = instance.tick(&snapshot);
        assert_eq!(frame.animation, "react");
        assert_eq!(frame.behavior, Some("wave".to_string()));
    }

    /// MCP Expression path for a hop: enqueue succeeds and the Engine leaves
    /// the floor.
    #[test]
    fn play_behavior_jump_leaves_the_ground() {
        use crate::character::{Behavior, Primitive};
        use crate::engine::Point;

        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut roster = crate::roster::Roster::new();
        let mut character = test_character("Jumper");
        character.behaviors.insert(
            "jump".to_string(),
            Behavior {
                primitives: vec![Primitive::Jump],
                then: None,
                weight: 0,
                trigger: None,
            },
        );
        let instance_id = roster.spawn(
            &character,
            "Jumper".to_string(),
            Point { x: 100.0, y: 100.0 },
        );
        let mut snapshot = test_snapshot();
        for _ in 0..50 {
            if let Some(instance) = roster.get_mut(&instance_id) {
                instance.tick(&snapshot);
            }
            snapshot.elapsed_ms = 16;
        }

        let roster_info = roster
            .list()
            .into_iter()
            .map(|(id, name)| InstanceInfo {
                id,
                name,
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(&mut roster),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        let result = dispatch("play_behavior", json!({"behavior": "jump"}), &mut context)
            .expect("dispatch succeeds");
        assert_eq!(result["success"], true);
        assert_eq!(result["behavior"], "jump");
        drop(context);

        let instance = roster
            .get_mut(&instance_id)
            .expect("instance still in roster");
        let launch = instance.tick(&test_snapshot());
        assert_eq!(launch.behavior.as_deref(), Some("jump"));

        let mut flying = test_snapshot();
        flying.elapsed_ms = 100;
        let launched = instance.tick(&flying);
        assert!(
            launched.velocity.y < 0.0,
            "play_behavior jump must leave the floor: {launched:?}"
        );
    }

    #[test]
    fn omitted_instance_id_with_several_instances_is_failure() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let infos = [
            InstanceInfo {
                id: "instance-1".to_string(),
                name: "First".to_string(),
                ..Default::default()
            },
            InstanceInfo {
                id: "instance-2".to_string(),
                name: "Second".to_string(),
                ..Default::default()
            },
        ];
        let mut context = test_context(&temp, &source, &infos);

        let args = json!({"behavior": "wave"});
        let result = dispatch("play_behavior", args, &mut context).expect("dispatch succeeds");

        assert_eq!(result["success"], false);
        assert_eq!(result["behavior"], "wave");
        assert_eq!(
            result["reason"],
            "Several Character Instances are running; name one with instance_id. \
             Call list_instances to find each one's instance_id."
        );
    }

    /// Every tool, not only the Expression ones, refuses an id no Instance
    /// carries, and the refusal names the ids that would work.
    #[test]
    fn every_tool_refuses_an_unknown_instance_id_and_names_the_running_ones() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let infos = [
            InstanceInfo {
                id: "pip".to_string(),
                name: "Pip".to_string(),
                ..Default::default()
            },
            InstanceInfo {
                id: "rex".to_string(),
                name: "Rex".to_string(),
                ..Default::default()
            },
        ];
        let mut context = test_context(&temp, &source, &infos);

        for tool in list_tools() {
            let refused = dispatch(
                &tool.name,
                json!({"behavior": "wave", "instance_id": "nope"}),
                &mut context,
            )
            .expect_err(&format!("{} accepted an unknown instance_id", tool.name));
            assert_eq!(refused.code, ErrorCode::InvalidArguments, "{}", tool.name);
            assert_eq!(
                refused.message,
                "No Character Instance has id nope. Running: pip (Pip), rex (Rex). \
                 Pass your own instance_id from your prompt, or call list_instances to find it.",
                "{}",
                tool.name
            );
        }
    }

    #[test]
    fn an_instance_id_with_nothing_running_says_so() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let refused = dispatch("recall", json!({"instance_id": "nope"}), &mut context)
            .expect_err("an id is refused when no Instance runs");

        assert_eq!(
            refused.message,
            "No Character Instance has id nope. No Character Instance is running."
        );
    }

    /// A known id is accepted by a tool that does not target anything.
    #[test]
    fn a_known_instance_id_passes_through_a_sensing_tool() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![window("Terminal", 0.0, 0.0, 10.0, 10.0)]);
        let infos = [InstanceInfo {
            id: "pip".to_string(),
            name: "Pip".to_string(),
            ..Default::default()
        }];
        let mut context = test_context(&temp, &source, &infos);

        let result = dispatch("list_windows", json!({"instance_id": "pip"}), &mut context)
            .expect("a known id is accepted");

        assert_eq!(result["windows"][0]["owner"], "Terminal");
        let refused = dispatch("list_windows", json!({"instance_id": 5}), &mut context)
            .expect_err("an id that is not a string slipped past the check");
        assert_eq!(refused.code, ErrorCode::InvalidArguments);
        assert_eq!(refused.message, "instance_id must be a string");
    }

    /// Optional, so the stdio shim in the user's own sessions keeps working
    /// with one Instance (ADR-0026).
    #[test]
    fn every_tool_advertises_an_optional_instance_id() {
        for tool in list_tools() {
            let schema = &tool.input_schema;
            assert_eq!(
                schema["properties"]["instance_id"]["type"], "string",
                "{}",
                tool.name
            );
            let required = schema["required"].as_array().cloned().unwrap_or_default();
            assert!(
                !required.contains(&json!("instance_id")),
                "{} requires instance_id",
                tool.name
            );
        }
    }

    /// Break: success on an empty roster lets a stdio verification pass
    /// against nothing at all.
    #[test]
    fn an_empty_roster_fails_and_says_no_instance_is_running() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let args = json!({"behavior": "wave"});
        let played = dispatch("play_behavior", args, &mut context).expect("dispatch succeeds");

        assert_eq!(played["success"], false);
        assert_eq!(played["behavior"], "wave");
        assert_eq!(
            played["reason"],
            "No Character Instance is running, so nothing changed on screen"
        );
    }

    /// Fails if an empty behavior reports success, or stops being a
    /// DispatchError.
    #[test]
    fn an_empty_behavior_keeps_the_existing_failure_shape() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let behavior_args = json!({"behavior": ""});
        let behavior_result =
            dispatch("play_behavior", behavior_args, &mut context).expect("dispatch succeeds");
        assert_eq!(behavior_result["success"], false);
        assert_eq!(behavior_result["behavior"], "");
        assert_eq!(behavior_result["reason"], "The behavior name is empty");
    }

    /// Break: success with no handle. The stdio path runs this way, so a
    /// populated roster must not be enough to claim the Expression landed.
    #[test]
    fn a_roster_without_a_handle_fails_and_says_there_is_no_connection() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let infos = [InstanceInfo {
            id: "instance-1".to_string(),
            name: "Buddy".to_string(),
            ..Default::default()
        }];
        // `test_context` carries `expression: None`, which is the shipped
        // stdio shape.
        let mut context = test_context(&temp, &source, &infos);

        let played = dispatch("play_behavior", json!({"behavior": "wave"}), &mut context)
            .expect("dispatch succeeds");
        assert_eq!(played["success"], false);
        assert_eq!(
            played["reason"],
            "No live connection to the running app, so nothing changed on screen"
        );
    }

    /// Break: success on a stale roster entry. The caller's roster is a
    /// snapshot, so the Instance it names can retire before the enqueue.
    #[test]
    fn a_retired_instance_fails_and_says_it_is_no_longer_running() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);

        // The snapshot still names an Instance the live Roster no longer holds.
        let roster_info = [InstanceInfo {
            id: "retired-instance".to_string(),
            name: "Gone".to_string(),
            ..Default::default()
        }];
        let mut live = crate::roster::Roster::new();

        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(&mut live),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        let result = dispatch("play_behavior", json!({"behavior": "wave"}), &mut context)
            .expect("dispatch succeeds");

        assert_eq!(result["success"], false);
        assert_eq!(
            result["reason"],
            "Character Instance retired-instance is no longer running, so nothing changed on screen"
        );
    }

    #[test]
    fn an_undeclared_behavior_is_still_enqueued_and_the_engine_refuses() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let (mut roster, instance_id) = test_roster_with_grounded_instance("TestBuddy");

        let roster_info = roster
            .list()
            .into_iter()
            .map(|(id, name)| InstanceInfo {
                id,
                name,
                ..Default::default()
            })
            .collect::<Vec<_>>();

        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &roster_info,
            expression: Some(&mut roster),
            placement: PlacementQuery::empty(),
            caffeinate: None,
        };

        let args = json!({"behavior": "undeclared_behavior"});
        let result = dispatch("play_behavior", args, &mut context).expect("dispatch succeeds");

        assert_eq!(result["success"], true);
        assert_eq!(result["behavior"], "undeclared_behavior");
        drop(context);

        let snapshot = test_snapshot();
        let instance = roster
            .get_mut(&instance_id)
            .expect("instance still in roster");
        let frame = instance.tick(&snapshot);
        assert_eq!(frame.behavior, None);
    }

    /// The payload a caller reads: names, which display, the others, and feet
    /// already in that display. No subtraction left to do.
    #[test]
    fn whereabouts_returns_the_display_and_the_feet_with_nothing_left_to_convert() {
        use crate::engine::Point;

        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
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
        let names = [Some("Left".to_string()), None];
        let spots = [FeetAt {
            id: "pip".to_string(),
            name: "Pip".to_string(),
            at: Point {
                x: 2000.0,
                y: 100.0,
            },
        }];
        let mut context = DispatchContext {
            window_source: &source,
            memory_path: temp.path().join("memory.md"),
            denylist: DenyList::default(),
            roster: &[],
            expression: None,
            placement: PlacementQuery {
                frames: &frames,
                usable: &frames,
                names: &names,
                instances: &spots,
            },
            caffeinate: None,
        };

        let result = dispatch("whereabouts", json!({}), &mut context).expect("dispatch succeeds");

        assert_eq!(
            result,
            json!({
                "unit": "points",
                "displays": [
                    {
                        "index": 0,
                        "name": "Left",
                        "origin_x": 0.0,
                        "origin_y": 0.0,
                        "width": 1920.0,
                        "height": 1080.0
                    },
                    {
                        "index": 1,
                        "name": null,
                        "origin_x": 1920.0,
                        "origin_y": 0.0,
                        "width": 1920.0,
                        "height": 1080.0
                    }
                ],
                "instances": [
                    {
                        "id": "pip",
                        "name": "Pip",
                        "on_display": 1,
                        "other_displays": [0],
                        "x": 80.0,
                        "y": 100.0
                    }
                ]
            })
        );
    }
}
