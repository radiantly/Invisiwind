//! Everything here assumes the target runs the *same* architecture as this build: system DLLs
//! share a base across processes of a given architecture, so our own `GetProcAddress` result is
//! also the target's. Cross-architecture targets are handed to an architecture-appropriate build
//! of Invisiwind — see [`crate::delegate`].

use std::error;
use std::ffi::{CString, c_void};
use std::mem::size_of;
use std::sync::OnceLock;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            SystemInformation::{
                IMAGE_FILE_MACHINE, IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64,
                IMAGE_FILE_MACHINE_I386, IMAGE_FILE_MACHINE_UNKNOWN,
            },
            Threading::{
                CREATE_SUSPENDED, CreateRemoteThread, IsWow64Process2, LPTHREAD_START_ROUTINE,
                OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_CREATE_THREAD,
                PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
                ResumeThread, WaitForSingleObject,
            },
        },
    },
    core::{PCSTR, w},
};

type Result<T> = std::result::Result<T, Box<dyn error::Error>>;

const PROCESS_ACCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(
    PROCESS_CREATE_THREAD.0
        | PROCESS_QUERY_INFORMATION.0
        | PROCESS_VM_OPERATION.0
        | PROCESS_VM_WRITE.0
        | PROCESS_VM_READ.0,
);

/// A remote call should be near-instant. Anything longer means the target is wedged, and we'd
/// rather report that than hang the GUI's worker thread forever.
const CALL_TIMEOUT_MS: u32 = 3000;

/// The architecture this binary was built for. A target reporting anything else has to be
/// handled by a different build, because neither the resolved export addresses nor the stub
/// encoding carry across architectures.
#[cfg(target_arch = "x86_64")]
pub const HOST_MACHINE: IMAGE_FILE_MACHINE = IMAGE_FILE_MACHINE_AMD64;
#[cfg(target_arch = "x86")]
pub const HOST_MACHINE: IMAGE_FILE_MACHINE = IMAGE_FILE_MACHINE_I386;
#[cfg(target_arch = "aarch64")]
pub const HOST_MACHINE: IMAGE_FILE_MACHINE = IMAGE_FILE_MACHINE_ARM64;

/// A human-readable name for a machine type, for error messages and delegation lookups.
pub fn machine_name(machine: IMAGE_FILE_MACHINE) -> &'static str {
    match machine {
        IMAGE_FILE_MACHINE_AMD64 => "x64",
        IMAGE_FILE_MACHINE_I386 => "x86",
        IMAGE_FILE_MACHINE_ARM64 => "arm64",
        _ => "unknown",
    }
}

/// A user-mode APC passes three arguments to its routine (rcx/rdx/r8 on x64)
const APC_ARGS: usize = 3;

/// Reads a NUL-terminated string out of one of the fixed-size UTF-16 buffers in the
/// ToolHelp entry structs.
fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Closes a raw `HANDLE` on drop. The `windows` crate hands back bare handles, and we open
/// several on paths that can fail partway through.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// An open handle to a target process, plus the architecture it actually runs as.
pub struct Process {
    handle: OwnedHandle,
    pub pid: u32,
    /// The instruction set the process runs as, which is not necessarily the host's: a WOW64
    /// process on x64 reports x86, and an emulated x64 process on ARM64 reports x64.
    pub machine: IMAGE_FILE_MACHINE,
}

impl Process {
    pub fn open(pid: u32) -> Result<Process> {
        let handle = unsafe { OpenProcess(PROCESS_ACCESS, false, pid) }?;
        let handle = OwnedHandle(handle);

        // IsWow64Process2 reports UNKNOWN for a process running natively, in which case the
        // architecture is the machine's own.
        let mut process_machine = IMAGE_FILE_MACHINE_UNKNOWN;
        let mut native_machine = IMAGE_FILE_MACHINE_UNKNOWN;
        unsafe { IsWow64Process2(handle.0, &mut process_machine, Some(&mut native_machine)) }?;

        let machine = if process_machine == IMAGE_FILE_MACHINE_UNKNOWN {
            native_machine
        } else {
            process_machine
        };

        Ok(Process {
            handle,
            pid,
            machine,
        })
    }

    fn raw(&self) -> HANDLE {
        self.handle.0
    }
}

/// Finds processes whose executable name contains `needle`, ignoring case.
pub fn find_pids_by_name(needle: &str) -> Result<Vec<u32>> {
    let snapshot = OwnedHandle(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }?);

    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe { Process32FirstW(snapshot.0, &mut entry) }?;

    let needle = needle.to_lowercase();
    let mut pids = Vec::new();
    loop {
        if wide_to_string(&entry.szExeFile)
            .to_lowercase()
            .contains(&needle)
        {
            pids.push(entry.th32ProcessID);
        }

        if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
            break;
        }
    }

    Ok(pids)
}

