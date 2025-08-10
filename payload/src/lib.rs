use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongW, SetWindowDisplayAffinity, SetWindowLongW,
        WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    },
};

#[unsafe(no_mangle)]
pub extern "system" fn SetWindowVisibility(hwnd: HWND, hide: bool) -> bool {
    let dwaffinity = if hide {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        WDA_NONE
    };
    let result = unsafe { SetWindowDisplayAffinity(hwnd, dwaffinity) };
    return !result.is_err();
}

#[unsafe(no_mangle)]
pub extern "system" fn ShowOnTaskBar(hwnd: HWND, show: bool) -> bool {
    let mut style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) };
    if style == 0 {
        return false;
    }
    if show {
        style |= WS_EX_APPWINDOW.0 as i32;
        style &= (!WS_EX_TOOLWINDOW.0) as i32;
    } else {
        style |= WS_EX_TOOLWINDOW.0 as i32;
        style &= (!WS_EX_APPWINDOW.0) as i32;
    }
    unsafe { SetWindowLongW(hwnd, GWL_EXSTYLE, style) };
    true
}
