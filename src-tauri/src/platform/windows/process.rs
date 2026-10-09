//! The application name behind a window, from the process that owns it.
//! Shared by `window_source` and `sensing` because both feed the same
//! Sensing exclusion list, which matches application names not titles.

use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

/// Wide characters of process image path we read. Past `MAX_PATH`'s 260, since
/// a long path is legal; a name that does not fit is a name we do not have.
const MAX_PATH_LENGTH: usize = 512;

/// The application behind `hwnd`, as an application name.
/// Sensing exclusions match application names, not titles: `Notepad` never
/// matched `Untitled - Notepad`.
pub(super) fn window_owner(hwnd: HWND) -> Option<String> {
    let mut pid: u32 = 0;
    // SAFETY: GetWindowThreadProcessId writes the process ID into the
    // out-pointer, which lives until this function returns. hwnd is a window
    // handle the caller holds for the call.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    if pid == 0 {
        return None;
    }

    // PROCESS_QUERY_LIMITED_INFORMATION is the least right that reads an image
    // path and is granted across integrity levels, so an elevated window still
    // gets a name. SAFETY: OpenProcess returns a handle this function closes.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }

    let mut path_buf = [0u16; MAX_PATH_LENGTH];
    let mut len = MAX_PATH_LENGTH as u32;
    // SAFETY: QueryFullProcessImageNameW writes at most `len` wide characters
    // into the buffer we own and sets `len` to what it wrote. Both live until
    // this function returns, and `process` is the handle opened above.
    let read = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path_buf.as_mut_ptr(), &mut len)
    };
    // SAFETY: `process` came from OpenProcess above and is not used again.
    unsafe { CloseHandle(process) };

    if read == 0 {
        return None;
    }

    process_name(&String::from_utf16_lossy(&path_buf[..len as usize]))
}

/// The application name in a process image path: `notepad` from
/// `C:\Windows\System32\notepad.exe`.
fn process_name(image_path: &str) -> Option<String> {
    Path::new(image_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Win32 half needs a live desktop; the naming does not, and the naming
    /// is the half that used to hand back a title.
    #[test]
    fn process_name_is_the_image_file_stem() {
        assert_eq!(
            process_name("C:\\Windows\\System32\\notepad.exe").as_deref(),
            Some("notepad"),
            "an exclusion for `Notepad` matches this; `Untitled - Notepad` never did"
        );
        assert_eq!(
            process_name("C:/Program Files/Some App/some app.exe").as_deref(),
            Some("some app"),
            "forward slashes and spaces are both legal in a Windows image path"
        );
        assert_eq!(
            process_name("C:\\bin\\harness").as_deref(),
            Some("harness"),
            "an image path need not end in .exe"
        );
    }

    /// A path with no file name leaves nothing to name the owner with, and each
    /// caller decides what an unnameable window is.
    #[test]
    fn process_name_is_none_without_a_file() {
        assert_eq!(process_name(""), None);
        assert_eq!(process_name("C:\\"), None);
    }
}
