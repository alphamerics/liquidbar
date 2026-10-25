import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ContextMenu } from "./features/taskbar/ContextMenu";
import { SettingsWindow } from "./features/settings/SettingsWindow";
import { Taskbar } from "./features/taskbar/Taskbar";
import {
  exitGlassbar,
  getMediaSnapshot,
  getShellSnapshot,
  type MediaSnapshot,
  type ShellSnapshot,
} from "./lib/shell";
import "./App.css";

const refreshIntervalMs = 2_000;

function isSettingsWindow(): boolean {
  if (window.location.hash.startsWith("#/settings")) {
    return true;
  }
  try {
    return getCurrentWindow().label === "settings";
  } catch {
    return false;
  }
}

function App() {
  if (isSettingsWindow()) {
    return <SettingsWindow />;
  }
  return <Glassbar />;
}

function Glassbar() {
  const [shell, setShell] = useState<ShellSnapshot | null>(null);
  const [media, setMedia] = useState<MediaSnapshot | null>(null);
  const [mediaLevels, setMediaLevels] = useState<number[]>([]);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);

  const refresh = useCallback(async () => {
    setShell(await getShellSnapshot());
    setMedia(await getMediaSnapshot());
  }, []);

  useEffect(() => {
    void refresh();
    const interval = window.setInterval(() => void refresh(), refreshIntervalMs);
    return () => window.clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    let unlisteners: Array<() => void> = [];
    void import("@tauri-apps/api/event").then(({ listen }) => {
      void listen<number[]>("media-levels", (event) => {
        if (Array.isArray(event.payload) && event.payload.length === 5) {
          setMediaLevels(event.payload);
        }
      }).then((unlisten) => unlisteners.push(unlisten));
      void listen<MediaSnapshot>("media-changed", (event) => {
        setMedia(event.payload);
      }).then((unlisten) => unlisteners.push(unlisten));
    });
    return () => unlisteners.forEach((unlisten) => unlisten());
  }, []);

  const handleSettingsOpenChange = useCallback((open: boolean) => {
    setIsSettingsOpen(open);
  }, []);

  const handleContextMenuOpenChange = useCallback((_open: boolean) => {}, []);

  const handleRefresh = useCallback(() => {
    window.location.reload();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    // The native auto-hide slide drives the pill's content scale: hidden =
    // slightly small, revealed = full size, transitioned by CSS with the same
    // duration/easing as the native slide.
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<boolean>("taskbar-slide", (event) => {
          const content = document.querySelector<HTMLElement>(".taskbar-content");
          if (event.payload === true) {
            // Reveal: jump to the small size without animating, then
            // transition up to full so the scale-in runs with the slide.
            if (content) {
              content.style.transition = "none";
              content.classList.add("pill-small");
              void content.offsetWidth;
              content.style.transition = "";
              content.classList.remove("pill-small");
            }
          } else {
            // Hide: the pill keeps full size and only slides away. Any open
            // dropdown closes with it — the control center deletes itself and
            // resets to its main view instead of riding the sliding pill.
            handleSettingsOpenChange(false);
            if (content) {
              content.classList.remove("pill-small");
            }
          }
        }),
      )
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, [handleSettingsOpenChange]);

  if (!shell) {
    return <div className="boot-surface" aria-label="Starting Glassbar" />;
  }

  return (
    <>
      <svg className="glass-filter-definitions" aria-hidden="true">
        <filter id="glass-surface-displacement">
          <feTurbulence
            type="fractalNoise"
            baseFrequency="0.012 0.18"
            numOctaves="2"
            seed="8"
            result="surfaceNoise"
          />
          <feDisplacementMap
            in="SourceGraphic"
            in2="surfaceNoise"
            scale="-3.5"
            xChannelSelector="R"
            yChannelSelector="G"
          />
        </filter>
      </svg>
      <ContextMenu
        onOpenChange={handleContextMenuOpenChange}
        onRefresh={handleRefresh}
        onExit={() => void exitGlassbar()}
      >
        <Taskbar
          snapshot={shell}
          media={media}
          mediaLevels={mediaLevels}
          isSettingsOpen={isSettingsOpen}
          onSettingsOpenChange={handleSettingsOpenChange}
          onPinnedChange={() => void refresh()}
        />
      </ContextMenu>
    </>
  );
}

export default App;