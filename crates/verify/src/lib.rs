//! `fidget-verify` — agent/CI verify entry (ADR-0027).
//!
//! Binary: `cargo run -p fidget-verify -- <doctor|units|overlay|poke|summon|scenario|cleanup>`
//!
//! What a caller may depend on — exit codes, `--json`, the `PROOF.md` section —
//! is [`contract`].

pub mod cleanup;
pub mod contract;
pub mod debug_ipc;
pub mod doctor;
pub mod gesture;
pub mod overlay;
pub mod paths;
pub mod poke;
pub mod proof;
pub mod scenario;
pub mod summon;
pub mod units;
