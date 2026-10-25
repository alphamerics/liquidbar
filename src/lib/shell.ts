import { invoke } from "@tauri-apps/api/core";

export interface TaskbarApp {
  id: number;
  title: string;
  processName: string;
  icon: string | null;
  isActive: boolean;
  isMinimized: boolean;
}

export interface PinnedApp {
  processName: string;
  title: string;
  icon: string | null;
  executable: string | null;
}

export interface ShellSnapshot {
  apps: TaskbarApp[];
  monitorCount: number;
  pinned: PinnedApp[];
}

const previewSnapshot: ShellSnapshot = {
  monitorCount: 1,
  pinned: [],
  apps: [
    { id: 1, title: "Visual Studio Code", processName: "Code", icon: null, isActive: true, isMinimized: false },
    { id: 2, title: "Microsoft Edge", processName: "Edge", icon: null, isActive: false, isMinimized: false },
    { id: 3, title: "Spotify Premium", processName: "Spotify", icon: null, isActive: false, isMinimized: false },
    { id: 4, title: "File Explorer", processName: "Explorer", icon: null, isActive: false, isMinimized: false },
  ],
};

export async function getShellSnapshot(): Promise<ShellSnapshot> {
  try {
    return await invoke<ShellSnapshot>("shell_snapshot");
  } catch {
    return previewSnapshot;
  }
}

export async function activateWindow(
  id: number,
  minimizeToggle?: boolean,
): Promise<void> {
  try {
    // minimizeToggle: the taskbar button's click toggle — a visible window
    // minimizes, a minimized one restores and takes focus. The preview
    // panel omits it (clicking a thumbnail always activates the window).
    await invoke("activate_window", { id, minimizeToggle: minimizeToggle ?? false });
  } catch {
    // The browser preview does not expose the native shell command.
  }
}

// The liquid glass bridge lives in the shared glass-ui kit; re-exported
// here so the taskbar's glass calls read as one import surface.
export {
  setUiHitRects,
  positionMenuGlass,
  logUi,
  prewarmMenuGlass,
  syncMenuGlasses,
} from "glass-ui";
export type { MenuGlassRect } from "glass-ui";

/** One placed control-center widget: template id + top-left grid cell. */
export interface CcLayoutItemSetting {
  id: string;
  x: number;
  y: number;
}

/** The control center's persisted visual toggles. */
export interface CcToggles {
  airplane: boolean;
  nightLight: boolean;
}

export type TemperatureUnit = "fahrenheit" | "celsius";

export interface WeatherGadgetSettings {
  unit: TemperatureUnit;
}

/** Per-gadget micro settings, keyed by gadget id; each gadget owns the
 * shape of its entry. */
export type GadgetSettings = {
  weather?: WeatherGadgetSettings;
} & { [gadget: string]: Record<string, unknown> | undefined };

