use dll_syringe::{
    Syringe,
    process::OwnedProcess,
    rpc::{RawRpcFunctionPtr, RemoteRawProcedure},
};
use std::env;
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, TRUE},
        Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowDisplayAffinity, GetWindowTextW, GetWindowThreadProcessId,
            IsWindowVisible,
        },
    },
    core::BOOL,
};

#[derive(Debug)]
pub struct WindowInfo {
    pub hwnd: u32,
    pub title: String,
    pub pid: u32,
    pub hidden: bool,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // skip invisible windows
    let is_visible = unsafe { IsWindowVisible(hwnd) };
    if !is_visible.as_bool() {
        return TRUE;
    }

    // get title
    let mut buf = [0u16; 128];
    let title_len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if title_len == 0 {
        return TRUE;
    }

    let title = String::from_utf16_lossy(&buf[..title_len as usize]);

    // skip cloaked windows (Calculator, Settings)
    let mut cloaked: u32 = 0;
    let result_get = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as _,
            std::mem::size_of::<u32>() as u32,
        )
    };

    if result_get.is_err() || cloaked != 0 {
        return TRUE;
    }

    let mut affinity: u32 = 0;
    let result_affinity = unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity as *mut _) };

    if result_affinity.is_err() {
        return TRUE;
    }
    let hidden = affinity != 0;

    // Get owning process ID
    let mut pid = 0u32;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };

    if thread_id == 0 {
        return TRUE;
    }

    // Recover our Vec<WindowInfo> from lparam and push.
    let out: &mut Vec<WindowInfo> = unsafe { &mut *(lparam.0 as *mut _) };
    out.push(WindowInfo {
        hwnd: hwnd.0 as u32,
        title,
        pid,
        hidden,
    });

    TRUE // continue enumeration
}

pub fn get_top_level_windows() -> Vec<WindowInfo> {
    let mut top_level_windows = Vec::new();

    unsafe {
        // Pass a pointer to our Vec via LPARAM.
        let param = LPARAM(&mut top_level_windows as *mut _ as isize);
        // Enumerate all *top-level* windows.
        let _ = EnumWindows(Some(enum_windows_proc), param);
    }
    top_level_windows
}

pub fn inject_and_get_remote_proc<F>(
    target_process: OwnedProcess,
    proc_name: &str,
) -> RemoteRawProcedure<F>
where
    F: RawRpcFunctionPtr,
{
    let syringe = Syringe::for_process(target_process);

    let mut dll_path = env::current_exe().unwrap();
    dll_path.pop();
    dll_path.push("payload.dll");

    let injected_payload = syringe.find_or_inject(dll_path).unwrap();
    return unsafe { syringe.get_raw_procedure::<F>(injected_payload, proc_name) }
        .unwrap()
        .unwrap();
}

pub fn set_window_props(target_process: OwnedProcess, hwnds: &[u32], hide: bool) {
    let remote_proc = inject_and_get_remote_proc::<extern "system" fn(HWND, bool) -> bool>(
        target_process,
        "SetWindowVisibility",
    );

    for hwnd in hwnds {
        remote_proc
            .call(HWND(hwnd.clone() as *mut _), hide)
            .unwrap();
    }
}

pub fn set_window_props_with_pid(pid: u32, hwnd: u32, hide: bool) {
    let target_process = OwnedProcess::from_pid(pid).unwrap();
    set_window_props(target_process, &[hwnd], hide);
}
