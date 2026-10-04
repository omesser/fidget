//! The built `fidget.exe` opens a console only when debug assertions are on.
//! CI also runs this with them off, the switch a release build flips.
#![cfg(windows)]

const WINDOWS_GUI: u16 = 2;
const WINDOWS_CUI: u16 = 3;

#[test]
fn the_exe_has_a_console_only_with_debug_assertions() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_fidget")).expect("the built fidget.exe");
    // The PE header offset sits at 0x3C. Subsystem is 92 bytes past it, in
    // PE32 and PE32+ alike.
    let pe = u32::from_le_bytes(exe[0x3C..0x40].try_into().unwrap()) as usize;
    let subsystem = u16::from_le_bytes(exe[pe + 92..pe + 94].try_into().unwrap());
    let expected = if cfg!(debug_assertions) {
        WINDOWS_CUI
    } else {
        WINDOWS_GUI
    };
    assert_eq!(subsystem, expected, "2 is WINDOWS_GUI, 3 is the console");
}
