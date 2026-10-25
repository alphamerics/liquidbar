use base64::{engine::general_purpose::STANDARD, Engine};
use std::{collections::HashMap, ffi::c_void, mem::size_of, path::Path};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::CloseHandle,
        Graphics::Gdi::{
            CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        },
        Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        UI::{
            Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON},
            WindowsAndMessaging::DestroyIcon,
        },
    },
};

pub struct ApplicationVisual {
    pub process_name: String,
    pub icon: Option<String>,
}

pub fn visual_for_process(process_id: u32) -> ApplicationVisual {
    let Some(executable) = executable_path(process_id) else {
        return ApplicationVisual {
            process_name: "Application".to_string(),
            icon: None,
        };
    };
    let executable_text = String::from_utf16_lossy(&executable[..executable.len() - 1]);
    let process_name = Path::new(&executable_text)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Application")
        .to_string();

    ApplicationVisual {
        process_name,
        icon: extract_for_executable(&executable),
    }
}

/// Extracts the app icon for an executable path as a PNG data URL (used for
/// pinned apps whose processes aren't running).
pub fn icon_for_path(path: &str) -> Option<String> {
    let mut wide: Vec<u16> = path.encode_utf16().collect();
    wide.push(0);
    extract_for_executable(&wide)
}

fn extract_for_executable(executable: &[u16]) -> Option<String> {
    let mut file_info = SHFILEINFOW::default();

    unsafe {
        let result = SHGetFileInfoW(
            PCWSTR(executable.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut file_info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if result == 0 || file_info.hIcon.0.is_null() {
            return None;
        }

        let encoded = icon_as_png_data_url(file_info.hIcon);
        let _ = DestroyIcon(file_info.hIcon);
        encoded
    }
}

pub fn executable_path(process_id: u32) -> Option<Vec<u16>> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
        let mut buffer = vec![0_u16; 32_768];
        let mut length = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        result.ok()?;
        buffer.truncate(length as usize);
        buffer.push(0);
        Some(buffer)
    }
}

/// Maps every running process id to its parent process id, taken from one
/// system snapshot. Used to walk windows owned by helper processes (Steam's
/// steamwebhelper, embedded WebView2 hosts) up to their launchable app.
pub fn parent_map() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return map;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }
    map
}

pub(crate) unsafe fn icon_as_png_data_url(
    icon: windows::Win32::UI::WindowsAndMessaging::HICON,
) -> Option<String> {
    let mut icon_info = windows::Win32::UI::WindowsAndMessaging::ICONINFO::default();
    windows::Win32::UI::WindowsAndMessaging::GetIconInfo(icon, &mut icon_info).ok()?;

    let mut bitmap = BITMAP::default();
    let copied = GetObjectW(
        icon_info.hbmColor,
        size_of::<BITMAP>() as i32,
        Some((&mut bitmap as *mut BITMAP).cast::<c_void>()),
    );
    if copied == 0 || bitmap.bmWidth <= 0 || bitmap.bmHeight <= 0 {
        let _ = DeleteObject(icon_info.hbmColor);
        let _ = DeleteObject(icon_info.hbmMask);
        return None;
    }

    let width = bitmap.bmWidth;
    let height = bitmap.bmHeight;
    let mut bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    let dc = CreateCompatibleDC(None);
    let copied_lines = GetDIBits(
        dc,
        icon_info.hbmColor,
        0,
        height as u32,
        Some(pixels.as_mut_ptr().cast::<c_void>()),
        &mut bitmap_info,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(dc);
    let _ = DeleteObject(icon_info.hbmColor);
    let _ = DeleteObject(icon_info.hbmMask);
    if copied_lines == 0 {
        return None;
    }

    let mut rgba = Vec::with_capacity(pixels.len());
    for row in pixels.chunks_exact(width as usize * 4).rev() {
        for pixel in row.chunks_exact(4) {
            rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }

    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut data, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }

    Some(format!("data:image/png;base64,{}", STANDARD.encode(data)))
}
