//! Minimal runtime FFI to libmpv via `libloading`. Only the entry points
//! Deskemy needs are bound, and the DLL is loaded at runtime so there is no
//! build-time link dependency or import library to manage.

use crate::error::{DeskemyError, Result};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::OnceLock;

// mpv_format
pub const MPV_FORMAT_DOUBLE: c_int = 5;
pub const MPV_FORMAT_INT64: c_int = 4;
pub const MPV_FORMAT_FLAG: c_int = 3;
pub const MPV_FORMAT_NONE: c_int = 0;

// mpv_event_id (subset)
pub const MPV_EVENT_NONE: c_int = 0;
pub const MPV_EVENT_SHUTDOWN: c_int = 1;
pub const MPV_EVENT_START_FILE: c_int = 6;
pub const MPV_EVENT_END_FILE: c_int = 7;
pub const MPV_EVENT_FILE_LOADED: c_int = 8;
pub const MPV_EVENT_IDLE: c_int = 11;
pub const MPV_EVENT_PLAYBACK_RESTART: c_int = 21;
pub const MPV_EVENT_PROPERTY_CHANGE: c_int = 22;

type Handle = c_void;

#[repr(C)]
pub struct MpvEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct MpvEventProperty {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

// mpv_end_file_reason
pub const MPV_END_FILE_REASON_EOF: c_int = 0;

#[repr(C)]
pub struct MpvEventEndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

struct Fns {
    _lib: libloading::Library,
    create: unsafe extern "C" fn() -> *mut Handle,
    initialize: unsafe extern "C" fn(*mut Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(*mut Handle),
    set_option_string: unsafe extern "C" fn(*mut Handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(*mut Handle, *const c_char, *const c_char) -> c_int,
    get_property_string: unsafe extern "C" fn(*mut Handle, *const c_char) -> *mut c_char,
    command: unsafe extern "C" fn(*mut Handle, *mut *const c_char) -> c_int,
    observe_property: unsafe extern "C" fn(*mut Handle, u64, *const c_char, c_int) -> c_int,
    wait_event: unsafe extern "C" fn(*mut Handle, f64) -> *mut MpvEvent,
    free: unsafe extern "C" fn(*mut c_void),
}
unsafe impl Send for Fns {}
unsafe impl Sync for Fns {}

static FNS: OnceLock<Option<Fns>> = OnceLock::new();

/// libmpv DLL is not bundled (it exceeds GitHub's file size limit). We discover
/// it from the user's system instead: an explicit override, next to our exe (if
/// an installer placed one), on PATH, or in common mpv install locations.
const DLL_NAMES: &[&str] = &["libmpv-2.dll", "mpv-2.dll", "libmpv.dll"];

fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            dirs.push(d.to_path_buf()); // app: target/<profile>/ (or install dir)
            dirs.push(d.join("..")); // examples: target/<profile>/examples/
        }
    }
    // A system mpv anywhere on PATH.
    if let Ok(path) = std::env::var("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    // Common Windows install locations.
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA", "ProgramData"] {
        if let Ok(base) = std::env::var(var) {
            dirs.push(PathBuf::from(&base).join("mpv"));
        }
    }
    if let Ok(up) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(&up).join("scoop/apps/mpv/current"));
        dirs.push(PathBuf::from(&up).join("scoop/shims"));
    }
    dirs
}

fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(p) = std::env::var("DESKEMY_LIBMPV") {
        v.push(PathBuf::from(p));
    }
    for dir in candidate_dirs() {
        for name in DLL_NAMES {
            v.push(dir.join(name));
        }
    }
    // Bare names → let the OS resolve via its own search path.
    for name in DLL_NAMES {
        v.push(PathBuf::from(name));
    }
    v
}

