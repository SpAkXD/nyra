//! Resource limits for the programs that `nyra mcp` runs (`nyra_run`): at most `MEMORY` bytes of
//! memory and a budget of CPU time, on top of the wall-clock timeout of the caller.
//!
//! - Windows: a Job Object with a per-process memory and user-time limit. The child is assigned
//!   right after it starts, and the job kills it when it is closed (`Limited` drops it).
//! - Unix: `setrlimit` in the child before `exec`: the address space (the data segment for
//!   Node.js, which reserves far more address space than it uses) and the CPU seconds.
//!
//! These limits keep one program from taking the machine down. They are no sandbox: a program
//! can still read and write files. For untrusted code, run `nyra mcp` inside a container.

use std::process::{Child, Command};

/// The memory a program may use.
pub const MEMORY: u64 = 1 << 30;

/// The CPU time a program may use for a wall-clock `timeout` (its threads together, so Node.js
/// with its helper threads gets some room), in seconds.
pub fn cpu_seconds(timeout_ms: u64) -> u64 {
    timeout_ms.div_ceil(1000) * 2 + 2
}

/// Sets the limits that must be in place before the program starts (Unix). `node`: the program is
/// run by Node.js.
pub fn before_spawn(cmd: &mut Command, cpu_secs: u64, node: bool) {
    #[cfg(all(unix, target_pointer_width = "64"))]
    unix::limit(cmd, cpu_secs, node);
    #[cfg(not(all(unix, target_pointer_width = "64")))]
    let _ = (cmd, cpu_secs, node);
}

/// The limits of a running child; dropping it ends them (on Windows, closing the job kills a
/// child that is still running).
pub struct Limited {
    #[cfg(windows)]
    _job: Option<windows::Job>,
}

/// Puts a child that just started under the limits (Windows; Unix set them before it started).
pub fn after_spawn(child: &Child, cpu_secs: u64) -> Limited {
    #[cfg(windows)]
    {
        Limited { _job: windows::Job::new(child, cpu_secs) }
    }
    #[cfg(not(windows))]
    {
        let _ = (child, cpu_secs);
        Limited {}
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    type Handle = *mut c_void;

    #[repr(C)]
    #[derive(Default)]
    struct BasicLimits {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters {
        counts: [u64; 6],
    }

    /// JOBOBJECT_EXTENDED_LIMIT_INFORMATION
    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimits {
        basic: BasicLimits,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
    const JOB_OBJECT_LIMIT_PROCESS_TIME: u32 = 0x0000_0002;
    const JOB_OBJECT_LIMIT_PROCESS_MEMORY: u32 = 0x0000_0100;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(job: Handle, class: i32, info: *mut c_void, length: u32) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub struct Job(Handle);

    impl Job {
        /// A job with the limits, holding `child`; `None` if Windows refused (the program then
        /// runs with the timeout only).
        pub fn new(child: &Child, cpu_secs: u64) -> Option<Job> {
            // SAFETY: plain Win32 calls with valid arguments; the handle is closed by `Drop`.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let job = Job(job);
                let mut info = ExtendedLimits::default();
                info.basic.limit_flags =
                    JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_PROCESS_TIME | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                // in units of 100 ns
                info.basic.per_process_user_time_limit = (cpu_secs as i64).saturating_mul(10_000_000);
                info.process_memory_limit = super::MEMORY as usize;
                let size = std::mem::size_of::<ExtendedLimits>() as u32;
                let set =
                    SetInformationJobObject(job.0, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION, &mut info as *mut _ as *mut c_void, size);
                if set == 0 || AssignProcessToJobObject(job.0, child.as_raw_handle() as Handle) == 0 {
                    return None;
                }
                Some(job)
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateJobObjectW and is closed once
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    // the handle is only used to close the job, from whichever thread drops it
    unsafe impl Send for Job {}
}

#[cfg(all(unix, target_pointer_width = "64"))]
mod unix {
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    /// `struct rlimit` (`rlim_t` is 64 bits on 64-bit Linux and macOS).
    #[repr(C)]
    struct RLimit {
        cur: u64,
        max: u64,
    }

    extern "C" {
        fn setrlimit(resource: i32, limit: *const RLimit) -> i32;
    }

    const RLIMIT_CPU: i32 = 0;
    const RLIMIT_DATA: i32 = 2;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const RLIMIT_AS: Option<i32> = Some(9);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    const RLIMIT_AS: Option<i32> = Some(5);
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios")))]
    const RLIMIT_AS: Option<i32> = None;

    pub fn limit(cmd: &mut Command, cpu_secs: u64, node: bool) {
        let memory = super::MEMORY;
        // SAFETY: the closure runs in the child between fork and exec and only calls setrlimit,
        // which is async-signal-safe; it allocates nothing.
        unsafe {
            cmd.pre_exec(move || {
                let cpu = RLimit { cur: cpu_secs, max: cpu_secs + 1 };
                setrlimit(RLIMIT_CPU, &cpu);
                let mem = RLimit { cur: memory, max: memory };
                // Node.js reserves gigabytes of address space up front: limit what it writes instead
                match RLIMIT_AS {
                    Some(r) if !node => setrlimit(r, &mem),
                    _ => setrlimit(RLIMIT_DATA, &mem),
                };
                Ok(())
            });
        }
    }
}
