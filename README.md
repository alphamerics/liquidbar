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

## Releasing

The GitHub Actions release workflow builds the Deploy installer and publishes
the signed installer, its signature, and `latest.json` to GitHub Releases.
The installed app checks the manifest configured in `src-tauri/tauri.conf.json`.
The manifest and installer must be publicly reachable by installed clients.

The Moonshift rebrand changes the Windows identity, installer registry key,
and settings directory. It does not migrate earlier installations or settings:
uninstall the previous version before installing this one, and back up any
settings or custom gadgets you want to move manually. The old installation's
uninstaller should be used while it is still installed. Publish the matching
changes in the sibling repositories before building a release.

### One-time setup

1. Check out `glassbar`, `glass-ui`, `liquid-glass`, and `deploy` as sibling
   directories. Install the dependencies needed by the signing script. From
   the parent directory, generate the signing materials:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\deploy\scripts\setup-signing.ps1 -Config .\glassbar\packaging\app-pack.json
   ```

   Keep the private keys and password backed up securely. The script reuses
   an existing updater key, so pass its original password when reusing one;
   generating a new password does not change the existing key's password.
   The Windows certificate created by the script is self-signed, so it does
   not provide the public trust of a commercial code-signing certificate.
2. Set `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` to the generated
   updater **public** key and commit it. The private key must match this key
   for installed versions to verify updates; keep the same key across releases.
3. Add these repository secrets under **Settings > Secrets and variables >
   Actions**:

   - `TAURI_SIGNING_PRIVATE_KEY`: contents of `packaging\keys\Glassbar-update.key`.
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: updater key password.
   - `WINDOWS_CERT_PFX`: base64-encoded contents of `packaging\keys\Glassbar.pfx`.
   - `WINDOWS_CERT_PASSWORD`: PFX password.

   For example, copy the PFX as base64 without printing it:

   ```powershell
   [Convert]::ToBase64String([IO.File]::ReadAllBytes((Resolve-Path .\glassbar\packaging\keys\Glassbar.pfx))) | Set-Clipboard
   ```

   The workflow also needs access to the three sibling GitHub repositories;
   its default checkout token cannot access other private repositories unless
   the checkout authentication is separately configured.

### Publish a version

From a clean Glassbar checkout on a branch tracking its remote:

```powershell
npm run release             # 0.1.0 -> 0.1.1
npm run release -- minor    # 0.1.0 -> 0.2.0
npm run release -- major    # 0.1.0 -> 1.0.0
```

The command checks that the tracked branch and remote have the same commit,
the updater public key is configured, and the five version files agree. It
increments the version, commits the version files, creates an annotated tag,
and atomically pushes both the branch and tag to the tracked remote. The tag
starts the **Release** workflow; watch that run in GitHub Actions. A successful
push does not mean the build or release succeeded.

If a command fails after changing files or creating the commit or tag, inspect
`git status`, the local commit, and tag before retrying. If the atomic push
failed, after resolving the issue you can push the existing commit and tag
together rather than creating another release. Manual workflow dispatch must
select a matching version tag, not a branch. Test the first release by
installing it, publishing a newer version, and using **Settings > Updates**.