/// Resolves a `user32` export to the address it lives at inside a same-architecture process.
pub fn resolve_user32_export(name: &str) -> Result<u64> {
    let module = unsafe { GetModuleHandleW(w!("user32.dll")) }?;
    let symbol = CString::new(name)?;
    match unsafe { GetProcAddress(module, PCSTR(symbol.as_ptr() as *const u8)) } {
        Some(addr) => Ok(addr as usize as u64),
        None => Err(format!("user32.dll does not export {}", name).into()),
    }
}

/// `NtQueueApcThread(ThreadHandle, ApcRoutine, Arg1, Arg2, Arg3) -> NTSTATUS`. Undocumented, so
/// resolved from ntdll at runtime rather than imported.
type NtQueueApcThread = unsafe extern "system" fn(
    HANDLE,
    *const c_void,
    *const c_void,
    *const c_void,
    *const c_void,
) -> i32;

/// The ntdll routines the APC call needs. ntdll is mapped at the same base in every
/// same-architecture process for the life of the boot, so our own addresses are also the
/// target's — no per-process resolution required.
struct NtdllProcs {
    /// `NtTestAlert`, used as the remote thread's start address. It drains the thread's APC
    /// queue, which is how our queued call actually runs.
    test_alert: usize,
    queue_apc: NtQueueApcThread,
}

// SAFETY: both fields are code addresses in ntdll, valid for the process lifetime and shared
// across threads; the function pointer is only ever called through a valid thread handle.
unsafe impl Sync for NtdllProcs {}
unsafe impl Send for NtdllProcs {}

fn ntdll_procs() -> Result<&'static NtdllProcs> {
    static PROCS: OnceLock<std::result::Result<NtdllProcs, String>> = OnceLock::new();

    PROCS
        .get_or_init(|| {
            let module = unsafe { GetModuleHandleW(w!("ntdll.dll")) }.map_err(|e| e.to_string())?;
            let resolve = |name: &str| -> std::result::Result<usize, String> {
                let symbol = CString::new(name).map_err(|e| e.to_string())?;
                unsafe { GetProcAddress(module, PCSTR(symbol.as_ptr() as *const u8)) }
                    .map(|p| p as usize)
                    .ok_or_else(|| format!("ntdll.dll does not export {name}"))
            };

            Ok(NtdllProcs {
                test_alert: resolve("NtTestAlert")?,
                queue_apc: unsafe {
                    std::mem::transmute::<usize, NtQueueApcThread>(resolve("NtQueueApcThread")?)
                },
            })
        })
        .as_ref()
        .map_err(|e| e.clone().into())
}

/// Runs `fn_addr(args..)` inside `process` via a queued user-mode APC.
pub fn call_remote(process: &Process, fn_addr: u64, args: &[u64]) -> Result<()> {
    debug_assert_eq!(
        process.machine, HOST_MACHINE,
        "cross-architecture targets must be delegated, not called directly"
    );
    assert!(
        args.len() <= APC_ARGS,
        "an APC passes at most {APC_ARGS} args"
    );

    let procs = ntdll_procs()?;

    // Suspended so the APC is queued before NtTestAlert would drain it; a plain start would race.
    let start: LPTHREAD_START_ROUTINE = Some(unsafe { std::mem::transmute(procs.test_alert) });
    let thread = OwnedHandle(unsafe {
        CreateRemoteThread(
            process.raw(),
            None,
            0,
            start,
            None,
            CREATE_SUSPENDED.0,
            None,
        )
    }?);

    let mut apc_args = [0u64; APC_ARGS];
    apc_args[..args.len()].copy_from_slice(args);

    let status = unsafe {
        (procs.queue_apc)(
            thread.0,
            fn_addr as *const c_void,
            apc_args[0] as *const c_void,
            apc_args[1] as *const c_void,
            apc_args[2] as *const c_void,
        )
    };
    if status < 0 {
        return Err(format!(
            "NtQueueApcThread failed for process {}: {status:#010X}",
            process.pid
        )
        .into());
    }

    unsafe { ResumeThread(thread.0) };

    if unsafe { WaitForSingleObject(thread.0, CALL_TIMEOUT_MS) } != WAIT_OBJECT_0 {
        return Err(format!("remote call in process {} timed out", process.pid).into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_known_user32_export() {
        assert!(resolve_user32_export("SetWindowDisplayAffinity").unwrap() != 0);
    }

    #[test]
    fn rejects_a_missing_user32_export() {
        assert!(resolve_user32_export("NoSuchExportZZZ").is_err());
    }

    #[test]
    fn resolves_ntdll_procs() {
        // Both NtTestAlert and NtQueueApcThread must resolve, or call_remote can never work.
        assert!(ntdll_procs().unwrap().test_alert != 0);
    }
}