export interface GlassbarSettings {
  reserveSpace: boolean;
  refractionStrength: number;
  blurRadius: number;
  blurSamples: number;
  tintStrength: number;
  tintColor: [number, number, number];
  edgeBand: number;
  chromaticOffset: number;
  chromaticBlend: number;
  outlineStrength: number;
  outlineThickness: number;
  outlineAngle: number;
  outlineMirror: number;
  outlineFade: number;
  /** Uniform black border around every glass surface. */
  borderThickness: number;
  borderOpacity: number;
  turbulenceStrength: number;
  turbulenceScale: number;
  wobble: number;
  pillRadius: number;
  /** Taskbar resize squash & stretch: depth (0..1). */
  resizeSquash: number;
  /** Taskbar resize spring bounciness (0..1). */
  resizeBounce: number;
  /** Taskbar resize spring: one oscillation period, milliseconds. */
  resizeDuration: number;
  /** The invisible reveal strip's regular height, physical px. */
  hoverStripHeight: number;
  /** The reveal strip's taller height while hovered, physical px. */
  hoverStripHoverHeight: number;
  /** Taskbar app icon size, physical px (applied as a CSS variable). */
  iconSize: number;
  /** Gap between taskbar app icons, physical px. */
  iconGap: number;
  /** Ids of the enabled taskbar gadgets (left zone), in display order. */
  enabledGadgets: string[];
  /** Per-gadget micro settings, keyed by gadget id. */
  gadgetSettings: GadgetSettings;
  /** Whether pinned apps render to the left of running apps in the bar. */
  pinnedSideLeft: boolean;
  /** Whether Glassbar hides Windows' own taskbar entirely. */
  hideTaskbar: boolean;
  /** Whether Glassbar launches itself when the user logs in (the per-user
   * Run key); on by default. */
  launchOnStartup: boolean;
  /** Stricter auto-hide: the taskbar fully hides whenever it is not
   * hovered, even when no maximized window covers the strip. */
  hideWhenUnhovered: boolean;
  /** Whether the reveal gesture still works under fullscreen apps: on,
   * only while the mouse cursor is visible (games hide it during play);
   * off, the strip is fully ignored under fullscreen. */
  revealInFullscreen: boolean;
  /** The control center's widget layout (edited in the settings window). */
  ccLayout: CcLayoutItemSetting[];
  /** The control center layout's format version (bumped on every layout
   * write; suppresses the legacy-default migration). */
  ccLayoutVersion: number;
  /** The control center's persisted visual toggles. */
  ccToggles: CcToggles;
  /** Display order of the taskbar's pinned apps (process names). */
  taskbarPinOrder: string[];
  /** Display order of the taskbar's running (unpinned) apps. */
  taskbarAppOrder: string[];
}

export async function getSettings(): Promise<GlassbarSettings | null> {
  try {
    return await invoke<GlassbarSettings>("get_settings");
  } catch {
    return null;
  }
}

/** Persists a settings patch: only the given fields are merged into the
 * shared store, so concurrent writers (the settings window, the taskbar and
 * the control center) can never revert each other's fields. */
export async function setSettings(patch: Partial<GlassbarSettings>): Promise<void> {
  try {
    await invoke("set_settings", { patch });
  } catch {
    // The browser preview does not expose the settings store.
  }
}

/** A custom gadget source file found in the app data dir (next to settings.json). */
export interface CustomGadgetFile {
  id: string;
  name: string;
  path: string;
}

export async function listCustomGadgetFiles(): Promise<CustomGadgetFile[]> {
  try {
    return await invoke<CustomGadgetFile[]>("list_custom_gadgets");
  } catch {
    return [];
  }
}

export async function readCustomGadget(path: string): Promise<string | null> {
  try {
    return await invoke<string>("read_custom_gadget", { path });
  } catch {
    return null;
  }
}

/** Opens the app data dir (the custom gadgets folder) in Explorer. */
export async function openGadgetsFolder(): Promise<void> {
  try {
    await invoke("open_gadgets_folder");
  } catch {
    // The browser preview does not expose the native file system.
  }
}

/**
 * Restores every setting to its default, applying them live. Resolves with
 * the defaults (null when the native command is unavailable, e.g. the
 * browser preview).
 */
export async function resetSettings(): Promise<GlassbarSettings | null> {
  try {
    return await invoke<GlassbarSettings>("reset_settings");
  } catch {
    return null;
  }
}

export async function openSettingsWindow(): Promise<void> {
  try {
    await invoke("open_settings_window");
  } catch {
    // The browser preview does not expose the settings window.
  }
}

export async function setReserveSpace(enabled: boolean): Promise<void> {
  try {
    await invoke("set_reserve_space", { enabled });
  } catch {
    // The browser preview does not expose the native AppBar registration.
  }
}

/**
 * Reports the taskbar's measured content width (physical pixels). The pill
 * grows to fit the content and shrinks back when it needs less, capped below
 * the monitor width so it can never overflow the screen.
 */
export async function setTaskbarContentWidth(width: number): Promise<void> {
  try {
    await invoke("set_taskbar_content_width", { desiredWidth: width });
  } catch {
    // The browser preview does not expose the native AppBar registration.
  }
}

export interface MediaSnapshot {
  title: string | null;
  artist: string | null;
  app: string | null;
  isPlaying: boolean;
  /** Playback position when the snapshot was taken (seconds). */
  positionSecs: number;
  /** Track length in seconds (0 when the session reports none). */
  durationSecs: number;
}

