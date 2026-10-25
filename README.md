# Glassbar

A GPU-rendered Windows 11 taskbar replacement built with Tauri 2, React, and
Rust. Explorer remains active; Glassbar owns only the visible taskbar surface
and the shell interaction layer.

## Features

- **Taskbar pill** — a compact, centered, transparent AppBar with live
  Windows Graphics Capture underneath a liquid-glass shader: refraction,
  dual-Kawase blur, chromatic dispersion, tint and reveal animations.
- **App buttons and menus** — live window enumeration with extracted
  executable icons, per-app menus, hover previews, pinned apps with launch,
  focus, minimize and restore.
- **Auto-hide and reveal** — the pill slides away (eased off-screen) when a
  maximized or fullscreen app covers it, and returns when the cursor enters
  a bottom reveal strip; the reveal band's height and hysteresis are
  user-configurable, and it also honors a z-order "desktop band" so a parked
  pill is treated like part of the desktop rather than always-on-top.
- **Full-platform click-through** — the overlay is hit-tested every 8 ms:
  only the pill's buttons and open menus capture the mouse; every other
  pixel passes clicks to the windows behind.
- **Control center** — quick settings cards: Wi-Fi, Bluetooth, battery,
  sound (including per-app mixer), Do Not Disturb, night light, airplane
  mode.
- **System tray** — enumerates Explorer's tray service, forwards clicks and
  context menus to system tray icons, and offers a hidden-window reveal.
- **Media** — transport controls, timeline and hover cards for the current
  media session, plus a scroll-wheel volume feature.
- **Gadgets** — clock/calendar, weather, keyboard layout, media and other
  live DOM gadgets inside the pill.
- **Glass settings** — live, session-wide glass material tuning: the values
  are shared with every glass app on the system through the liquid-glass
  broker's material file.

## Design

- **UI** (`src/features`): React taskbar controls, menus, settings, control
  center and gadgets. It only receives typed shell data and never implements
  desktop rendering.
- **Shell** (`src-tauri/src/shell`): Windows API integration through
  `windows-rs` — window enumeration, foreground activation, monitor queries,
  the AppBar registration boundary, tray, media, radios and the shell state
  modules.
- **Renderer** (`src-tauri/src/renderer`): the taskbar's glass state — the
  pill surface, hover previews, menu-glass delegation, resize springs, and
  the overlay supervision plumbing (slide, z-order, click-through rules).
- **Supervision loop** (`src-tauri/src/hit_test_loop`): the 8 ms overlay
  loop that wires click-through hit-testing, the auto-hide slide, the
  reveal strip and the z-order band together.
- **Commands** (`src-tauri/src/commands`): the Tauri command surface, one
  module per feature area.

## Glass dependencies

Glassbar is a consumer of two reusable library repos:

- [`liquid-glass`](https://github.com/alphamerics/liquid-glass) — the engine:
  D3D11 surfaces, Windows Graphics Capture, the shader, and the session
  broker (`glass-broker.exe`, compiled from this repo's trampoline bin so
  the app and its broker always share the same protocol build).
- [`glass-ui`](https://github.com/alphamerics/glass-ui) — the reusable glass
  UI kit: the React components and styles behind the pill, plus the native
  `MenuGlassManager` for menus and previews.

The Rust deps track those repos' `main` branches; the committed
`[patch]` sections in `src-tauri/Cargo.toml` redirect them to sibling
checkouts (`../liquid-glass`, `../glass-ui/native`) so local edits apply
instantly — clone all three repos side by side to contribute to everything
at once.

## Development

```powershell
npm install
npm run tauri dev
```

For a frontend-only visual preview (no native glass or shell):

```powershell
npm run dev
```

Note for Rust work: cargo fetches git dependencies itself and cannot use
your Git credential helper by default; enable the CLI fallback once per
machine if the two library repos are private:

```toml
# ~/.cargo/config.toml
[net]
git-fetch-with-cli = true
```

## Packaging

```powershell
npm run tauri build
```

Tauri bundles the app and automatically ships every `[[bin]]` target —
including the `glass-broker` trampoline — beside the executable, which is
where the liquid-glass library's clients spawn the broker.
