//! The Tauri command surface: one module per feature area, each holding only
//! the `#[tauri::command]` handlers (plus their small wire DTOs). Everything
//! is re-exported here so `generate_handler!` can list flat `commands::` names.

pub mod dnd;
pub mod gadgets;
pub mod glass;
pub mod hidden;
pub mod media;
pub mod pins;
pub mod power;
pub mod radios;
pub mod settings;
pub mod shell;
pub mod taskbar;
pub mod tray;

pub use dnd::*;
pub use gadgets::*;
pub use glass::*;
pub use hidden::*;
pub use media::*;
pub use pins::*;
pub use power::*;
pub use radios::*;
pub use settings::*;
pub use shell::*;
pub use taskbar::*;
pub use tray::*;
