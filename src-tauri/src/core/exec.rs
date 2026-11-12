//! Trusted resolution of the external programs the app shells out to.
//!
//! `Command::new("powershell.exe")` does **not** name a fixed program on
//! Windows: the OS search order starts with the directory of the calling exe
//! and the *current working directory* before it reaches `%SystemRoot%`. The
//! CLI front end runs from whatever folder the user is in, so a
//! `powershell.exe` or `cmd.exe` sitting there (or written by any other
//! process into a folder we run from) would be executed with the developer's
//! credentials and the saved environment password in its environment.
//!
//! Everything that spawns a process therefore goes through this module:
//! Windows system tools resolve to their absolute `System32` path, and tools
//! that genuinely live on `PATH` are resolved by scanning `PATH` entries only
//! — never the working directory.
//!
//! Ported from Remote Desktop's `core::exec` (commit 76f65f0), plus
//! [`kill_with_parent`]: WinGUI holds long-lived `powershell.exe` session
//! workers, and they must not outlive the app if it crashes.

use std::path::{Path, PathBuf};
use std::process::Command;

/// `CREATE_NO_WINDOW` — keeps a GUI launch from flashing a console window.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows system directory (`%SystemRoot%\System32`), defaulting to the
/// documented location when `SystemRoot` is unset.
#[cfg(windows)]
fn system32_dir() -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32")
}

/// Absolute path of a program under `System32`, e.g. `mstsc.exe` or
/// `WindowsPowerShell\v1.0\powershell.exe`. Falls back to the bare name if the
/// file isn't there (an unusual install shouldn't break the feature outright).
#[cfg(windows)]
fn system32(relative: &str) -> String {
    let path = system32_dir().join(relative);
    if path.is_file() {
        path.to_string_lossy().into_owned()
    } else {
        Path::new(relative)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| relative.to_string())
    }
}

#[cfg(not(windows))]
fn system32(relative: &str) -> String {
    Path::new(relative)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| relative.to_string())
}

/// Windows PowerShell 5.1 — the interpreter every PS script in the app runs on.
pub fn powershell() -> String {
    system32(r"WindowsPowerShell\v1.0\powershell.exe")
}

/// The command interpreter used to run configured build commands.
/// Deliberately *not* `%ComSpec%`: that variable is attacker-settable.
pub fn cmd() -> String {
    system32("cmd.exe")
}

/// The OS Remote Desktop client.
pub fn mstsc() -> String {
    system32("mstsc.exe")
}

/// The `.rdp` signing tool.
pub fn rdpsign() -> String {
    system32("rdpsign.exe")
}

/// Executable extensions probed when resolving a `PATH` program on Windows.
#[cfg(windows)]
const PATH_EXTS: &[&str] = &[".exe", ".cmd", ".bat", ".com"];
#[cfg(not(windows))]
const PATH_EXTS: &[&str] = &[""];

/// Find `program` in `dirs`, trying each executable extension. **Relative
/// directories are skipped** — a `PATH` entry like `.` (or an empty entry,
/// which Windows treats as the current directory) must never be able to
/// satisfy a lookup, since the working directory is untrusted repo content.
fn resolve_in_dirs<I, P>(dirs: I, program: &str) -> Option<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    for dir in dirs {
        let dir = dir.as_ref();
        if dir.as_os_str().is_empty() || !dir.is_absolute() {
            continue;
        }
        for ext in PATH_EXTS {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Absolute path of a `PATH`-resolved program (git), or the bare name when it
/// isn't found — so the failure message stays the familiar "is it installed
/// and on PATH?" rather than a confusing path error.
pub fn on_path(program: &str) -> String {
    let path = std::env::var_os("PATH").unwrap_or_default();
    resolve_in_dirs(std::env::split_paths(&path), program)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| program.to_string())
}

/// `git`, resolved from `PATH` only.
pub fn git() -> String {
    on_path("git")
}

/// A `Command` for `program` with the no-console-window flag already applied.
pub fn command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Tie `child` to this process's lifetime: when the app exits **for any
/// reason** — including a crash or a kill from Task Manager, where no `Drop`
/// runs — Windows terminates the child too.
///
/// Implemented with one process-wide Job Object flagged
/// `KILL_ON_JOB_CLOSE`. The job handle is deliberately never closed: the OS
/// closes it when this process dies, and that is the trigger. Best-effort —
/// if the job can't be created the worker still runs, it just loses the
/// crash-safety net (normal shutdown still kills it via `Drop`).
#[cfg(windows)]
pub fn kill_with_parent(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // A raw handle value; `HANDLE` itself is not `Sync`.
    static JOB: OnceLock<Option<isize>> = OnceLock::new();

    let job = JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        Some(job.0 as isize)
    });

    if let Some(job) = job {
        unsafe {
            let _ = AssignProcessToJobObject(
                HANDLE(*job as *mut core::ffi::c_void),
                HANDLE(child.as_raw_handle() as *mut core::ffi::c_void),
            );
        }
    }
}

#[cfg(not(windows))]
pub fn kill_with_parent(_child: &std::process::Child) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_in_dirs_skips_relative_and_empty_entries() {
        let dir = std::env::temp_dir().join(format!("rd-exec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = "rd-fake-tool";
        let file = dir.join(format!("{name}{}", PATH_EXTS[0]));
        std::fs::write(&file, "x").unwrap();

        // A relative entry and an empty entry (the current directory on
        // Windows) must never satisfy the lookup.
        let found = resolve_in_dirs(vec![PathBuf::from(""), PathBuf::from("."), dir.clone()], name);
        assert_eq!(found.as_deref(), Some(file.as_path()));

        assert!(resolve_in_dirs(vec![PathBuf::from(""), PathBuf::from(".")], name).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_path_program_falls_back_to_the_bare_name() {
        assert_eq!(on_path("rd-definitely-not-installed"), "rd-definitely-not-installed");
    }

    #[cfg(windows)]
    #[test]
    fn system_tools_resolve_under_system32() {
        let ps = powershell();
        assert!(
            ps.to_lowercase().contains(r"system32\windowspowershell"),
            "powershell should resolve to System32: {ps}"
        );
        assert!(cmd().to_lowercase().ends_with(r"system32\cmd.exe"), "{}", cmd());
    }
}
