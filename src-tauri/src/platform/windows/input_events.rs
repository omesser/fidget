//! Where a mouse button went down, and a wake for the idle frame loop.
//!
//! A visible idle sprite is polled, up to a second apart while the cursor is
//! away. Hit-testing the cursor at that tick misses a drag that has already
//! left the sprite. The hook reads the cursor in the button-down, the same
//! point XI2 stores for `press_point`.
//!
//! Motion does not wake the loop. A wake per move would tick for every step
//! of the pointer across the desktop, which is the cost the back-off avoids.
//! The hook still runs for those moves and returns at once.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::Duration;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL,
};

use super::super::press_origin::{self, PressRecord};

static PRESS: Mutex<PressRecord> = Mutex::new(PressRecord::new());
static WAKE: OnceLock<SyncSender<()>> = OnceLock::new();

/// Consume the latest recent button-down in `GetCursorPos` screen pixels.
/// A missing, consumed, or expired record falls back to the sampled cursor.
pub fn press_point() -> Option<(i32, i32)> {
    PRESS.lock().unwrap_or_else(PoisonError::into_inner).at()
}

/// Store a button-down. The bool is whether this edge should wake the loop.
fn note_edge(message: u32, at: (i32, i32)) -> bool {
    PRESS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .observe(press_origin::edge_of(message), at)
}

/// Listen for button edges. `None` if the hook will not install, and the
/// frame loop keeps the poll it already has.
pub fn spawn_press_listener() -> Option<Receiver<()>> {
    let (wake, woken) = mpsc::sync_channel(1);
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::Builder::new()
        .name("mouse-hook".to_string())
        .spawn(move || listen(wake, ready_tx))
        .ok()?;
    match ready_rx.recv_timeout(Duration::from_secs(2)) {
        Ok(true) => Some(woken),
        _ => None,
    }
}

fn listen(wake: SyncSender<()>, ready: mpsc::Sender<bool>) {
    // Installed only after the sender is in place, so a press that lands
    // during setup still has somewhere to go.
    let _ = WAKE.set(wake);
    // SAFETY: the proc outlives the hook. It is unhooked before this thread
    // returns, and the process exit unhooks it if we do not get that far.
    // A null module names this executable, which is where the proc lives.
    let hook = unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(heard),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        eprintln!("overlay: mouse hook did not install");
        let _ = ready.send(false);
        return;
    }
    let _ = ready.send(true);

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe { UnhookWindowsHookEx(hook) };
}

/// Low-level mouse hook. Records a button-down and wakes on a button edge.
unsafe extern "system" fn heard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && lparam != 0 {
        // SAFETY: for a low-level mouse hook, `lparam` is a pointer to
        // `MSLLHOOKSTRUCT` for this call.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if note_edge(wparam as u32, (info.pt.x, info.pt.y)) {
            // Full means a wake is already waiting. Disconnected means the
            // frame loop has gone, and the next press still keeps its point.
            if let Some(sender) = WAKE.get() {
                let _ = sender.try_send(());
            }
        }
    }
    // SAFETY: a null hook handle asks for the next proc. Swallowing the
    // event would eat the click for every window.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
    };

    use crate::platform::press_origin::{self, PointerEdge};

    #[test]
    fn the_hook_messages_match_the_press_record() {
        assert_eq!(
            press_origin::edge_of(WM_LBUTTONDOWN),
            PointerEdge::PrimaryDown
        );
        assert_eq!(press_origin::edge_of(WM_LBUTTONUP), PointerEdge::PrimaryUp);
        assert_eq!(
            press_origin::edge_of(WM_RBUTTONDOWN),
            PointerEdge::SecondaryDown
        );
        assert_eq!(
            press_origin::edge_of(WM_RBUTTONUP),
            PointerEdge::SecondaryUp
        );
        assert_eq!(press_origin::edge_of(WM_MOUSEMOVE), PointerEdge::Other);
    }
}
