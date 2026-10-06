//! CLI entry for `fidget-verify`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use fidget_verify::cleanup;
use fidget_verify::contract::{Outcome, RunReport};
use fidget_verify::debug_ipc;
use fidget_verify::doctor;
use fidget_verify::overlay;
use fidget_verify::paths::{self, RunPaths};
use fidget_verify::poke;
use fidget_verify::proof;
use fidget_verify::scenario;
use fidget_verify::summon;
use fidget_verify::units;

#[derive(Debug, Parser)]
#[command(name = "fidget-verify")]
#[command(
    about = "Agent/CI verify entry (doctor / units / overlay / poke / summon / scenario / cleanup)"
)]
#[command(version)]
struct Cli {
    /// Override evidence directory. Scratch is the sibling `scratch` under the
    /// same parent (pass `<run-root>/evidence` so layout matches the default).
    #[arg(long, global = true, value_name = "PATH")]
    evidence_dir: Option<PathBuf>,

    /// Run id used in the default root `$TMPDIR/fidget-verify-$RUN_ID`.
    /// Default: UTC timestamp + process id.
    #[arg(long, global = true, value_name = "ID")]
    run_id: Option<String>,

    /// Print one machine-parseable result object on stdout instead of human progress.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Read-only readiness (layout, binary/cargo, OS tool SKIPs).
    Doctor,
    /// cargo test core + node tests + overlay-diagnostics script.
    Units,
    /// Dispatch platform verify-overlay leaf script; collect stamps into evidence.
    Overlay,
    /// Click the sprite for real; assert `verbs:.*Poke` in the app's trace.
    Poke,
    /// Double-click the sprite for real; assert `verbs:.*Summon`.
    Summon,
    /// Run one `scripts/scenarios` leaf for this host. Without `--go`, print
    /// its takeover header and skip.
    Scenario {
        /// Scenario name. macOS is `<name>.sh`, X11 `<name>.x11.sh`, Windows `<name>.win.ps1`.
        name: String,
        /// The owner said go for this run: build the binaries and take over the GUI.
        #[arg(long)]
        go: bool,
        /// Passed to the script after the binaries.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Kill recorded PIDs; remove scratch; keep evidence.
    Cleanup,
    /// Debug: place character at (x,y) in a running fidget instance.
    /// Requires fidget to run with FIDGET_DEBUG_IPC=1.
    Place {
        /// X coordinate
        #[arg(long)]
        x: i32,
        /// Y coordinate
        #[arg(long)]
        y: i32,
    },
    /// Debug: snapshot position/state from a running fidget instance.
    /// Requires fidget to run with FIDGET_DEBUG_IPC=1.
    Snapshot,
}

impl Commands {
    fn name(&self) -> &'static str {
        match self {
            Commands::Doctor => "doctor",
            Commands::Units => "units",
            Commands::Overlay => "overlay",
            Commands::Poke => "poke",
            Commands::Summon => "summon",
            Commands::Scenario { .. } => "scenario",
            Commands::Cleanup => "cleanup",
            Commands::Place { .. } => "place",
            Commands::Snapshot => "snapshot",
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let _ = e.print();
            // clap exits 2 on a usage error, and 2 is `skip` in this contract.
            // A bad flag proved nothing and broke nothing, it is a tool error.
            return if e.use_stderr() {
                ExitCode::from(Outcome::Error.exit_code())
            } else {
                ExitCode::SUCCESS
            };
        }
    };

    let paths = RunPaths::resolve(cli.evidence_dir, cli.run_id);
    let mut report = RunReport::new(cli.command.name(), &paths, cli.json);

    if let Err(e) = paths.ensure_dirs() {
        report.check(
            Outcome::Error,
            "evidence dirs",
            &format!("cannot create run dirs: {e}"),
        );
    } else {
        match paths::discover_repo_root() {
            Ok(repo_root) => match &cli.command {
                Commands::Doctor => doctor::run(&repo_root, &mut report),
                Commands::Units => units::run(&repo_root, &mut report),
                Commands::Overlay => overlay::run(&repo_root, &mut report),
                Commands::Poke => poke::run(&repo_root, &mut report),
                Commands::Summon => summon::run(&repo_root, &mut report),
                Commands::Scenario { name, go, args } => {
                    scenario::run(&repo_root, name, *go, args, &mut report)
                }
                Commands::Cleanup => cleanup::run(&mut report),
                Commands::Place { x, y } => return ExitCode::from(debug_ipc::place(*x, *y)),
                Commands::Snapshot => return ExitCode::from(debug_ipc::snapshot()),
            },
            // Doctor's product is the layout report, so it still runs and
            // records the layout failure itself.
            Err(e) => match &cli.command {
                Commands::Doctor => doctor::run(Path::new("."), &mut report),
                _ => report.check(Outcome::Error, "repo root", &e),
            },
        }
    }

    let _ = proof::append_proof(&report);
    report.emit();
    ExitCode::from(report.outcome().exit_code())
}
