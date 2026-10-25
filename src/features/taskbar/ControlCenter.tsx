import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  exitGlassbar,
  getAudioSnapshot,
  getBatterySnapshot,
  getBluetoothDevices,
  getBluetoothEnabled,
  getDndState,
  getSettings,
  getTrayIcons,
  getWifiEnabled,
  getWifiNetworks,
  openSettingsWindow,
  pairBluetoothDevice,
  setAirplaneMode,
  setAudioApp,
  setAudioMaster,
  setAudioMuted,
  setBluetoothEnabled,
  setDnd,
  setDndSleep,
  setNightLight,
  setSettings,
  setWifiEnabled,
  trayIconContextMenu,
  trayIconLeftClick,
  unpairBluetoothDevice,
  wifiConnect,
  wifiDisconnect,
  type AudioSnapshot,
  type BatterySnapshot,
  type BluetoothDevice,
  type CcToggles,
  type DndState,
  type GlassbarSettings,
  type TrayIcon,
  type WifiNetwork,
} from "../../lib/shell";
import { BluetoothView } from "./cc/BluetoothView";
import { BatteryView } from "./cc/BatteryView";
import { FocusView } from "./cc/FocusView";
import { TrayIconsView } from "./cc/TrayIconsView";
import { CcMainGrid, type CcWidgetContext } from "./cc/CcWidget";
import { CalendarTile } from "./cc/CalendarTile";
import { sanitizeCcLayout } from "./cc/layout";
import { PasswordModal } from "./cc/PasswordModal";
import { SoundView } from "./cc/SoundView";
import { WifiView } from "./cc/WifiView";

interface ControlCenterProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  trigger: ReactNode;
}

type View = "main" | "wifi" | "bluetooth" | "tray" | "sound" | "focus" | "calendar" | "battery";

/**
 * The macOS-style control center behind the settings cog. The panel state and
 * data loading live here; the visuals are componentized under `cc/` â€” every
 * container (tiles, circle toggles, slider cards, lists) is its own native
 * liquid glass surface via GlassCard.
 */