/** The live playback timeline behind the media card's progress slider. */
export interface MediaTimeline {
  positionSecs: number;
  durationSecs: number;
  isPlaying: boolean;
}

export async function getMediaTimeline(): Promise<MediaTimeline | null> {
  try {
    return await invoke<MediaTimeline>("media_timeline");
  } catch {
    return null;
  }
}

export async function mediaSeek(positionSecs: number): Promise<void> {
  try {
    await invoke("media_seek", { positionSecs });
  } catch {
    // The browser preview does not expose the media transport.
  }
}

/** The focused app's active keyboard layout (the "input language"). */
export interface KeyboardLayout {
  /** Short language tag like "ENG" (the taskbar-style indicator). */
  short: string;
  /** Human-readable display name like "English (United States)". */
  name: string;
}

export async function getKeyboardLayout(): Promise<KeyboardLayout | null> {
  try {
    return await invoke<KeyboardLayout>("keyboard_layout");
  } catch {
    return null;
  }
}

export async function getMediaSnapshot(): Promise<MediaSnapshot | null> {
  try {
    return await invoke<MediaSnapshot>("media_snapshot");
  } catch {
    return null;
  }
}

export async function closeApp(id: number): Promise<void> {
  try {
    await invoke("close_app", { id });
  } catch {
    // The browser preview does not expose the native shell command.
  }
}

export interface PreviewRect {
  x: number;
  y: number;
  width: number;
  height: number;
  radius?: number;
}

/**
 * Shows (or hides, with `id = null`) the native live preview of one app
 * window. The rect is the preview image area in physical pixels relative to
 * the overlay's top-left corner.
 */
export async function setWindowPreview(
  id: number | null,
  rect: PreviewRect | null,
): Promise<void> {
  try {
    await invoke("set_window_preview", { id, rect });
  } catch {
    // The browser preview does not expose native window thumbnails.
  }
}

export async function toggleMediaPlay(): Promise<void> {
  try {
    await invoke("toggle_media_play");
  } catch {
    // The browser preview does not expose the media transport.
  }
}

export async function mediaNext(): Promise<void> {
  try {
    await invoke("media_next");
  } catch {
    // The browser preview does not expose the media transport.
  }
}

export async function mediaPrevious(): Promise<void> {
  try {
    await invoke("media_previous");
  } catch {
    // The browser preview does not expose the media transport.
  }
}

/** The displayed track's album art as a data URL (fetched lazily). */
export async function getMediaThumbnail(): Promise<string | null> {
  try {
    return await invoke<string | null>("media_thumbnail");
  } catch {
    return null;
  }
}

export interface AudioApp {
  pid: number;
  name: string;
  /** The app's real icon as a PNG data URL (from its executable). */
  icon: string | null;
  volume: number;
  muted: boolean;
}

export interface AudioSnapshot {
  master: number;
  masterMuted: boolean;
  apps: AudioApp[];
}

export async function getAudioSnapshot(): Promise<AudioSnapshot | null> {
  try {
    return await invoke<AudioSnapshot>("audio_snapshot");
  } catch {
    return null;
  }
}

export async function setAudioMaster(volume: number): Promise<void> {
  try {
    await invoke("set_audio_master", { volume });
  } catch {
    // The browser preview does not expose the audio mixer.
  }
}

export async function setAudioMuted(muted: boolean): Promise<void> {
  try {
    await invoke("set_audio_muted", { muted });
  } catch {
    // The browser preview does not expose the audio mixer.
  }
}

export async function setAudioApp(pid: number, volume: number): Promise<void> {
  try {
    await invoke("set_audio_app", { pid, volume });
  } catch {
    // The browser preview does not expose the audio mixer.
  }
}

/** The machine's battery state (the control center's battery widget). */
export interface BatterySnapshot {
  hasBattery: boolean;
  percent: number | null;
  charging: boolean;
  acConnected: boolean;
  /** Estimated remaining runtime in seconds; null when unknown. */
  lifeSeconds: number | null;
}

