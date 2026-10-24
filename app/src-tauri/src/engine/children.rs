//! Ties the processes NASMirror starts (robocopy, restic) to its own life.
//!
//! Without it, closing the window, ending the task or a crash during a scan
//! or a copy left robocopy running on its own: unseen, still writing to the
//! destination (or deleting from it, in mirror mode) and competing for the
//! NAS with the next run, which started another one next to it.
//! `kill_on_drop` does not cover this: no destructor runs when the process
//! is ended.

use tokio::process::{Child, Command};

/// Starts `cmd`; the new process is ended by Windows when NASMirror ends.
pub fn spawn(cmd: &mut Command) -> std::io::Result<Child> {
    let child = cmd.spawn()?;
    #[cfg(windows)]
    tie_to_this_process(&child);
    Ok(child)
}

/// Puts `child` in a job object that kills its processes once its last
/// handle closes: this process holds that handle until it ends, however it
/// ends. Best effort: a process that cannot be added simply is not tied.
#[cfg(windows)]
fn tie_to_this_process(child: &Child) {
    use std::sync::OnceLock;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // The handle as an integer: `HANDLE` is a raw pointer, not `Sync`.
    static JOB: OnceLock<Option<isize>> = OnceLock::new();
    let job = JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
        .ok()?;
        Some(job.0 as isize)
    });
    if let (Some(job), Some(process)) = (job, child.raw_handle()) {
        unsafe {
            let _ = AssignProcessToJobObject(HANDLE(*job as *mut _), HANDLE(process as *mut _));
        }
    }
}