unsafe fn load() -> Result<Fns> {
    let mut last = String::from("no candidates");
    for path in candidates() {
        let lib = match libloading::Library::new(&path) {
            Ok(l) => l,
            Err(e) => {
                last = format!("{}: {e}", path.display());
                continue;
            }
        };
        macro_rules! sym {
            ($n:literal) => {{
                let s: libloading::Symbol<_> = lib
                    .get(concat!($n, "\0").as_bytes())
                    .map_err(|e| DeskemyError::Player(format!("libmpv symbol {}: {e}", $n)))?;
                *s
            }};
        }
        let create = sym!("mpv_create");
        let initialize = sym!("mpv_initialize");
        let terminate_destroy = sym!("mpv_terminate_destroy");
        let set_option_string = sym!("mpv_set_option_string");
        let set_property_string = sym!("mpv_set_property_string");
        let get_property_string = sym!("mpv_get_property_string");
        let command = sym!("mpv_command");
        let observe_property = sym!("mpv_observe_property");
        let wait_event = sym!("mpv_wait_event");
        let free = sym!("mpv_free");
        return Ok(Fns {
            create,
            initialize,
            terminate_destroy,
            set_option_string,
            set_property_string,
            get_property_string,
            command,
            observe_property,
            wait_event,
            free,
            _lib: lib,
        });
    }
    Err(DeskemyError::Player(format!(
        "libmpv not found. Install mpv (it provides libmpv-2.dll), or set the \
         DESKEMY_LIBMPV environment variable to the DLL path. Last attempt — {last}"
    )))
}

fn fns() -> Result<&'static Fns> {
    FNS.get_or_init(|| unsafe { load().ok() })
        .as_ref()
        .ok_or_else(|| DeskemyError::Player("libmpv is not available".into()))
}

/// Whether libmpv could be loaded (checked once, cached).
pub fn is_available() -> bool {
    fns().is_ok()
}

fn check(rc: c_int) -> Result<()> {
    if rc < 0 {
        Err(DeskemyError::Player(format!("mpv error code {rc}")))
    } else {
        Ok(())
    }
}

/// A libmpv client handle. The mpv client API is thread-safe, so this is Send+Sync.
pub struct Mpv {
    ctx: *mut Handle,
}
unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

impl Mpv {
    pub fn new() -> Result<Mpv> {
        let f = fns()?;
        let ctx = unsafe { (f.create)() };
        if ctx.is_null() {
            return Err(DeskemyError::Player("mpv_create returned null".into()));
        }
        Ok(Mpv { ctx })
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<()> {
        let f = fns()?;
        let n = cstr(name)?;
        let v = cstr(value)?;
        check(unsafe { (f.set_option_string)(self.ctx, n.as_ptr(), v.as_ptr()) })
    }

    pub fn initialize(&self) -> Result<()> {
        let f = fns()?;
        check(unsafe { (f.initialize)(self.ctx) })
    }

    pub fn set_property(&self, name: &str, value: &str) -> Result<()> {
        let f = fns()?;
        let n = cstr(name)?;
        let v = cstr(value)?;
        check(unsafe { (f.set_property_string)(self.ctx, n.as_ptr(), v.as_ptr()) })
    }

    pub fn get_property_string(&self, name: &str) -> Option<String> {
        let f = fns().ok()?;
        let n = cstr(name).ok()?;
        let ptr = unsafe { (f.get_property_string)(self.ctx, n.as_ptr()) };
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned();
        unsafe { (f.free)(ptr as *mut c_void) };
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }

    pub fn get_f64(&self, name: &str) -> Option<f64> {
        self.get_property_string(name).and_then(|s| s.parse().ok())
    }

    pub fn get_i64(&self, name: &str) -> Option<i64> {
        self.get_property_string(name).and_then(|s| s.parse().ok())
    }

    pub fn command(&self, args: &[&str]) -> Result<()> {
        let f = fns()?;
        let cstrs = args
            .iter()
            .map(|a| cstr(a))
            .collect::<Result<Vec<_>>>()?;
        let mut ptrs: Vec<*const c_char> = cstrs.iter().map(|c| c.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        check(unsafe { (f.command)(self.ctx, ptrs.as_mut_ptr()) })
    }

    pub fn observe_property(&self, id: u64, name: &str, format: c_int) -> Result<()> {
        let f = fns()?;
        let n = cstr(name)?;
        check(unsafe { (f.observe_property)(self.ctx, id, n.as_ptr(), format) })
    }

    /// Blocks up to `timeout` seconds for the next event. Returns a raw pointer
    /// owned by mpv (valid until the next `wait_event` on this handle).
    pub fn wait_event(&self, timeout: f64) -> *mut MpvEvent {
        match fns() {
            Ok(f) => unsafe { (f.wait_event)(self.ctx, timeout) },
            Err(_) => std::ptr::null_mut(),
        }
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if let Ok(f) = fns() {
            unsafe { (f.terminate_destroy)(self.ctx) };
        }
    }
}

fn cstr(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| DeskemyError::Player("interior NUL in string".into()))
}
