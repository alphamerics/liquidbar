use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS};

pub fn count() -> u32 {
    unsafe { GetSystemMetrics(SM_CMONITORS).max(1) as u32 }
}