export async function getBatterySnapshot(): Promise<BatterySnapshot | null> {
  try {
    return await invoke<BatterySnapshot>("battery_snapshot");
  } catch {
    return null;
  }
}



export interface WifiNetwork {
  ssid: string;
  signal: number;
  connected: boolean;
  secure: boolean;
  saved: boolean;
}

export async function getWifiNetworks(): Promise<WifiNetwork[]> {
  try {
    return await invoke<WifiNetwork[]>("wifi_networks");
  } catch {
    return [];
  }
}

export async function getWifiEnabled(): Promise<boolean | null> {
  try {
    return await invoke<boolean>("wifi_enabled");
  } catch {
    return null;
  }
}

export async function setWifiEnabled(enabled: boolean): Promise<void> {
  try {
    await invoke("set_wifi_enabled", { enabled });
  } catch {
    // The browser preview does not expose the radio API.
  }
}

export async function getBluetoothEnabled(): Promise<boolean | null> {
  try {
    return await invoke<boolean>("bluetooth_enabled");
  } catch {
    return null;
  }
}

export async function setBluetoothEnabled(enabled: boolean): Promise<void> {
  try {
    await invoke("set_bluetooth_enabled", { enabled });
  } catch {
    // The browser preview does not expose the radio API.
  }
}

/**
 * Joins a Wi-Fi network. Saved and open networks ignore the password; new
 * secure networks save a profile with it first. Resolves once the join
 * settles (connected or failed) — the error carries the reason.
 */
export async function wifiConnect(ssid: string, password: string): Promise<void> {
  await invoke("wifi_connect", { ssid, password });
}

/** Disconnects the Wi-Fi adapter from its current network. Throws on failure. */
export async function wifiDisconnect(): Promise<void> {
  await invoke("wifi_disconnect");
}

export interface BluetoothDevice {
  /** WinRT AEP id — the stable handle for pairing/unpairing. */
  id: string;
  name: string;
  connected: boolean;
  paired: boolean;
}

export async function getBluetoothDevices(): Promise<BluetoothDevice[]> {
  try {
    return await invoke<BluetoothDevice[]>("bluetooth_devices");
  } catch {
    return [];
  }
}

/**
 * Pairs an unpaired Bluetooth device. Blocks until the pairing settles —
 * passkey/confirmation devices show Windows' own pairing panel. Throws on
 * failure.
 */
export async function pairBluetoothDevice(id: string): Promise<void> {
  await invoke("bluetooth_pair", { id });
}

/** Unpairs a paired Bluetooth device. Throws on failure. */
export async function unpairBluetoothDevice(id: string): Promise<void> {
  await invoke("bluetooth_unpair", { id });
}

/** Airplane mode: turns every radio (Wi-Fi, Bluetooth, broadband) off. */
export async function setAirplaneMode(enabled: boolean): Promise<void> {
  try {
    await invoke("set_airplane_mode", { enabled });
  } catch {
    // The browser preview does not expose the radio API.
  }
}

/** Night light: applies a warm gamma ramp to every monitor. Rejects with a
 * reason (HDR on, remote session, driver refusal) when the native side
 * can't apply it. */
export async function setNightLight(enabled: boolean): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) {
    // The browser preview does not expose the gamma ramp.
    return;
  }
  await invoke("set_night_light", { enabled });
}

/** Windows Do not disturb state, with the end of a pending sleep window. */
export interface DndState {
  enabled: boolean;
  /** When a "sleep" focus window is armed: its end in epoch ms. */
  expiresAtMs: number | null;
}

export async function getDndState(): Promise<DndState> {
  try {
    return await invoke<DndState>("dnd_state");
  } catch {
    return { enabled: false, expiresAtMs: null };
  }
}

/** Manual Do not disturb toggle; clears any pending sleep window. */
export async function setDnd(enabled: boolean): Promise<void> {
  try {
    await invoke("set_dnd", { enabled });
  } catch {
    // The browser preview does not expose the quiet-hours switch.
  }
}

/**
 * Arms Do not disturb until the next 8:00 AM (the "Sleep" focus mode) and
 * resolves with the window's end, epoch ms (null when unavailable).
 */
