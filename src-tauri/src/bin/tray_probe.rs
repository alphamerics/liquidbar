//! Read-only diagnostic: reports the notification-icon registrations the
//! shell holds for every hidden background app — no input is synthesized.
fn main() {
    glassbar_lib::tray_probe_main();
}
