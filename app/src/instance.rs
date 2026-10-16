// One Deskemy per data folder. Two copies on one library each kept their own
// view of it and could lose each other's writes, so a second launch brings the
// running copy's window forward and exits instead. Per data folder: a portable
// copy, the installed one and a QA scratch library (DESKEMY_DATA_DIR) don't
// get in each other's way.
//
// The claim is a lock on `instance.lock` in the data folder, which the OS
// lets go of when the process ends, even in a crash. The holder writes its
// process id to `instance.pid`, for the second launch to find its window.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

const LOCK_FILE: &str = "instance.lock";
const PID_FILE: &str = "instance.pid";

/// This run's claim on the data folder; dropping it lets go (before the
/// relaunch after a backup import, so the new run can take it).
pub struct Instance {
    _lock: Option<File>,
    /// Why there's no lock, for the log (which isn't set up yet at claim time).
    pub problem: Option<String>,
}

pub enum Claim {
    Ours(Instance),
    /// Another copy has it; its process id, if it has written one yet.
    Running(Option<u32>),
}

pub fn claim(dir: &Path) -> Claim {
    let unlocked = |why: String| Claim::Ours(Instance { _lock: None, problem: Some(why) });
    let _ = std::fs::create_dir_all(dir);
    let opened = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(LOCK_FILE));
    let file = match opened {
        Ok(file) => file,
        // A read-only data folder, say: run without the guard rather than not at all.
        Err(e) => return unlocked(format!("can't open {LOCK_FILE}: {e}")),
    };
    match file.try_lock() {
        Ok(()) => {
            // Whole or not at all: written beside, then renamed over.
            let tmp = dir.join(format!("{PID_FILE}.tmp"));
            if std::fs::write(&tmp, std::process::id().to_string()).is_ok() {
                let _ = std::fs::rename(&tmp, dir.join(PID_FILE));
            }
            Claim::Ours(Instance { _lock: Some(file), problem: None })
        }
        Err(TryLockError::WouldBlock) => Claim::Running(running_pid(dir)),
        Err(TryLockError::Error(e)) => unlocked(format!("can't lock {LOCK_FILE}: {e}")),
    }
}

/// The running copy's pid. A copy that has only just started may not have
/// written it yet, so wait a moment for it. (It can still be an earlier run's,
/// briefly: `show_running` only raises a window that's Deskemy's own.)
fn running_pid(dir: &Path) -> Option<u32> {
    let path = dir.join(PID_FILE);
    for _ in 0..10 {
        if let Some(pid) = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()) {
            return Some(pid);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

/// Bring the running copy's window forward (restored if minimized). Windows
/// lets this process do it: the user just launched it.
#[cfg(windows)]
pub fn show_running(pid: Option<u32>) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible, ShowWindow, GW_OWNER,
        SW_RESTORE,
    };
    let Some(pid) = pid else { return };
    // An earlier run's pid could belong to another program by now.
    if !is_deskemy(pid) {
        return;
    }
    struct Search {
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
        // SAFETY: `data` is the &mut Search passed to EnumWindows below.
        let search = unsafe { &mut *(data as *mut Search) };
        let mut owner_pid = 0;
        // SAFETY: a window handle from EnumWindows; a u32 to fill.
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner_pid) };
        // Its main window: visible, top-level, not owned by another.
        // SAFETY: as above.
        if owner_pid == search.pid && unsafe { IsWindowVisible(hwnd) != 0 && GetWindow(hwnd, GW_OWNER).is_null() } {
            search.found = hwnd;
            return 0;
        }
        1
    }
    let mut search = Search { pid, found: std::ptr::null_mut() };
    // SAFETY: the callback only touches `search`, which outlives the call.
    unsafe { EnumWindows(Some(visit), &mut search as *mut Search as LPARAM) };
    if search.found.is_null() {
        return;
    }
    // SAFETY: a live top-level window of the other copy.
    unsafe {
        if IsIconic(search.found) != 0 {
            ShowWindow(search.found, SW_RESTORE);
        }
    }
    crate::bring_to_front(search.found);
}

/// Whether `pid` runs the same program as this process (by file name).
#[cfg(windows)]
fn is_deskemy(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let ours = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_ascii_lowercase()));
    // SAFETY: a handle opened, queried into a sized buffer, then closed.
    let theirs = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(process);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    };
    let theirs = theirs.and_then(|p| std::path::Path::new(&p).file_name().map(|n| n.to_ascii_lowercase()));
    ours.is_some() && ours == theirs
}

/// Elsewhere the window manager decides what comes forward; say why nothing
/// opened, for a terminal launch.
#[cfg(not(windows))]
pub fn show_running(_: Option<u32>) {
    eprintln!("Deskemy is already running.");
}

#[cfg(test)]
mod tests {
    use super::{claim, Claim};

    #[cfg(windows)]
    #[test]
    fn only_our_own_program_counts_as_the_running_copy() {
        assert!(super::is_deskemy(std::process::id()));
        // The System process (4), and a pid that's surely free.
        assert!(!super::is_deskemy(4));
        assert!(!super::is_deskemy(u32::MAX - 3));
    }

    #[test]
    fn a_second_claim_on_the_same_folder_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let Claim::Ours(first) = claim(tmp.path()) else { panic!("the first claim is ours") };
        match claim(tmp.path()) {
            Claim::Running(pid) => assert_eq!(pid, Some(std::process::id())),
            Claim::Ours(_) => panic!("the folder is taken"),
        }
        // Another folder is free.
        let other = tempfile::tempdir().unwrap();
        assert!(matches!(claim(other.path()), Claim::Ours(_)));
        // Letting go frees it (the relaunch after a backup import).
        drop(first);
        assert!(matches!(claim(tmp.path()), Claim::Ours(_)));
    }
}
