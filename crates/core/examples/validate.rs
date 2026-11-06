//! Validate a Character Package directory with `character::load`.
//!
//! ```sh
//! cargo run -p fidget-core --example validate -- characters/cat
//! ```
//!
//! `scripts/import-pet.py` runs this before declaring an import a success;
//! the shell's own reader adds archive and size handling this example does
//! not need, so the walk here stays a plain recursive read.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use fidget_core::character;
use fidget_core::engine::{BehaviorProposal, Engine, Point, Rect, WorldSnapshot};

fn collect(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(root, &path, files)?;
        } else {
            let name = path
                .strip_prefix(root)
                .expect("every walked path starts at the root")
                .components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.insert(name, std::fs::read(&path)?);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let Some(root) = std::env::args().nth(1) else {
        eprintln!("usage: validate <package-directory>");
        return ExitCode::FAILURE;
    };
    let root = Path::new(&root);
    let mut files = BTreeMap::new();
    if let Err(why) = collect(root, root, &mut files) {
        eprintln!("{}: {why}", root.display());
        return ExitCode::FAILURE;
    }
    match character::load(&files) {
        Ok(character) => {
            println!("{} is a valid Character Package", character.name);
            if let Some(behavior) = std::env::args().nth(2) {
                match played_animation(&character, &behavior) {
                    Some(animation) => println!("played {behavior} -> {animation}"),
                    None => {
                        eprintln!("{behavior} did not draw a named animation");
                        return ExitCode::FAILURE;
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(errors) => {
            eprintln!("{} is not a valid Character Package:", root.display());
            for error in errors {
                eprintln!("  - {error}");
            }
            ExitCode::FAILURE
        }
    }
}

/// Land, propose `behavior`, and return the animation that step draws.
fn played_animation(character: &character::Character, behavior: &str) -> Option<String> {
    let ground = WorldSnapshot {
        displays: vec![Rect {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        }],
        elapsed_ms: 100,
        ..WorldSnapshot::default()
    };
    let mut engine = Engine::new(Point { x: 100.0, y: 0.0 })
        .with_behaviors(character.behaviors.clone())
        .with_variant_seed(1)
        .with_idle_plan(character.idle_plan());
    let mut landed = false;
    for _ in 0..80 {
        let frame = engine.tick(&ground);
        if frame.animation == "idle" && frame.playing_gesture.is_none() {
            landed = true;
            break;
        }
    }
    if !landed {
        return None;
    }
    for tick in 0..40 {
        let frame = engine.tick(&WorldSnapshot {
            proposal: (tick == 0).then(|| BehaviorProposal {
                behavior: behavior.to_string(),
                dialogue: None,
                direction: None,
            }),
            ..ground.clone()
        });
        let Some(name) = frame.playing_gesture.as_deref() else {
            continue;
        };
        let drawn = character.draw(name, frame.animation_ms, frame.ring_choice(), frame.facing)?;
        if drawn.index == 0 && drawn.animation == name {
            return Some(drawn.animation.to_string());
        }
    }
    None
}