export async function setDndSleep(): Promise<number | null> {
  try {
    return await invoke<number>("set_dnd_sleep");
  } catch {
    return null;
  }
}

export async function pinApp(id: number): Promise<void> {
  try {
    await invoke("pin_app", { id });
  } catch {
    // The browser preview does not expose pinning.
  }
}

export async function unpinApp(processName: string): Promise<void> {
  try {
    await invoke("unpin_app", { processName });
  } catch {
    // The browser preview does not expose pinning.
  }
}

export async function launchPinned(executable: string): Promise<void> {
  try {
    await invoke("launch_pinned", { executable });
  } catch {
    // The browser preview does not expose launching.
  }
}

/** A completely hidden background process: a top-level window that exists
 * but isn't on screen (the tray-style hidden apps). */
export interface HiddenWindow {
  /** The window handle � the stable id for reveal. */
  id: number;
  /** The display name: the program itself (what the tray tooltip shows). */
  name: string;
  /** The hidden window's internal title (context under the name). */
  title: string;
  processName: string;
  /** The program's real icon as a PNG data URL. */
  icon: string | null;
}

/**
 * Enumerates the hidden background processes � top-level windows that
 * exist but aren't on screen, one entry per process with its real icon.
 */
export async function getHiddenWindows(): Promise<HiddenWindow[]> {
  try {
    return await invoke<HiddenWindow[]>("hidden_windows");
  } catch {
    return [];
  }
}

/** Reveals a hidden background window: restores, shows and focuses it. */
export async function revealHiddenWindow(id: number): Promise<void> {
  try {
    await invoke("reveal_hidden_window", { id });
  } catch {
    // The browser preview does not expose the native shell command.
  }
}

export async function exitGlassbar(): Promise<void> {
  try {
    await invoke("exit_glassbar");
  } catch {
    // The browser preview does not expose the native exit command.
  }
}
/**
 * A notification-area icon enumerated from Explorer's tray toolbars, with
 * its stored callback metadata (owner window, icon id, callback message,
 * image and tooltip). The custom tray UI is built from this data.
 */
export interface TrayIcon {
  /** Stable interaction key: "<owner-hwnd>:<uid>". */
  key: string;
  /** The window that owns the `Shell_NotifyIcon` registration. */
  ownerHwnd: number;
  /** The registration's icon id. */
  uid: number;
  /** The callback message id the shell posts to the owner window. */
  callbackMessage: number;
  /** The owning application's display name. */
  processName: string;
  /** The registered tooltip/title, when available. */
  tooltip: string | null;
  /** The icon image as a PNG data URL, when available. */
  icon: string | null;
}

/**
 * Enumerates every notification-area icon from Explorer's tray toolbars and
 * (re)stores its callback metadata for future interactions.
 */
export async function getTrayIcons(): Promise<TrayIcon[]> {
  try {
    return await invoke<TrayIcon[]>("tray_icons");
  } catch {
    return [];
  }
}

/** How a forwarded tray interaction was delivered (diagnostics). */
export interface TrayForwardInfo {
  /** The window that received the callback messages. */
  ownerHwnd: number;
  /** The registration's icon id. */
  uid: number;
  /** The callback message the events were posted with. */
  callbackMessage: number;
  /** The notification-icon version the events were built for, when known. */
  version: number | null;
  /** Whether the application visibly reacted (opened its menu/window). */
  reacted: boolean;
  /** The delivered events, e.g. "WM_CONTEXTMENU". */
  events: string;
}

/**
 * Left click on a custom tray icon: forwards the equivalent tray activation
 * message to the owning application, replicating Explorer's behavior.
 */
export async function trayIconLeftClick(key: string): Promise<void> {
  await invoke<TrayForwardInfo>("tray_icon_activate", { key });
}

/**
 * Right click on a custom tray icon: sends the tray context-menu event to
 * the application's registered callback window, so the application creates
 * and displays its own native menu � exactly as it would for a real
 * right-click in Explorer. No menu is reconstructed client-side.
 */
export async function trayIconContextMenu(key: string): Promise<void> {
  await invoke<TrayForwardInfo>("tray_icon_context_menu", { key });
}