//! MCP tool dispatch: tool names and JSON arguments onto the handlers in
//! `tools`. Tested in-process, with no MCP transport, so it can live in core.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

use crate::caffeinate::{CaffeinateHandle, Fact};
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
        "speak" => {
            #[derive(Deserialize)]
            struct Args {
                message: String,
                #[serde(default)]
                instance_id: Option<String>,
            }
            let args: Args = parse_args(arguments, tool_name)?;

            let mut expression = context.expression.take();
            let result = tools::speak(
                &args.message,
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
        "play_behavior" => {
            #[derive(Deserialize)]
            struct Args {
                behavior: String,
                #[serde(default)]
                instance_id: Option<String>,
            }
            let args: Args = parse_args(arguments, tool_name)?;

            let mut expression = context.expression.take();
            let result = tools::play_behavior(
                &args.behavior,
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
            name: "speak".to_string(),
            description: "Make the Character speak a line of dialogue".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to speak"
                    }
                },
                "required": ["message"]
            }),
        },
        ToolInfo {
            name: "play_behavior".to_string(),
            description: "Play a named Behavior".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "behavior": {
                        "type": "string",
                        "description": "The name of the Behavior to play"
                    }
                },
                "required": ["behavior"]
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
            "description": "Your Character Instance id, named in your prompt. speak and play_behavior act on that Instance. Optional while only one Instance runs."
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
        let result = dispatch("speak", args, &mut context);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidArguments);
    }

    #[test]
    fn list_tools_includes_all_handler_tools() {
        let tools = list_tools();
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();

        assert_eq!(names.len(), 9);
        assert!(names.contains(&"speak"));
        assert!(names.contains(&"play_behavior"));
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
            Animation, Behavior, Character, CursorReaction, Primitive, DEFAULT_MODEL_BASE,
            DEFAULT_MODEL_POWER, DEFAULT_WEIGHT,
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
        }

        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let (roster, instance_id) = test_roster_with_grounded_instance("TestBuddy");

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

        let first = dispatch("speak", json!({"message": "First line"}), &mut context)
            .expect("dispatch succeeds");
        assert_eq!(first["success"], true);

        let snapshot = test_snapshot();
        let first_frame = shared
            .borrow_mut()
            .get_mut(&instance_id)
            .expect("instance still in roster")
            .tick(&snapshot);
        assert_eq!(first_frame.dialogue.as_deref(), Some("First line"));

        let second = dispatch("speak", json!({"message": "Second line"}), &mut context)
            .expect("dispatch succeeds");
        assert_eq!(second["success"], true);

        let second_frame = shared
            .borrow_mut()
            .get_mut(&instance_id)
            .expect("instance still in roster")
            .tick(&snapshot);
        assert_eq!(second_frame.dialogue.as_deref(), Some("Second line"));
    }

    #[test]
    fn speak_with_one_instance_enqueues_dialogue_and_plays_talk() {
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

        let args = json!({"message": "Hello, world!"});
        let result = dispatch("speak", args, &mut context).expect("dispatch succeeds");

        assert_eq!(result["success"], true);
        assert_eq!(result["message"], "Hello, world!");
        drop(context);

        let snapshot = test_snapshot();
        let instance = roster
            .get_mut(&instance_id)
            .expect("instance still in roster");
        let frame = instance.tick(&snapshot);
        assert!(frame.dialogue.is_some(), "Frame should carry dialogue");
        assert_eq!(frame.dialogue.as_ref().unwrap(), "Hello, world!");
        assert_eq!(frame.animation, "talk");
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

        let args = json!({"message": "Hello"});
        let result = dispatch("speak", args, &mut context).expect("dispatch succeeds");

        assert_eq!(result["success"], false);
        assert_eq!(result["message"], "Hello");
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
                json!({"message": "hi", "behavior": "wave", "instance_id": "nope"}),
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

        let args = json!({"message": "Hello, world"});
        let spoke = dispatch("speak", args, &mut context).expect("dispatch succeeds");

        assert_eq!(spoke["success"], false);
        assert_eq!(spoke["message"], "Hello, world");
        assert_eq!(
            spoke["reason"],
            "No Character Instance is running, so nothing changed on screen"
        );

        let args = json!({"behavior": "wave"});
        let played = dispatch("play_behavior", args, &mut context).expect("dispatch succeeds");

        assert_eq!(played["success"], false);
        assert_eq!(played["behavior"], "wave");
        assert_eq!(played["reason"], spoke["reason"]);
    }

    /// Fails if an empty message or behavior reports success, or stops being
    /// a DispatchError.
    #[test]
    fn empty_message_and_empty_behavior_keep_the_existing_failure_shape() {
        let temp = tempfile::tempdir().expect("temp dir is creatable");
        let source = fake_source(vec![]);
        let mut context = test_context(&temp, &source, &[]);

        let speak_args = json!({"message": ""});
        let speak_result = dispatch("speak", speak_args, &mut context).expect("dispatch succeeds");
        assert_eq!(speak_result["success"], false);
        assert_eq!(speak_result["message"], "");
        assert_eq!(speak_result["reason"], "The message is empty");

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

        let spoke = dispatch("speak", json!({"message": "Hello"}), &mut context)
            .expect("dispatch succeeds");
        assert_eq!(spoke["success"], false);
        assert_eq!(
            spoke["reason"],
            "No live connection to the running app, so nothing changed on screen"
        );

        let played = dispatch("play_behavior", json!({"behavior": "wave"}), &mut context)
            .expect("dispatch succeeds");
        assert_eq!(played["success"], false);
        assert_eq!(played["reason"], spoke["reason"]);
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

        let result = dispatch("speak", json!({"message": "Hello"}), &mut context)
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
