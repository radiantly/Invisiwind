use dll_syringe::{
    Syringe,
    process::OwnedProcess,
    rpc::{RawRpcFunctionPtr, RemoteRawProcedure},
};
use std::env;
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
            EnumWindows, GCLP_HICONSM, GetClassLongPtrW, GetIconInfo, GetWindowDisplayAffinity,
            GetWindowTextW, GetWindowThreadProcessId, HICON, ICON_SMALL2, ICONINFO,
            IsWindowVisible, SendMessageW, WM_GETICON,
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

pub fn get_icon(hwnd: u32) -> Option<(usize, usize, Vec<u8>)> {
    let hwnd = HWND(hwnd.clone() as *mut _);
    let lresult =
        unsafe { SendMessageW(hwnd, WM_GETICON, Some(WPARAM(ICON_SMALL2 as usize)), None) };

    let hicon = if lresult.0 == 0 {
        println!("- no hicon from sendmessage");

        let uresult = unsafe { GetClassLongPtrW(hwnd, GCLP_HICONSM) };
        if uresult == 0 {
            println!("- no hicon from getclasslongptrsm");
            return None;
        }
        HICON(uresult as *mut _)
    } else {
        HICON(lresult.0 as *mut _)
    };

    let mut icon_info = ICONINFO::default();
    let info_result = unsafe { GetIconInfo(hicon, &mut icon_info as *mut _) };
    if let Err(err) = info_result {
        println!("- no iconinfo retrieved {:?}", err);
        return None;
    }

    let hdc = unsafe { GetDC(None) };
    if hdc.is_invalid() {
        println!("- no dc");
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
        println!("no object");
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

pub struct Injector {}

impl Injector {
    pub fn inject_and_get_remote_proc<F>(
        syringe: &Syringe,
        proc_name: &str,
    ) -> RemoteRawProcedure<F>
    where
        F: RawRpcFunctionPtr,
    {
        let mut dll_path = env::current_exe().unwrap();
        dll_path.pop();
        dll_path.push("utils.dll");

        let injected_payload = syringe.find_or_inject(dll_path).unwrap();

        return unsafe { syringe.get_raw_procedure::<F>(injected_payload, proc_name) }
            .unwrap()
            .unwrap();
    }

    pub fn set_window_props(
        target_process: OwnedProcess,
        hwnds: &[u32],
        hide: bool,
        hide_from_taskbar: Option<bool>,
    ) {
        let syringe = Syringe::for_process(target_process);

        let remote_proc = Self::inject_and_get_remote_proc::<extern "system" fn(HWND, bool) -> bool>(
            &syringe,
            "SetWindowVisibility",
        );

        let remote_proc2 = Self::inject_and_get_remote_proc::<extern "system" fn(HWND, bool) -> bool>(
            &syringe,
            "HideFromTaskbar",
        );

        for hwnd in hwnds {
            remote_proc
                .call(HWND(hwnd.clone() as *mut _), hide)
                .unwrap();

            if let Some(hide_from_taskbar) = hide_from_taskbar {
                remote_proc2
                    .call(HWND(hwnd.clone() as *mut _), hide_from_taskbar)
                    .unwrap();
            }
        }
    }

    pub fn set_window_props_with_pid(
        pid: u32,
        hwnd: u32,
        hide: bool,
        hide_from_taskbar: Option<bool>,
    ) {
        let target_process = OwnedProcess::from_pid(pid).unwrap();
        Self::set_window_props(target_process, &[hwnd], hide, hide_from_taskbar);
    }
}
