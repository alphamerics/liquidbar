//! Custom gadget source files: listing, reading and opening the folder.

use tauri::Manager;

/// One custom gadget source file in the app data dir (next to settings.json).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomGadgetFile {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// The app data dir: settings.json and the custom gadget sources live here.
fn gadgets_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve the app data dir: {error}"))?;
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir)
}

/// Whether `path` is a custom gadget source (extension-wise).
fn is_gadget_source(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| ["tsx", "jsx"].contains(&extension.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Lists the custom gadget sources in the app data dir. The gadget id and
/// display name derive from the file stem ("MyGadget.tsx" -> "MyGadget";
/// the webview titleizes it for display).
#[tauri::command]
pub fn list_custom_gadgets(app: tauri::AppHandle) -> Result<Vec<CustomGadgetFile>, String> {
    let dir = gadgets_dir(&app)?;
    let mut files = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .map_err(|error| format!("Could not read {}: {error}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_gadget_source(&path) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        files.push(CustomGadgetFile {
            id: stem.to_string(),
            name: stem.to_string(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    files.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(files)
}

/// Reads one custom gadget's source. Only gadget files directly inside the
/// app data dir can be read — the webview only ever receives paths from
/// `list_custom_gadgets`, but the check keeps the command from becoming a
/// generic file reader.
#[tauri::command]
pub fn read_custom_gadget(app: tauri::AppHandle, path: String) -> Result<Option<String>, String> {
    let dir = gadgets_dir(&app)?;
    let candidate = std::path::PathBuf::from(&path);
    let allowed = candidate
        .parent()
        .map(|parent| parent == dir)
        .unwrap_or(false)
        && is_gadget_source(&candidate);
    if !allowed {
        return Err("Only gadget sources inside the app data dir can be read".to_string());
    }
    Ok(std::fs::read_to_string(&candidate).ok())
}

/// Opens the gadget folder (the app data dir) in Explorer.
#[tauri::command]
pub fn open_gadgets_folder(app: tauri::AppHandle) -> Result<(), String> {
    let dir = gadgets_dir(&app)?;
    std::process::Command::new("explorer")
        .arg(dir.as_os_str())
        .spawn()
        .map_err(|error| format!("Could not open {}: {error}", dir.display()))?;
    Ok(())
}
