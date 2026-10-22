// One Deskemy per data folder. Two copies on one library each kept their own
// view of it and could lose each other's writes, so a second launch brings the
// running copy's window forward and exits instead. Per data folder: a portable
// copy, the installed one and a QA scratch library (DESKEMY_DATA_DIR) don't
// get in each other's way.
//
// The claim is a lock on `instance.lock` in the data folder, which the OS
// lets go of when the process ends, even in a crash. On Windows the holder
// writes its process id to `instance.pid`, for the second launch to find its
// window and raise it. Elsewhere the holder listens on `instance.sock`, and
// a second launch connecting to it asks it to show its window itself.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = "instance.lock";
const PID_FILE: &str = "instance.pid";
#[cfg(unix)]
const SOCKET_FILE: &str = "instance.sock";

/// This run's claim on the data folder; dropping it lets go (before the
/// relaunch after a backup import, so the new run can take it).
pub struct Instance {
    /// Held for the claim; read on unix (only the holder listens).
    #[cfg_attr(not(unix), allow(dead_code))]
    lock: Option<File>,
    #[cfg_attr(not(unix), allow(dead_code))]
    dir: PathBuf,
    /// Why there's no lock, for the log (which isn't set up yet at claim time).
    pub problem: Option<String>,
}

pub enum Claim {
    Ours(Instance),
    /// Another copy has it; its process id, if it has written one yet.
    Running(Option<u32>),
}

pub fn claim(dir: &Path) -> Claim {
    let unlocked = |why: String| Claim::Ours(Instance { lock: None, dir: dir.to_path_buf(), problem: Some(why) });
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
            Claim::Ours(Instance { lock: Some(file), dir: dir.to_path_buf(), problem: None })
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

#[cfg(unix)]
impl Instance {
    /// Answer later launches: each connection to the socket runs `show` (on
    /// the listener's thread). Only the lock's holder listens.
    pub fn listen(&self, show: impl Fn() + Send + 'static) {
        if self.lock.is_none() {
            return;
        }
        let path = self.dir.join(SOCKET_FILE);
        // A socket left by an earlier run: the lock says that run is over.
        let _ = std::fs::remove_file(&path);
        let listener = match std::os::unix::net::UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(e) => {
                tracing::warn!(error = %e, path = %path.display(), "a second launch can't show this window");
                return;
            }
        };
        let spawned = std::thread::Builder::new().name("instance".into()).spawn(move || {
            for stream in listener.incoming() {
                if stream.is_ok() {
                    show();
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "instance listener");
        }
    }
}

#[cfg(unix)]
impl Drop for Instance {
    fn drop(&mut self) {
        if self.lock.is_some() {
            let _ = std::fs::remove_file(self.dir.join(SOCKET_FILE));
        }
    }
}

/// Bring the running copy's window forward (restored if minimized). Windows
/// lets this process do it: the user just launched it.
#[cfg(windows)]
pub fn show_running(_dir: &Path, pid: Option<u32>) {
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

/// Ask the running copy to show its window (`Instance::listen`). If it can't
/// be reached, say why nothing opened, for a terminal launch.
#[cfg(unix)]
pub fn show_running(dir: &Path, _pid: Option<u32>) {
    if std::os::unix::net::UnixStream::connect(dir.join(SOCKET_FILE)).is_err() {
        eprintln!("Deskemy is already running.");
    }
}

#[cfg(not(any(windows, unix)))]
pub fn show_running(_dir: &Path, _pid: Option<u32>) {
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

    #[cfg(unix)]
    #[test]
    fn a_second_launch_asks_the_first_to_show_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let Claim::Ours(first) = claim(tmp.path()) else { panic!("the first claim is ours") };
        let (shown, asked) = std::sync::mpsc::channel();
        first.listen(move || shown.send(()).unwrap());

        let Claim::Running(pid) = claim(tmp.path()) else { panic!("the folder is taken") };
        super::show_running(tmp.path(), pid);
        asked.recv_timeout(std::time::Duration::from_secs(5)).expect("the first copy was asked");

        drop(first);
        assert!(!tmp.path().join(super::SOCKET_FILE).exists(), "letting go removes the socket");
    }
}
