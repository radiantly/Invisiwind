use crate::{delegate, remote};
use std::error;
use tracing::{debug, warn};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM, TRUE, WPARAM},
        Graphics::{
            Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
            Gdi::{
                BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
                GetDIBits, GetObjectW, ReleaseDC,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GCLP_HICONSM, GET_CLASS_LONG_INDEX, GWL_EXSTYLE, GetIconInfo,
            GetWindowDisplayAffinity, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId,
            HICON, ICON_SMALL2, ICONINFO, IsWindowVisible, SendMessageW, WDA_EXCLUDEFROMCAPTURE,
            WDA_NONE, WM_GETICON, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
        },
    },
    core::BOOL,
};

/// `GetClassLongPtrW` exists only on 64-bit Windows. On 32-bit the pointer-sized accessor *is*
/// `GetClassLongW` — the Win32 headers alias one to the other.
#[cfg(target_pointer_width = "64")]
unsafe fn get_class_long_ptr(hwnd: HWND, index: GET_CLASS_LONG_INDEX) -> usize {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassLongPtrW(hwnd, index) }
}

#[cfg(target_pointer_width = "32")]
unsafe fn get_class_long_ptr(hwnd: HWND, index: GET_CLASS_LONG_INDEX) -> usize {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassLongW(hwnd, index) as usize }
}

#[derive(Debug)]
pub struct WindowInfo {
    pub hwnd: u32,
    pub title: String,
    pub pid: u32,
    pub hidden: bool,
}

#[tracing::instrument]
pub fn get_icon(hwnd: u32) -> Option<(usize, usize, Vec<u8>)> {
    let hwnd = HWND(hwnd.clone() as *mut _);
    let lresult =
        unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(ICON_SMALL2 as usize)), None) };

    let hicon = if lresult.0 == 0 {
        debug!("no hicon from sendmessage");

        let uresult = unsafe { get_class_long_ptr(hwnd, GCLP_HICONSM) };
        if uresult == 0 {
            debug!("no hicon from getclasslongptrsm");
            return None;
        }
        HICON(uresult as *mut _)
    } else {
        HICON(lresult.0 as *mut _)
    };

    let mut icon_info = ICONINFO::default();
    let info_result = unsafe { GetIconInfo(hicon, &mut icon_info as *mut _) };
    if let Err(err) = info_result {
        debug!("no iconinfo retrieved {:?}", err);
        return None;
    }

    let hdc = unsafe { GetDC(None) };
    if hdc.is_invalid() {
        debug!("no dc");
        return None;
    }

    let mut bitmap = BITMAP::default();
    let object_result = unsafe {
        GetObjectW(
            icon_info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut _ as *mut _),
        )
    };

    if object_result == 0 {
        debug!("no object");
        return None;
    }

    let mut bmi = BITMAPINFO::default();
    bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = bitmap.bmWidth;
    bmi.bmiHeader.biHeight = -bitmap.bmHeight;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB.0;

    let pixel_count = bitmap.bmWidth * bitmap.bmHeight;
    let mut pixels: Vec<u8> = vec![0; (pixel_count * 4) as usize];
    let _ = unsafe {
        GetDIBits(
            hdc,
            icon_info.hbmColor,
            0,
            bitmap.bmHeight as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi as *mut _,
            DIB_RGB_COLORS,
        )
    };

    for i in (0..pixels.len()).step_by(4) {
        (pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]) =
            (pixels[i + 2], pixels[i + 1], pixels[i], pixels[i + 3]);
    }

    let icon = Some((bitmap.bmWidth as usize, bitmap.bmHeight as usize, pixels));

    let _ = unsafe { ReleaseDC(None, hdc) };
    let _ = unsafe { DeleteObject(icon_info.hbmColor.into()) };
    let _ = unsafe { DeleteObject(icon_info.hbmMask.into()) };

    return icon;
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

    debug!("Window {:?} {:?} {:?}", hwnd.0, cloaked, title);

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

#[tracing::instrument]
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

pub struct Injector {}

impl Injector {
    /// Applies the taskbar/Alt+Tab ex-style flags
    fn set_taskbar_visibility(
        process: &remote::Process,
        set_window_long: u64,
        hwnd: u32,
        hide: bool,
    ) -> Result<(), Box<dyn error::Error>> {
        let mut style = unsafe { GetWindowLongW(HWND(hwnd as *mut _), GWL_EXSTYLE) };
        if style == 0 {
            warn!("could not read ex-style of hwnd {:#x}", hwnd);
            return Ok(());
        }

        if hide {
            style |= WS_EX_TOOLWINDOW.0 as i32;
            style &= !(WS_EX_APPWINDOW.0 as i32);
        } else {
            style |= WS_EX_APPWINDOW.0 as i32;
            style &= !(WS_EX_TOOLWINDOW.0 as i32);
        }

        remote::call_remote(
            process,
            set_window_long,
            &[
                hwnd as u64,
                // GWL_EXSTYLE is negative and must reach the callee sign-extended.
                GWL_EXSTYLE.0 as i64 as u64,
                style as u32 as u64,
            ],
        )?;

        Ok(())
    }

