use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId,
};

pub fn foreground_pid() -> Option<u32> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }

        let mut pid = 0_u32;
        let thread_id = GetWindowThreadProcessId(hwnd, &mut pid);
        (thread_id != 0 && pid != 0).then_some(pid)
    }
}
