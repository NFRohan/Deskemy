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
}

pub enum Claim {
    Ours(Instance),
    /// Another copy has it; its process id, if it wrote one yet.
    Running(Option<u32>),
}

pub fn claim(dir: &Path) -> Claim {
    let _ = std::fs::create_dir_all(dir);
    let opened = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(LOCK_FILE));
    let file = match opened {
        Ok(file) => file,
        // A read-only data folder, say: run without the guard rather than not at all.
        Err(e) => {
            eprintln!("deskemy: can't open {LOCK_FILE}: {e}; running without the single-instance lock");
            return Claim::Ours(Instance { _lock: None });
        }
    };
    match file.try_lock() {
        Ok(()) => {
            let _ = std::fs::write(dir.join(PID_FILE), std::process::id().to_string());
            Claim::Ours(Instance { _lock: Some(file) })
        }
        Err(TryLockError::WouldBlock) => {
            let pid = std::fs::read_to_string(dir.join(PID_FILE)).ok().and_then(|s| s.trim().parse().ok());
            Claim::Running(pid)
        }
        Err(TryLockError::Error(e)) => {
            eprintln!("deskemy: can't lock {LOCK_FILE}: {e}; running without the single-instance lock");
            Claim::Ours(Instance { _lock: None })
        }
    }
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

#[cfg(not(windows))]
pub fn show_running(_: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::{claim, Claim};

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