export function ControlCenter({ open, onOpenChange, trigger }: ControlCenterProps) {
  const [view, setView] = useState<View>("main");
  // The widget layout and the visual toggles live in the shared settings
  // JSON (settings.json), edited from the settings window and pushed live.
  const [ccSettings, setCcSettings] = useState<GlassbarSettings | null>(null);
  const [audio, setAudio] = useState<AudioSnapshot | null>(null);
  // The battery state behind the battery widget (null until the first poll).
  const [battery, setBattery] = useState<BatterySnapshot | null>(null);
  const [networks, setNetworks] = useState<WifiNetwork[] | null>(null);
  const [devices, setDevices] = useState<BluetoothDevice[] | null>(null);
  // Real radio state; `null` until the first poll answers.
  const [wifiOn, setWifiOn] = useState<boolean | null>(null);
  const [btOn, setBtOn] = useState<boolean | null>(null);
  // The Focus pill reads the real Do not disturb state (a sleep window may
  // end on its own, so the pill polls instead of persisting locally).
  const [dndState, setDndState] = useState<DndState | null>(null);
  // SSID currently joining (its list row shows the circle loader), and the
  // password dialog for unsaved secure networks.
  const [connecting, setConnecting] = useState<string | null>(null);
  const [dialogSsid, setDialogSsid] = useState<string | null>(null);
  const [dialogBusy, setDialogBusy] = useState(false);
  const [dialogError, setDialogError] = useState<string | null>(null);
  // Failure of a saved/open join (no dialog to carry it).
  const [listError, setListError] = useState<string | null>(null);
  // The SSID being disconnected from (its row shows the loader).
  const [disconnecting, setDisconnecting] = useState<string | null>(null);
  // The Bluetooth device being paired/unpaired, and any action failure.
  const [btBusy, setBtBusy] = useState<string | null>(null);
  const [btError, setBtError] = useState<string | null>(null);
  // The notification-area icons enumerated from Explorer's tray toolbars
  // (the custom tray UI's data), and the icon whose forwarded interaction
  // is in flight.
  const [trayIcons, setTrayIcons] = useState<TrayIcon[] | null>(null);
  const [trayDispatching, setTrayDispatching] = useState<string | null>(null);
  // Marks a tray interaction as in flight for the panel's dismissal guard
  // (see `handleOpenChange`); cleared when the forward settles.
  const trayDispatchingRef = useRef(false);
  const setTrayDispatchingTracked = (key: string | null) => {
    trayDispatchingRef.current = key !== null;
    setTrayDispatching(key);
  };
  // Radio toggles are optimistic; the poll may briefly read a stale radio
  // state while the change is still applying, so fresh flips are shielded.
  const radioPending = useRef<{ wifi: number; bluetooth: number }>({
    wifi: 0,
    bluetooth: 0,
  });

  /**
   * The controlled open state, gated on tray dispatches. Forwarding a tray
   * interaction hands the Windows foreground to the target application (its
   * own native menu needs it), which deactivates this window. Radix reads
   * that as a dismissal — via the webview's focus/blur handling — and would
   * close the whole panel, list and all, on every right-click. Every
   * dismissal path (DismissableLayer dismiss, Escape, trigger toggle) funnels
   * through `onOpenChange(false)`, so refusing it while a dispatch is in
   * flight pins the panel open for the interaction's duration. The ref
   * mirrors the state for synchronous reads: dismissal events can fire in
   * the same tick the dispatch starts, before React commits.
   */
  const handleOpenChange = (next: boolean) => {
    if (!next && trayDispatchingRef.current) {
      return;
    }
    onOpenChange(next);
  };

  // Reset to the main panel (and clear transient state) whenever the dropdown
  // closes.
  useEffect(() => {
    if (!open) {
      setView("main");
      setDialogSsid(null);
      setListError(null);
      setDisconnecting(null);
      setBtBusy(null);
      setBtError(null);
      setTrayDispatchingTracked(null);
    }
  }, [open]);

  // The layout and toggles follow the shared settings: loaded once, then
  // applied on every settings-changed push (from either window).
  useEffect(() => {
    void getSettings().then((loaded) => {
      if (loaded) {
        setCcSettings(loaded);
      }
    });
    let unlisten: (() => void) | undefined;
    void listen<GlassbarSettings>("settings-changed", (event) => {
      if (event.payload) {
        setCcSettings(event.payload);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);


  /** Persists a visual-toggle change into the shared settings JSON. Only
   * the ccToggles field is sent, so this can never revert another window's
   * fields with this window's (possibly stale) snapshot. */
  const updateCcToggles = (patch: Partial<CcToggles>) => {
    setCcSettings((current) => {
      if (!current) {
        return current;
      }
      const ccToggles = { ...current.ccToggles, ...patch };
      void setSettings({ ccToggles });
      return { ...current, ccToggles };
    });
  };

  /** Optimistic real toggles: the UI flips instantly, the native side
   * follows — and puts the toggle back if the change is rejected (e.g.
   * night light under HDR or in a remote session). */
  const toggleReal = (
    key: "airplane" | "nightLight",
    apply: (enabled: boolean) => Promise<void>,
  ) => {
    const current = ccSettings?.ccToggles;
    const enabled = !(key === "airplane" ? current?.airplane : current?.nightLight);
    updateCcToggles({ [key]: enabled });
    apply(enabled).catch(() => {
      updateCcToggles({ [key]: !enabled });
    });
  };

  /** Wi-Fi/Bluetooth radio flips: optimistic, shielded from the poll until
   * the native radio state settles. */
  const toggleRadio = (
    key: "wifi" | "bluetooth",
    set: (value: boolean | null) => void,
    apply: (enabled: boolean) => Promise<void>,
  ) => {
    const current = key === "wifi" ? wifiOn : btOn;
    const enabled = !(current ?? false);
    set(enabled);
    radioPending.current[key] = Date.now() + 6000;
    void apply(enabled).finally(() => {
      // Pull the authoritative state once the change settles.
      window.setTimeout(() => {
        radioPending.current[key] = 0;
      }, 500);
    });
  };

  /**
   * The Focus pill's icon: a plain Do not disturb toggle. Any pending sleep
   * window is cleared — a manual take-over replaces the automatic one.
   */
  const toggleDnd = () => {
    const enabled = !(dndState?.enabled ?? false);
    setDndState({ enabled, expiresAtMs: null });
    void setDnd(enabled);
  };

  /** The Focus view's two modes: plain DND, or DND until the next 8:00 AM. */
  const chooseFocusMode = (mode: "dnd" | "sleep") => {
    if (mode === "sleep") {
      // Optimistic; the resolved window refines the pill's label.
      setDndState({ enabled: true, expiresAtMs: Date.now() });
      void setDndSleep().then((expires) => {
        if (expires !== null) {
          setDndState({ enabled: true, expiresAtMs: expires });
        }
      });
    } else {
      setDndState({ enabled: true, expiresAtMs: null });
      void setDnd(true);
    }
  };

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    const load = async () => {
      const [snapshot, wifi, bt, dnd, power] = await Promise.all([
        getAudioSnapshot(),
        getWifiEnabled(),
        getBluetoothEnabled(),
        getDndState(),
        getBatterySnapshot(),
      ]);
      if (cancelled) {
        return;
      }
      setAudio(snapshot);
      setDndState(dnd);
      if (power) {
        setBattery(power);
      }
      const now = Date.now();
      if (now >= radioPending.current.wifi) {
        setWifiOn(wifi);
      }
      if (now >= radioPending.current.bluetooth) {
        setBtOn(bt);
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 3000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [open]);

  // Wi-Fi networks, Bluetooth devices and tray icons refresh only while
  // their view is open.
  useEffect(() => {
    if (view !== "wifi" && view !== "bluetooth" && view !== "tray") {
      return;
    }
    let cancelled = false;
    const load = async () => {
      if (view === "wifi") {
        const list = await getWifiNetworks();
        if (!cancelled) {
          setNetworks(list);
        }
      } else if (view === "tray") {
        const list = await getTrayIcons();
        if (!cancelled) {
          // The native side caches between scans; identical payloads (the
          // common case) must not re-render the list.
          setTrayIcons((prev) => (JSON.stringify(prev) === JSON.stringify(list) ? prev : list));
        }
      } else {
        const list = await getBluetoothDevices();
        if (!cancelled) {
          setDevices(list);
        }
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 4000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [view]);

  /**
   * Joins a network: saved and open networks connect straight away, new
   * secure ones come through the password modal. The clicked row's icon
   * becomes a circle loader until the join settles.
   */
  const joinNetwork = async (ssid: string, password: string) => {
    setListError(null);
    setConnecting(ssid);
    try {
      await wifiConnect(ssid, password);
      setNetworks(await getWifiNetworks());
      // A modal join that succeeded closes the modal.
      if (dialogSsid === ssid) {
        setDialogSsid(null);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (password.length > 0) {
        // Surface the failure inside the password modal.
        setDialogError(message);
        setDialogBusy(false);
      } else {
        setListError(message);
      }
    } finally {
      setConnecting(null);
    }
  };

  const openNetwork = (network: WifiNetwork) => {
    // Clicks during a join or a disconnect are ignored â€” the loader owns
    // the row.
    if (connecting !== null || dialogBusy || disconnecting !== null) {
      return;
    }
    if (network.connected) {
      void disconnectNetwork(network);
      return;
    }
    if (network.saved || !network.secure) {
      void joinNetwork(network.ssid, "");
    } else {
      setDialogError(null);
      setDialogBusy(false);
      setDialogSsid(network.ssid);
    }
  };

  /** Leaves the connected network; the row shows the loader until it settles. */
  const disconnectNetwork = async (network: WifiNetwork) => {
    setListError(null);
    setDisconnecting(network.ssid);
    try {
      await wifiDisconnect();
      setNetworks(await getWifiNetworks());
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setListError(message);
    } finally {
      setDisconnecting(null);
    }
  };

  /**
   * Pairs or unpairs a Bluetooth device: unpaired rows pair on click
   * (Windows may show its own confirmation panel), paired rows unpair. The
   * clicked row's icon becomes a circle loader until it settles.
   */
  const handleBluetoothDevice = async (device: BluetoothDevice) => {
    if (btBusy !== null) {
      return;
    }
    setBtError(null);
    setBtBusy(device.id);
    try {
      if (device.paired) {
        await unpairBluetoothDevice(device.id);
      } else {
        await pairBluetoothDevice(device.id);
      }
      setDevices(await getBluetoothDevices());
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setBtError(message);
    } finally {
      setBtBusy(null);
    }
  };

  const joinFromModal = (password: string) => {
    if (dialogSsid === null) {
      return;
    }
    setDialogError(null);
    setDialogBusy(true);
    void joinNetwork(dialogSsid, password);
  };

  const handleMaster = (value: number) => void setAudioMaster(value / 100);
  const handleMute = () => {
    const muted = !(audio?.masterMuted ?? false);
    setAudio((current) => (current ? { ...current, masterMuted: muted } : current));
    void setAudioMuted(muted);
  };

  // The panel follows settings.json live; sanitize guards against a
  // hand-edited or partially written store.
  const layout = sanitizeCcLayout(ccSettings?.ccLayout);
  const ccToggles = ccSettings?.ccToggles;

  /** The live state/actions bundle the layout-rendered widgets consume. */
  const widgetContext: CcWidgetContext = {
    wifiOn,
    btOn,
    dndLabel:
      dndState === null
        ? "…"
        : dndState.enabled
          ? dndState.expiresAtMs !== null
            ? "Until 8:00 AM"
            : "On"
          : "Off",
    dndEnabled: dndState?.enabled ?? false,
    airplane: ccToggles?.airplane ?? false,
    nightLight: ccToggles?.nightLight ?? false,
    audio,
    onOpenView: (next) => setView(next),
    onToggleRadio: (key) =>
      key === "wifi"
        ? toggleRadio("wifi", setWifiOn, setWifiEnabled)
        : toggleRadio("bluetooth", setBtOn, setBluetoothEnabled),
    onToggleDnd: toggleDnd,
    onToggleReal: (key) =>
      key === "airplane"
        ? toggleReal("airplane", setAirplaneMode)
        : toggleReal("nightLight", setNightLight),
    onOpenSettings: () => {
      onOpenChange(false);
      void openSettingsWindow();
    },
    battery,
    onExit: () => {
      // The exit widget: closing the panel first keeps the dismiss from
      // racing the teardown; the native command restores the Windows
      // taskbar before the process terminates.
      onOpenChange(false);
      void exitGlassbar();
    },
    onMaster: handleMaster,
    onMute: handleMute,
  };

  return (
    <DropdownMenu.Root open={open} onOpenChange={handleOpenChange}>
      <DropdownMenu.Trigger asChild>{trigger}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          className="cc-content"
          side="top"
          align="end"
          sideOffset={13}
          collisionPadding={14}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          <div className="cc-view" key={view}>
            {view === "main" && <CcMainGrid layout={layout} ctx={widgetContext} />}

            {view === "wifi" && (
              <WifiView
                wifiOn={wifiOn}
                networks={networks}
                connecting={connecting}
                disconnecting={disconnecting}
                error={listError}
                onBack={() => setView("main")}
                onOpenNetwork={openNetwork}
              />
            )}

            {view === "bluetooth" && (
              <BluetoothView
                devices={devices}
                busy={btBusy}
                error={btError}
                onBack={() => setView("main")}
                onDevice={(device) => void handleBluetoothDevice(device)}
              />
            )}

            {view === "tray" && (
              <TrayIconsView
                icons={trayIcons}
                dispatching={trayDispatching}
                onBack={() => setView("main")}
                onLeftClick={(key) => {
                  // The activation event is forwarded to the app's callback
                  // window (the custom taskbar proxies Explorer's tray
                  // event); the row shows a loader until it settles. The
                  // tracked setter also arms the panel-dismissal guard
                  // synchronously: the forwarded foreground hand-over must
                  // not close the list.
                  setTrayDispatchingTracked(key);
                  void trayIconLeftClick(key)
                    .catch((error) => console.warn("Tray activation forward failed", error))
                    .finally(() => setTrayDispatchingTracked(null));
                }}
                onContextMenu={(key) => {
                  // The native menu comes from the app itself: the
                  // context-menu event is replayed to the app's registered
                  // callback window exactly as Explorer would deliver it.
                  setTrayDispatchingTracked(key);
                  void trayIconContextMenu(key)
                    .catch((error) => console.warn("Tray context-menu forward failed", error))
                    .finally(() => setTrayDispatchingTracked(null));
                }}
              />
            )}

            {view === "sound" && (
              <SoundView
                audio={audio}
                onBack={() => setView("main")}
                onMaster={handleMaster}
                onMute={handleMute}
                onAppVolume={(pid, value) => void setAudioApp(pid, value)}
              />
            )}

            {view === "focus" && (
              <FocusView
                state={dndState}
                onBack={() => setView("main")}
                onMode={chooseFocusMode}
              />
            )}

            {view === "calendar" && <CalendarTile onBack={() => setView("main")} />}

            {view === "battery" && (
              <BatteryView battery={battery} onBack={() => setView("main")} />
            )}
          </div>

          <PasswordModal
            ssid={dialogSsid}
            busy={dialogBusy}
            error={dialogError}
            onJoin={joinFromModal}
            onClose={() => setDialogSsid(null)}
          />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}