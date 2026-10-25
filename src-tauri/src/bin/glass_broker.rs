//! The glass broker binary: the shared capture/blur service every glass
//! app in the session attaches to (spawned on demand next to this
//! executable). A trampoline on purpose — the engine lives in the
//! `liquid-glass` crate, so the app and its broker always ship the exact
//! same protocol build.

fn main() {
    std::process::exit(liquid_glass::broker::run());
}
