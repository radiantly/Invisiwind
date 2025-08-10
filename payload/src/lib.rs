use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE},
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