    pub fn set_window_props(
        pid: u32,
        hwnds: &[u32],
        hide: bool,
        hide_from_taskbar: Option<bool>,
    ) -> Result<(), Box<dyn error::Error>> {
        let process = remote::Process::open(pid)?;

        // Export addresses are architecture-specific, so anything that isn't our own
        // architecture is handed to the build that matches it.
        if process.machine != remote::HOST_MACHINE {
            debug!(
                "process {} is {}, delegating from {}",
                pid,
                remote::machine_name(process.machine),
                remote::machine_name(remote::HOST_MACHINE)
            );
            return delegate::run(process.machine, pid, hwnds, hide, hide_from_taskbar);
        }

        let set_display_affinity = remote::resolve_user32_export("SetWindowDisplayAffinity")?;

        let set_window_long = match hide_from_taskbar {
            Some(_) => Some(remote::resolve_user32_export("SetWindowLongW")?),
            None => None,
        };

        let affinity = if hide {
            WDA_EXCLUDEFROMCAPTURE
        } else {
            WDA_NONE
        };

        let mut unapplied = 0;
        for &hwnd in hwnds {
            remote::call_remote(
                &process,
                set_display_affinity,
                &[hwnd as u64, affinity.0 as u64],
            )?;

            let mut applied = 0u32;
            let read = unsafe { GetWindowDisplayAffinity(HWND(hwnd as *mut _), &mut applied) };
            if read.is_err() || applied != affinity.0 {
                warn!(
                    "SetWindowDisplayAffinity did not apply for hwnd {:#x} (got {:#x}, wanted {:#x})",
                    hwnd, applied, affinity.0
                );
                unapplied += 1;
            }

            if let (Some(hide_from_taskbar), Some(set_window_long)) =
                (hide_from_taskbar, set_window_long)
            {
                Self::set_taskbar_visibility(&process, set_window_long, hwnd, hide_from_taskbar)?;
            }
        }

        if unapplied > 0 {
            return Err(format!(
                "display affinity did not apply to {unapplied} of {} window(s)",
                hwnds.len()
            )
            .into());
        }

        Ok(())
    }

    pub fn set_window_props_with_pid(
        pid: u32,
        hwnd: u32,
        hide: bool,
        hide_from_taskbar: Option<bool>,
    ) -> Result<(), Box<dyn error::Error>> {
        Self::set_window_props(pid, &[hwnd], hide, hide_from_taskbar)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Kills the spawned window on the way out, including when an assertion unwinds, so a
    /// failing run doesn't leave Notepad behind.
    struct Spawned(std::process::Child);

    impl Drop for Spawned {
        fn drop(&mut self) {
            let _ = self.0.kill();
        }
    }

    /// Exercises the 3-argument remote `SetWindowLongW` path, which nothing else covers — the
    /// CLI never passes `hide_from_taskbar` on its own behalf. Needs an interactive desktop and
    /// spawns Notepad, so it's opt-in: `cargo test -- --ignored`.
    ///
    /// Same-architecture targets only. Cross-architecture delegation can't be driven from here
    /// because `current_exe()` is the test binary in `target/debug/deps`, where the sibling
    /// builds don't live; it's covered by invoking the built executables directly instead.
    #[test]
    #[ignore]
    fn taskbar_flags_round_trip() {
        let notepad = if cfg!(target_pointer_width = "64") {
            r"C:\Windows\System32\notepad.exe"
        } else {
            r"C:\Windows\SysWOW64\notepad.exe"
        };

        let child = Spawned(std::process::Command::new(notepad).spawn().unwrap());
        let pid = child.0.id();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let hwnd = loop {
            if let Some(w) = get_top_level_windows().into_iter().find(|w| w.pid == pid) {
                break w.hwnd;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{notepad} never opened a window"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let ex_style = || unsafe { GetWindowLongW(HWND(hwnd as *mut _), GWL_EXSTYLE) };

        let before = ex_style();
        Injector::set_window_props(pid, &[hwnd], true, Some(true)).unwrap();
        let hidden = ex_style();
        Injector::set_window_props(pid, &[hwnd], false, Some(false)).unwrap();
        let restored = ex_style();
        println!("before={before:#x} hidden={hidden:#x} restored={restored:#x}");

        assert_ne!(hidden & WS_EX_TOOLWINDOW.0 as i32, 0, "toolwindow set");
        assert_eq!(hidden & WS_EX_APPWINDOW.0 as i32, 0, "appwindow cleared");
        assert_eq!(
            restored & WS_EX_TOOLWINDOW.0 as i32,
            0,
            "toolwindow cleared"
        );
        assert_ne!(restored & WS_EX_APPWINDOW.0 as i32, 0, "appwindow restored");
    }
}
