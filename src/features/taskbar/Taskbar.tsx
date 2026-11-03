import { Cog } from "lucide-react";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { flushSync } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import {
  getSettings,
  pinApp,
  prewarmMenuGlass,
  setSettings,
  setTaskbarContentWidth,
  setTaskbarHeight,
  setUiHitRects,
  unpinApp,
  type GlassbarSettings,
  type MediaSnapshot,
  type MenuGlassRect,
  type ShellSnapshot,
  type TaskbarApp,
} from "../../lib/shell";
import { useGadgets } from "../gadgets/registry";
import { onDomMutations } from "glass-ui";
import { AppTaskbarButton } from "./AppTaskbarButton";
import { ControlCenter } from "./ControlCenter";
import { IconButton } from "./IconButton";
import { MediaGadget } from "./MediaGadget";


/**
 * Measures the taskbar content's intrinsic width and reports it to the
 * native shell, so the pill grows to fit its content (more app buttons,
 * a longer media title) and shrinks back when it needs less. The native
 * layer clamps the value below the monitor width so it can never overflow.
 *
 * Each section's intrinsic width is the sum of its children plus the flex
 * gaps — NOT scrollWidth, which for the stretchy grid columns equals the
 * current (possibly much wider) pill and would never let it shrink back.
 * Media/weather text is ellipsized with a bounded max-width, so long titles
 * cap their contribution.
 */
function useTaskbarWidthReporting(): void {
  // Only a width measured identically TWICE in a row (~500ms apart with the
  // tracking interval) is applied: single-measurement spikes (transient
  // content during boot, reflows, snap corrections) would otherwise churn
  // appbar repositions and resize springs — visible as size twitching.
  const lastSent = useRef(-1);
  const candidate = useRef(-1);
  const send = useCallback((desired: number) => {
    const dragActive = resizeDragActive.active;
    const flushed = performance.now() < resizeFlushUntil.until;
    // A pill-height change rescales the content zoom, which changes the
    // measured width instantly while the old two-measurement gate would
    // hold the new width back for ~500ms — the web UI would then be wider
    // than the window (right-anchored items displaced) until the gate let
    // the width through. During (and shortly after) a height change, apply
    // immediately at full precision — and as a non-animated report, since
    // springing on the rescale is the "pill keeps moving after the drag
    // ended" artifact.
    if (dragActive || flushed) {
      // Sub-3px differences are measurement noise around the settled value
      // (fractional zoom layouts, rounding): applying them churns the pill
      // with 1px repositions — the "keeps twitching after release" effect.
      if (Math.abs(desired - lastSent.current) < 3) {
        return;
      }
      candidate.current = -1;
      lastSent.current = desired;
      resizeWidthPump.lastWidth = desired;
      if (resizeMeasureOnly.active) {
        // The height drag owns the pill's geometry right now: the demand
        // was recorded for the drag's combined command, which applies it
        // in the same reposition as the height.
        return;
      }
      // The flush budget caps the full-precision corrections (see
      // `resizeFlushBudget`); once spent, the quantized path below takes
      // over and settles the pipeline for good.
      if (dragActive || resizeFlushBudget.left > 0) {
        if (!dragActive) {
          resizeFlushBudget.left -= 1;
        }
        void setTaskbarContentWidth(desired, false);
        return;
      }
    }
    const quantized = Math.ceil(desired / 10) * 10;
    if (quantized === lastSent.current) {
      return;
    }
    if (quantized !== candidate.current) {
      candidate.current = quantized;
      return;
    }
    candidate.current = -1;
    lastSent.current = quantized;
    resizeWidthPump.lastWidth = quantized;
    void setTaskbarContentWidth(quantized);
  }, []);
  const push = useCallback(() => {
    const content = document.querySelector<HTMLElement>(".taskbar-content");
    if (!content) {
      return;
    }
    const sections = ["taskbar-left", "taskbar-center", "taskbar-right"];
    const widths = [0, 0, 0];
    for (const [index, className] of sections.entries()) {
      const section = content.querySelector<HTMLElement>(`.${className}`);
      if (!section) {
        return;
      }
      const gap = Number.parseFloat(
        window.getComputedStyle(section).columnGap || "0",
      );
      const children = Array.from(section.children) as HTMLElement[];
      let intrinsic = 0;
      for (const [childIndex, child] of children.entries()) {
        // Only in-flow children count: the hover preview and the right-click
        // dropdown also mount as section children (absolutely positioned
        // popups) — counting them would resize the taskbar whenever a
        // preview opens.
        const position = window.getComputedStyle(child).position;
        if (position === "absolute" || position === "fixed") {
          continue;
        }
        // offsetWidth is the LAYOUT width and ignores the resize spring's
        // CSS transform — measuring getBoundingClientRect here would feed
        // the spring's own scale back into the pill width (feedback loop).
        // MARGINS are deliberately excluded: Chromium's computed style
        // returns auto margins as their RESOLVED pixel value, and the
        // gadget shells' margin-right: auto absorbs the section's free
        // space — measuring it would feed the pill's own width back into
        // the demand and ratchet the pill wider forever. Fixed child
        // margins (the 12px divider) are covered by the breathing
        // allowance below.
        intrinsic += child.offsetWidth;
        if (childIndex > 0) {
          intrinsic += gap;
        }
      }
      // Gadget text (media titles, weather) is truncated with ellipsis when
      // space is tight; its hidden width must count toward this section's
      // demand or the pill never grows to fit the full text. The demand is
      // capped at the text's CSS max-width: the gadget can never display more
      // than that, so counting the unbounded scrollWidth would make the pill
      // grow for width the gadget throws away.
      for (const text of section.querySelectorAll<HTMLElement>(
        ".taskbar-gadget-text, .media-gadget-text",
      )) {
        const maxWidth = Number.parseFloat(
          window.getComputedStyle(text).maxWidth,
        );
        const desired =
          Number.isFinite(maxWidth) && maxWidth > 0
            ? Math.min(text.scrollWidth, maxWidth)
            : text.scrollWidth;
        intrinsic += Math.max(0, desired - text.clientWidth);
      }
      widths[index] = intrinsic;
    }
    // The side columns are EQUAL (1fr auto 1fr — the apps must stay exactly
    // centered no matter what the gadgets hold), so each side gets
    // (width - center) / 2. The pill therefore has to fit TWICE the larger
    // side, not the sum of both — that is what lets an asymmetric audio
    // gadget expand while the center stays perfectly centered.
    const required = Math.max(widths[0], widths[2]) * 2 + widths[1];
    // The measured section widths are PRE-ZOOM local units (CSS zoom does
    // not appear in offsetWidth) — scaling them by the UI zoom maps the
    // demand to what the content actually occupies on screen. The content
    // grid's two 12px column gaps plus the shell's 16px side paddings plus
    // a small breathing allowance scale along with it (they live inside the
    // zoomed content). Quantization (to 10px steps, keeping appbar churn
    // low) happens in `send` — the immediate height-change path must stay
    // at full precision or the pill trails the rescaled content.
    const scale = window.devicePixelRatio || 1;
    const desired = Math.ceil((required + 24 + 32 + 8) * uiScale() * scale);
    send(desired);
  }, [send]);
  useEffect(() => {
    // The shared DOM lane (one body-subtree observer for the whole app,
    // with its own double-rAF settle) plus the content ResizeObserver.
    const content = document.querySelector<HTMLElement>(".taskbar-content");
    const observer = new ResizeObserver(push);
    if (content) {
      observer.observe(content);
    }
    const unsubscribe = onDomMutations(push);
    const timer = window.setInterval(push, 250);
    // Registered for the height drag's commit-time correction (see
    // beginResizeDrag): one exact measurement of the settled content.
    resizeWidthPump.fn = push;
    push();
    return () => {
      resizeWidthPump.fn = null;
      observer.disconnect();
      unsubscribe();
      window.clearInterval(timer);
    };
  }, [push]);
}

/**
 * While the pill's top-edge resize drag is in flight the whole overlay must
 * stay interactive: the pointer rides the moving top edge, and a momentary
 * miss (the window grows under a fast drag) must not drop the drag into
 * click-through. The hook below widens its pushed hit rects while this is
 * set; the drag owns the flag.
 */
const resizeDragActive = { active: false };

/**
 * Height changes rescale the content zoom, which changes the measured
 * content width in the same frame — the width pipeline's anti-twitch gate
 * (two identical measurements, 10px quantization) must step aside so the
 * native pill follows the DOM proportionally instead of ~500ms behind.
 * Every applied height change opens this immediate-apply window for a
 * moment (the drag keeps it open for its whole life).
 */
const resizeFlushUntil = { until: 0 };

/**
 * How many full-precision width corrections the flush window may still
 * apply. A width-dependent measurement (the demand shifts slightly with
 * the landed window width) plus an evergreen immediate mode would chase
 * its own tail forever — a ~6px width limit cycle observed live, once per
 * measurement lane. The budget caps the immediate corrections after a
 * height change; then the 10px-quantized path takes over, which is
 * self-terminating (the quantized demand equals the sent value and the
 * pipeline goes quiet).
 */
const resizeFlushBudget = { left: 0 };

/**
 * The width-reporting hook registers its measure-and-send closure here so
 * the height drag can pump it, plus the last width the pipeline produced
 * (physical px). During the drag the pump runs in MEASURE-ONLY mode (see
 * `resizeMeasureOnly`): it records the exact zoomed content demand here,
 * and the drag's combined height+width command applies it — the width and
 * height land in the SAME native reposition, in lockstep.
 */
const resizeWidthPump: { fn: (() => void) | null; lastWidth: number } = {
  fn: null,
  lastWidth: 0,
};

/**
 * Measure-only mode for the width pipeline: while the height drag owns the
 * pill's geometry, `push` still measures (so the drag's combined command
 * carries the EXACT content demand, not an estimate) but must not trigger
 * its own reposition — a separate width reposition lands a frame after the
 * height one, and the two would fight visibly.
 */
const resizeMeasureOnly = { active: false };

/**
 * Keeps the native hit-test loop informed of the taskbar's interactive
 * elements (overlay-relative physical rects). The overlay stays click-through
 * everywhere except these and the open menus, so clicks on empty pill areas
 * reach the windows behind instead of being swallowed by the oversized body.
 */
function useUiHitRects(): void {
  const push = useCallback(() => {
    const scale = window.devicePixelRatio || 1;
    const rects: MenuGlassRect[] = [];
    if (resizeDragActive.active) {
      // The resize drag holds the whole overlay interactive so the pointer
      // can never fall out of the webview mid-drag.
      const body = document.body.getBoundingClientRect();
      rects.push({
        x: body.left * scale,
        y: body.top * scale,
        width: body.width * scale,
        height: body.height * scale,
      });
    }
    document
      .querySelectorAll<HTMLElement>(
        ".taskbar-button, .cc-hit, .taskbar-resize-handle",
      )
      .forEach((node) => {
        const rect = node.getBoundingClientRect();
        if (rect.width < 1 || rect.height < 1) {
          return;
        }
        rects.push({
          x: rect.left * scale,
          y: rect.top * scale,
          width: rect.width * scale,
          height: rect.height * scale,
        });
      });
    void setUiHitRects(rects);
  }, []);
  useEffect(() => {
    // The shared DOM lane (one body-subtree observer for the whole app,
    // with its own double-rAF settle) plus a fallback interval.
    const unsubscribe = onDomMutations(push);
    const timer = window.setInterval(push, 120);
    push();
    return () => {
      unsubscribe();
      window.clearInterval(timer);
      void setUiHitRects([]);
    };
  }, [push]);
}

/**
 * Applies the icon size/gap settings as CSS custom properties on the root,
 * so the taskbar's icon and gap styling follows the settings live.
 */
function useIconMetrics(): void {
  useEffect(() => {
    const apply = (settings: GlassbarSettings) => {
      const root = document.documentElement.style;
      root.setProperty("--icon-size", `${Math.round(settings.iconSize)}px`);
      root.setProperty("--icon-gap", `${Math.round(settings.iconGap)}px`);
    };
    void getSettings().then((loaded) => {
      if (loaded) {
        apply(loaded);
      }
    });
    let unlisten: (() => void) | undefined;
    void listen<GlassbarSettings>("settings-changed", (event) => {
      if (event.payload) {
        apply(event.payload);
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
}

/** The pill height the DOM layout is authored against (CSS px): the web UI
 * scales proportionally with the pill around this base. */
const TASKBAR_BASE_HEIGHT_PX = 68;

/** Applies the pill height to the DOM: the shell height variable plus the
 * proportional UI scale the content zooms by. Every applied height change
 * also opens the width pipeline's immediate-apply window (with a small
 * correction budget — see `resizeFlushBudget`), so the rescaled content's
 * new width reaches the native pill promptly without evergreen immediate
 * mode. */
function applyTaskbarHeightVars(cssHeight: number): void {
  const root = document.documentElement.style;
  root.setProperty("--taskbar-height", `${cssHeight}px`);
  root.setProperty(
    "--taskbar-scale",
    `${cssHeight / TASKBAR_BASE_HEIGHT_PX}`,
  );
  resizeFlushUntil.until = performance.now() + 800;
  resizeFlushBudget.left = 2;
}

/** The content's current zoom scale (the `--taskbar-scale` variable). CSS
 * zoom does NOT appear in offsetWidth/offsetLeft/scrollWidth or computed
 * style values — they stay in pre-zoom local units — so every layout
 * measurement that must correspond to on-screen (viewport) pixels goes
 * through this. */
function uiScale(): number {
  return (
    Number.parseFloat(
      document.documentElement.style.getPropertyValue("--taskbar-scale"),
    ) || 1
  );
}

/**
 * The taskbar pill's height (macOS-dock style, resized by dragging the
 * pill's top edge).
 *
 * In the native app the overlay window is the single source of truth: it is
 * always `pill height + menu surface` tall, so the DOM derives its height
 * variables from the LIVE window size on every resize event. Writing the
 * variables ahead of the native resize (per pointer step) makes the DOM
 * lead the async native geometry and then fall behind it — the visible
 * jitter; deriving them from the landed window size keeps the DOM, the
 * glass and the pill in the same frame by construction.
 *
 * The browser preview has no native geometry, so there the saved setting
 * drives the variables directly.
 */
const MENU_SURFACE_HEIGHT_PX = 480;

function useTaskbarHeightVar(): { current: number } {
  const heightRef = useRef(68);
  // Layout effect: the first paint must already carry the landed window's
  // height, or a saved non-default height flashes at the 68px default.
  useLayoutEffect(() => {
    if ("__TAURI_INTERNALS__" in window) {
      // Only a HEIGHT change re-opens the width pipeline's immediate window
      // (and refreshes its correction budget): every width reposition also
      // fires a resize event, and re-arming on those kept immediate mode
      // alive forever — the width limit cycle.
      let lastSyncedHeight = -1;
      const sync = () => {
        const menuSurfaceCss = MENU_SURFACE_HEIGHT_PX / (window.devicePixelRatio || 1);
        const cssHeight = window.innerHeight - menuSurfaceCss;
        if (cssHeight < 24) {
          return;
        }
        heightRef.current = cssHeight;
        if (Math.abs(cssHeight - lastSyncedHeight) >= 1) {
          lastSyncedHeight = cssHeight;
          applyTaskbarHeightVars(cssHeight);
        }
      };
      sync();
      window.addEventListener("resize", sync);
      return () => window.removeEventListener("resize", sync);
    }
    const apply = (settings: GlassbarSettings | null | undefined) => {
      if (!settings || !Number.isFinite(settings.taskbarHeight)) {
        return;
      }
      const cssHeight = settings.taskbarHeight / (window.devicePixelRatio || 1);
      heightRef.current = cssHeight;
      applyTaskbarHeightVars(cssHeight);
    };
    void getSettings().then(apply);
    let unlisten: (() => void) | undefined;
    void listen<GlassbarSettings>("settings-changed", (event) => {
      apply(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);
  return heightRef;
}

/**
 * Tracks which taskbar gadgets are enabled, following the settings live:
 * loads the saved list once and then applies every settings-changed push.
 */
function useEnabledGadgets(): string[] {
  const [enabled, setEnabled] = useState<string[]>([]);
  useEffect(() => {
    void getSettings().then((loaded) => {
      if (loaded?.enabledGadgets) {
        setEnabled(loaded.enabledGadgets);
      }
    });
    let unlisten: (() => void) | undefined;
    void listen<GlassbarSettings>("settings-changed", (event) => {
      if (event.payload?.enabledGadgets) {
        setEnabled(event.payload.enabledGadgets);
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
  return enabled;
}

/**
 * Tracks whether pinned apps render to the left of running apps, following
 * the settings live: loads the saved flag once and then applies every
 * settings-changed push.
 */
function usePinnedSideLeft(): boolean {
  const [pinnedSideLeft, setPinnedSideLeft] = useState(false);
  useEffect(() => {
    void getSettings().then((loaded) => {
      if (loaded) {
        setPinnedSideLeft(loaded.pinnedSideLeft);
      }
    });
    let unlisten: (() => void) | undefined;
    void listen<GlassbarSettings>("settings-changed", (event) => {
      if (event.payload) {
        setPinnedSideLeft(event.payload.pinnedSideLeft);
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
  return pinnedSideLeft;
}

/** Stable sort by a process-name order list; names missing from the list
 * keep their relative order at the end. */
function sortByOrder<T>(items: T[], order: string[], name: (item: T) => string): T[] {
  const rank = new Map<string, number>();
  order.forEach((entry, index) => rank.set(entry, index));
  return items
    .map((item, index) => ({ item, key: rank.get(name(item)) ?? order.length + index }))
    .sort((a, b) => a.key - b.key)
    .map((entry) => entry.item);
}

/**
 * The taskbar's draggable app order, following the settings live. The two
 * order lists (pinned and running, by process name) are persisted into the
 * shared settings JSON; names that have left the list are filtered at sort
 * time, and reordering writes the currently displayed sequence back.
 */
function useTaskbarOrder() {
  const [pinOrder, setPinOrder] = useState<string[]>([]);
  const [appOrder, setAppOrder] = useState<string[]>([]);
  const settingsRef = useRef<GlassbarSettings | null>(null);
  useEffect(() => {
    const apply = (settings: GlassbarSettings | null) => {
      if (!settings) {
        return;
      }
      settingsRef.current = settings;
      setPinOrder(settings.taskbarPinOrder ?? []);
      setAppOrder(settings.taskbarAppOrder ?? []);
    };
    void getSettings().then(apply);
    let unlisten: (() => void) | undefined;
    void listen<GlassbarSettings>("settings-changed", (event) => {
      apply(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);
  const save = useCallback(
    (patch: Partial<Pick<GlassbarSettings, "taskbarPinOrder" | "taskbarAppOrder">>) => {
      const current = settingsRef.current;
      if (!current) {
        return;
      }
      // Merged locally too, so a drag's commit render shows the final order
      // immediately instead of flickering back until the push round-trips.
      settingsRef.current = { ...current, ...patch };
      if (patch.taskbarPinOrder) {
        setPinOrder(patch.taskbarPinOrder);
      }
      if (patch.taskbarAppOrder) {
        setAppOrder(patch.taskbarAppOrder);
      }
      // Only the order fields are sent, so a stale snapshot here can never
      // revert another window's fields (see set_settings' patch merge).
      void setSettings(patch);
    },
    [],
  );
  return { pinOrder, appOrder, save };
}

/** How far the pointer must cross a slot (or the pinned/running) boundary
 * before the preview swaps — hovering a boundary must not flip-flop. */
const SWAP_MARGIN = 14;

/** The top-edge resize drag's height bounds (CSS px), mirrored by the
 * native command's physical-pixel clamps. */
const TASKBAR_MIN_HEIGHT_PX = 48;
const TASKBAR_MAX_HEIGHT_PX = 160;

/** A taskbar app drag in flight: the dragged button stays in the layout flow
 * (at its preview slot, so its neighbors part around it) while its visual
 * follows the pointer via transform, grown by `scale`. */
interface AppDragState {
  pointerId: number;
  name: string;
  appId: number;
  side: "pinned" | "running";
  element: HTMLElement;
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
  offsetX: number;
  offsetY: number;
  scale: number;
  slotLeft: number;
  slotTop: number;
  pickupStart: number;
  pickupRaf: number;
  moved: boolean;
  target: { side: "pinned" | "running"; index: number } | null;
}

/** The live re-order preview while dragging. */
interface DragPreview {
  name: string;
  side: "pinned" | "running";
  index: number;
}

/** Springy drop easing: overshoot, one rebound, then settle. */
const SETTLE_EASING =
  "linear(0, 0.01 1%, 0.038 2%, 0.147 4.2%, 0.748 12.1%, 0.967 15.6%, " +
  "1.047 17.3%, 1.107 19%, 1.15 20.7%, 1.176 22.5%, 1.188 24.9%, " +
  "1.175 27.5%, 1.014 39.6%, 0.983 43.2%, 0.968 46.9%, 0.967 52.2%, " +
  "0.997 64.1%, 1.006 70.9%, 1)";
const SETTLE_DURATION_MS = 600;

interface TaskbarProps {
  snapshot: ShellSnapshot;
  media: MediaSnapshot | null;
  isSettingsOpen: boolean;
  onSettingsOpenChange: (open: boolean) => void;
  onPinnedChange: () => void;
}

/** Payload of the native `pill-resize-anim` event (see lib.rs). */
interface PillResizeAnim {
  fromWidth: number;
  toWidth: number;
  ratio: number;
  squash: number;
  bounce: number;
  durationMs: number;
}

/**
 * Mirrors the native glass resize spring (renderer/mod.rs) on the DOM
 * content: when the taskbar resizes to fit new content, the pill's content
 * springs with squash-and-stretch and bounce, anchored to the pill's
 * bottom-center — exactly like the native glass underneath, so both move as
 * one. A hard ceiling at 1.0 keeps every scale from exceeding the window,
 * so nothing is ever clipped: growing starts compressed and springs open,
 * shrinking starts flush with an inward kick; hitting 1.0 reflects the
 * velocity with restitution (the "squash into place" bounce).
 */
function usePillResizeAnimation(): void {
  const springRef = useRef(0);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<PillResizeAnim>("pill-resize-anim", (event) => {
      const { ratio, squash, bounce, durationMs } = event.payload;
      const target = document.querySelector<HTMLElement>(".taskbar-shell");
      if (!target) {
        return;
      }
      const omega = (2 * Math.PI) / (Math.max(durationMs, 60) / 1000);
      const zeta = Math.min(
        Math.max(1 - Math.min(Math.max(bounce, 0), 1) * 0.75, 0.15),
        1,
      );
      const restitution = Math.min(Math.max(bounce, 0), 1) * 0.6;
      const clampedRatio = Math.min(Math.max(ratio, 0.5), 2);
      // Velocity-proportional overshoot (mirrors renderer/mod.rs): the
      // initial kick scales with the size change, so bigger resizes rebound
      // harder off the 1.0 ceiling.
      const delta = Math.abs(clampedRatio - 1);
      let x = clampedRatio <= 1 ? clampedRatio : 1;
      let vx = delta * omega * (clampedRatio <= 1 ? 1 : -1);
      const amplitude = Math.min(Math.abs(clampedRatio - 1), 0.35);
      let y = 1 - amplitude * (0.5 + 0.5 * squash);
      let vy = 0;
      let last = performance.now();
      const step = (now: number) => {
        const dt = Math.min((now - last) / 1000, 0.05);
        last = now;
        vx += (omega * omega * (1 - x) - 2 * zeta * omega * vx) * dt;
        vy += (omega * omega * (1 - y) - 2 * zeta * omega * vy) * dt;
        x += vx * dt;
        y += vy * dt;
        if (x > 1) {
          x = 1;
          vx = -vx * restitution;
          if (Math.abs(vx) < 0.08) {
            vx = 0;
          }
        }
        if (y > 1) {
          y = 1;
          vy = -vy * restitution;
          if (Math.abs(vy) < 0.08) {
            vy = 0;
          }
        }
        const settled =
          Math.abs(x - 1) < 0.002 &&
          Math.abs(y - 1) < 0.002 &&
          Math.abs(vx) < 0.01 &&
          Math.abs(vy) < 0.01;
        if (settled) {
          target.style.transform = "";
          return;
        }
        target.style.transformOrigin = "50% 100%";
        target.style.transform = `scale(${x}, ${y})`;
        springRef.current = requestAnimationFrame(step);
      };
      cancelAnimationFrame(springRef.current);
      springRef.current = requestAnimationFrame(step);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
      cancelAnimationFrame(springRef.current);
    };
  }, []);
}

export function Taskbar({
  snapshot,
  media,
  isSettingsOpen,
  onSettingsOpenChange,
  onPinnedChange,
}: TaskbarProps) {
  useUiHitRects();
  useTaskbarWidthReporting();
  usePillResizeAnimation();
  useIconMetrics();
  const taskbarHeightRef = useTaskbarHeightVar();

  // ---- Top-edge resize drag (macOS-dock style) ----
  // Hovering the pill's top edge shows the ns-resize cursor; dragging up or
  // down scales the pill live. Three ingredients keep it smooth:
  // 1. The width is a PURE FUNCTION of the height: the content demand per
  //    unit scale is captured once at drag start and scaled by the zoom
  //    ratio — no feedback, no stale-baseline discontinuities.
  // 2. The DOM leads optimistically at the full frame rate (the vars apply
  //    the target instantly, so the content tracks the pointer) and the
  //    landed window size corrects it a frame later (see
  //    useTaskbarHeightVar) — the desync is bounded to one frame.
  // 3. Live steps take the native fast path (no shell-synchronizing AppBar
  //    round-trip) at 1px granularity, throttled only by the frame clock;
  //    the commit does the full reposition and persists.
  const beginResizeDrag = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const scale = window.devicePixelRatio || 1;
      // Screen coordinates, NOT client coordinates: the window's top edge
      // moves under the pointer as the pill grows, so clientY measures the
      // pointer's position relative to the MOVING window — the target
      // becomes "the pointer movement since the last native apply" and
      // oscillates every step (the violent resize jitter). screenY is
      // invariant to the window's own movement, so the target is the true
      // cumulative drag distance. (The app forces a 1x device scale, so
      // screenY is physical px, same as the heights here.)
      const startScreenY = event.screenY;
      const startHeight = taskbarHeightRef.current;
      // The width scaling needs a baseline: if the width pipeline hasn't
      // reported anything yet (fresh boot), measure once before dragging.
      if (resizeWidthPump.lastWidth <= 0) {
        resizeWidthPump.fn?.();
      }
      // The content demand per unit scale (physical px at the 68px base
      // height): zoom is linear in the layout, so the demand at any height
      // is this constant times the ratio — a pure function of the height.
      const widthBase =
        resizeWidthPump.lastWidth > 0
          ? resizeWidthPump.lastWidth / (startHeight / TASKBAR_BASE_HEIGHT_PX)
          : 0;
      let pending = Math.round(startHeight);
      let applied = pending;
      let raf = 0;
      // Backpressure: the native command runs on the main thread (window
      // resize + glass re-sync + events), so fire-and-forget invokes at the
      // frame rate can enqueue faster than they execute — the visible
      // resize then trails the pointer and keeps draining after release.
      // ONE command is ever in flight; newer targets coalesce into the
      // next send, and a commit queued mid-flight lands LAST, carrying the
      // final height.
      let inFlight = false;
      let commitQueued = false;
      let lastSent = pending;
      const widthFor = (height: number, fromHeight: number): number | null => {
        if (widthBase > 0) {
          return Math.round(widthBase * (height / TASKBAR_BASE_HEIGHT_PX));
        }
        // No baseline (never measured): chain from the last known width,
        // scaled by the height ratio since it was captured.
        return resizeWidthPump.lastWidth > 0 && fromHeight > 0
          ? Math.round(resizeWidthPump.lastWidth * (height / fromHeight))
          : null;
      };
      const pump = (commit: boolean) => {
        if (inFlight) {
          if (commit) {
            commitQueued = true;
          }
          return;
        }
        const isCommit = commit || commitQueued;
        commitQueued = false;
        const height = applied;
        if (!isCommit && height === lastSent) {
          return;
        }
        inFlight = true;
        // Height and width ride ONE command/one reposition, so the pill's
        // width always lands in lockstep with its height — the zoomed
        // content can never overhang the window mid-drag.
        void setTaskbarHeight(height * scale, isCommit, widthFor(height, lastSent))
          .catch(() => {})
          .finally(() => {
            inFlight = false;
            lastSent = height;
            // Converge: a target applied while this command was in flight
            // (or a queued commit) goes out with the next command.
            if (commitQueued || applied !== lastSent) {
              pump(commitQueued);
            }
          });
      };
      const apply = () => {
        raf = 0;
        // 1px steps at the display's frame rate; the DOM leads optimistically
        // (it tracks the pointer) and the native side converges with
        // backpressure — one command in flight, coalescing to the latest
        // target, never a queue.
        const height = Math.round(pending);
        if (height === applied) {
          return;
        }
        applied = height;
        // Optimistic DOM: the content rescales toward the target instantly;
        // the landed window size re-syncs the vars a frame later, so a
        // clamp/snap self-corrects.
        applyTaskbarHeightVars(height);
        pump(false);
      };
      resizeDragActive.active = true;
      resizeMeasureOnly.active = true;
      document.body.classList.add("taskbar-resizing");
      const move = (moveEvent: PointerEvent) => {
        pending = Math.min(
          Math.max(
            startHeight + (startScreenY - moveEvent.screenY),
            TASKBAR_MIN_HEIGHT_PX,
          ),
          TASKBAR_MAX_HEIGHT_PX,
        );
        if (!raf) {
          raf = requestAnimationFrame(apply);
        }
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        window.removeEventListener("pointercancel", up);
        if (raf) {
          cancelAnimationFrame(raf);
          raf = 0;
        }
        // Always finish with a commit: it persists the height (and applies
        // it natively when the last live step never landed), at the EXACT
        // pointer height. If a live command is still in flight, the commit
        // is queued and lands after it, carrying the final height.
        applied = Math.round(pending);
        applyTaskbarHeightVars(applied);
        pump(true);
        resizeMeasureOnly.active = false;
        resizeDragActive.active = false;
        document.body.classList.remove("taskbar-resizing");
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
      window.addEventListener("pointercancel", up);
    },
    [taskbarHeightRef],
  );

  const pinnedSideLeft = usePinnedSideLeft();
  const { pinOrder, appOrder, save } = useTaskbarOrder();
  const enabledGadgetIds = useEnabledGadgets();
  const gadgets = useGadgets();
  const enabledGadgets = gadgets.filter((gadget) =>
    enabledGadgetIds.includes(gadget.id),
  );

  const pinnedNames = new Set(snapshot.pinned.map((entry) => entry.processName));
  // Stack by process: one button per app, no matter how many windows it has
  // open. The stacked button tracks the app's active window when one is
  // focused (for the preview and the minimize toggle), otherwise the first
  // of its windows. Every instance is kept for the right-click dropdown's
  // window list.
  const stacked = new Map<string, TaskbarApp>();
  const instances = new Map<string, TaskbarApp[]>();
  for (const app of snapshot.apps) {
    // Pinned apps don't stack (they keep their own slot) but their windows
    // still belong in the instance list for the right-click dropdown.
    if (!pinnedNames.has(app.processName)) {
      const existing = stacked.get(app.processName);
      if (!existing || (!existing.isActive && app.isActive)) {
        stacked.set(app.processName, app);
      }
    }
    const list = instances.get(app.processName);
    if (list) {
      list.push(app);
    } else {
      instances.set(app.processName, [app]);
    }
  }
  const activeApps = [...stacked.values()];
  // Pinned apps stay on the taskbar even when closed: running ones use their
  // live window, closed ones render a placeholder that launches the stored
  // executable.
  const pinnedApps: {
    key: string;
    launchPath: string | null;
    app: TaskbarApp;
    instances: TaskbarApp[];
  }[] = snapshot.pinned.map((entry) => {
    const all = instances.get(entry.processName) ?? [];
    const running = snapshot.apps.find((app) => app.processName === entry.processName);
    return {
      key: entry.processName,
      launchPath: running ? null : entry.executable,
      app: running ?? {
        id: -1,
        title: entry.title,
        processName: entry.processName,
        icon: entry.icon,
        isActive: false,
        isMinimized: false,
      },
      instances: all.length > 0 ? all : running ? [running] : [],
    };
  });
  // Display order follows the drag-edited lists (missing names keep their
  // snapshot order).
  const sortedPinned = sortByOrder(pinnedApps, pinOrder, (entry) => entry.app.processName);
  const sortedActive = sortByOrder(activeApps, appOrder, (app) => app.processName);

  const handlePinToggle = useCallback(
    (app: { id: number; processName: string }, pinned: boolean) => {
      void (pinned ? unpinApp(app.processName) : pinApp(app.id)).then(onPinnedChange);
    },
    [onPinnedChange],
  );

  // ---- Taskbar app drag & drop ----
  const centerRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<AppDragState | null>(null);
  const suppressClickRef = useRef(false);
  const [dragPreview, setDragPreview] = useState<DragPreview | null>(null);
  // The pointermove/up listeners mount once on window; these refs forward
  // every event to the freshest closures (which see the current snapshot and
  // order lists).
  const dragMoveRef = useRef<(event: PointerEvent) => void>(() => {});
  const dragUpRef = useRef<(event: PointerEvent) => void>(() => {});

  const applyDragTransform = (drag: AppDragState) => {
    // Transform lengths live in the element's PRE-ZOOM local units, while
    // the offsets accumulate viewport (post-zoom) pointer deltas — divide
    // by the UI scale so the ghost tracks the pointer exactly.
    const ui = uiScale();
    drag.element.style.transform = `translate(${drag.offsetX / ui}px, ${drag.offsetY / ui}px) scale(${drag.scale})`;
  };

  /** The pick-up: the button grows from 1 to 1.28 with an ease-out curve. */
  const startPickupGrow = (drag: AppDragState) => {
    const duration = 180;
    const step = () => {
      if (dragRef.current !== drag) {
        return;
      }
      const t = Math.min((performance.now() - drag.pickupStart) / duration, 1);
      drag.scale = 1 + 0.28 * (1 - Math.pow(1 - t, 3));
      applyDragTransform(drag);
      if (t < 1) {
        drag.pickupRaf = requestAnimationFrame(step);
      }
    };
    drag.pickupRaf = requestAnimationFrame(step);
  };

  /**
   * The drop: the button snaps from its ghost position into its slot with a
   * springy overshoot-and-rebound easing (CSS `linear()` curve), applied to
   * both the translate and the scale in a single transform transition.
   */
  const startSettleSpring = (
    element: HTMLElement,
    offsetX: number,
    offsetY: number,
    startScale: number,
  ) => {
    element.classList.add("app-settling");
    element.style.transition = "none";
    // Transform lengths are pre-zoom local units (see applyDragTransform).
    const ui = uiScale();
    element.style.transform = `translate(${offsetX / ui}px, ${offsetY / ui}px) scale(${startScale})`;
    // Commit the starting frame so the transition actually runs.
    element.getBoundingClientRect();
    element.style.transition = `transform ${SETTLE_DURATION_MS}ms ${SETTLE_EASING}`;
    element.style.transform = "";
    window.setTimeout(() => {
      element.style.transition = "";
      element.classList.remove("app-settling");
    }, SETTLE_DURATION_MS);
  };

  /**
   * The drop point for a pointer position: the side (pinned/running) of the
   * button under the pointer — or, over a gap, of the divider (the section's
   * midpoint when nothing is pinned) — plus the insertion index, counted as
   * the buttons of that side whose center sits left of the pointer (the
   * dragged one is excluded, so the index is relative to the sequence
   * without it).
   *
   * Everything is measured in LAYOUT coordinates (offsetLeft/offsetWidth,
   * which ignore transforms): getBoundingClientRect would read the neighbors
   * mid-swipe animations and feed the motion back into itself, flip-flopping
   * the preview. On top of that, a slot (or side) change only applies once
   * the pointer crosses the boundary by SWAP_MARGIN — hysteresis, so
   * hovering a boundary rests instead of oscillating.
   */
  const findDropTarget = (clientX: number, drag: AppDragState) => {
    const center = centerRef.current;
    if (!center) {
      return null;
    }
    // The buttons' offsetLeft/offsetWidth are LAYOUT values in PRE-ZOOM
    // local units (CSS zoom does not appear in them), while clientX is a
    // viewport (post-zoom) pixel. Anchor on the center's visual border-box
    // left and scale the shell-relative layout offsets: the buttons share
    // the center's offsetParent (.taskbar-shell), so the offsetLeft
    // DIFFERENCE is a pure intra-content offset in zoom-free units, and
    // multiplying by the UI scale maps it to viewport pixels.
    const ui = uiScale();
    const centerLeft = center.getBoundingClientRect().left;
    const centerOffset = center.offsetLeft;
    const visualLeft = (el: HTMLElement) =>
      centerLeft + (el.offsetLeft - centerOffset) * ui;
    const layoutCenterX = (el: HTMLElement) =>
      visualLeft(el) + (el.offsetWidth * ui) / 2;
    const buttons = Array.from(center.querySelectorAll<HTMLElement>("[data-app-entry]")).filter(
      (el) => el !== drag.element,
    );
    const divider = center.querySelector<HTMLElement>(".taskbar-divider");
    const middle = divider
      ? layoutCenterX(divider)
      : centerLeft + (center.offsetWidth * ui) / 2;
    let side: "pinned" | "running" | null = null;
    for (const el of buttons) {
      const left = visualLeft(el);
      if (clientX >= left && clientX <= left + el.offsetWidth * ui) {
        side = el.dataset.appSide === "pinned" ? "pinned" : "running";
        break;
      }
    }
    if (!side) {
      side = (pinnedSideLeft ? clientX < middle : clientX > middle) ? "pinned" : "running";
    }
    const sideButtons = buttons.filter(
      (el) => (el.dataset.appSide === "pinned") === (side === "pinned"),
    );
    const centers = sideButtons.map(layoutCenterX);
    let index = centers.filter((centerX) => centerX < clientX).length;
    const previous = drag.target;
    if (previous) {
      if (previous.side !== side) {
        // Changing sections requires a decisive crossing of the divider.
        const crossed = pinnedSideLeft
          ? side === "pinned"
            ? clientX <= middle - SWAP_MARGIN
            : clientX >= middle + SWAP_MARGIN
          : side === "pinned"
            ? clientX >= middle + SWAP_MARGIN
            : clientX <= middle - SWAP_MARGIN;
        if (!crossed) {
          return previous;
        }
      } else if (index !== previous.index) {
        if (index > previous.index) {
          const boundary = centers[index - 1];
          if (clientX < boundary + SWAP_MARGIN) {
            index = previous.index;
          }
        } else {
          const boundary = centers[index];
          if (clientX > boundary - SWAP_MARGIN) {
            index = previous.index;
          }
        }
      }
    }
    return { side, index };
  };

  const commitAppDrag = (
    drag: { name: string; appId: number; side: "pinned" | "running" },
    target: { side: "pinned" | "running"; index: number },
  ) => {
    const pinnedNames = sortedPinned.map((entry) => entry.app.processName);
    const runningNames = sortedActive.map((app) => app.processName);
    const orderedNames = (names: string[], order: string[]) => {
      const without = names.filter((entry) => entry !== drag.name);
      const index = Math.min(Math.max(target.index, 0), without.length);
      without.splice(index, 0, drag.name);
      // Names not currently displayed (closed pinned apps) keep their saved
      // spots behind the displayed ones.
      const extras = order.filter((entry) => !without.includes(entry));
      return [...without, ...extras];
    };
    if (target.side === drag.side) {
      save(
        target.side === "pinned"
          ? { taskbarPinOrder: orderedNames(pinnedNames, pinOrder) }
          : { taskbarAppOrder: orderedNames(runningNames, appOrder) },
      );
      return;
    }
    if (target.side === "pinned") {
      // Dropping a running app on the pinned side pins it (at that spot).
      if (drag.appId !== -1) {
        void pinApp(drag.appId).then(onPinnedChange);
      }
      save({ taskbarPinOrder: orderedNames(pinnedNames, pinOrder) });
    } else {
      // Dropping a pinned app on the running side unpins it.
      void unpinApp(drag.name).then(onPinnedChange);
      save({ taskbarAppOrder: orderedNames(runningNames, appOrder) });
    }
  };

  const handleDragMove = (event: PointerEvent) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    if (!drag.moved) {
      const dx = event.clientX - drag.startX;
      const dy = event.clientY - drag.startY;
      if (Math.hypot(dx, dy) < 6) {
        return;
      }
      drag.moved = true;
      drag.lastX = drag.startX;
      drag.lastY = drag.startY;
      drag.offsetX = 0;
      drag.offsetY = 0;
      drag.slotLeft = drag.element.offsetLeft;
      drag.slotTop = drag.element.offsetTop;
      drag.pickupStart = performance.now();
      lastDragActivityRef.current = drag.pickupStart;
      drag.element.classList.add("app-dragging");
      document.body.classList.add("app-drag-active");
      startPickupGrow(drag);
    }
    // The offset tracks pointer motion incrementally; the layout effect
    // subtracts slot movements, so the button keeps following the pointer
    // even as its slot shifts under the re-order preview.
    drag.offsetX += event.clientX - drag.lastX;
    drag.offsetY += event.clientY - drag.lastY;
    drag.lastX = event.clientX;
    drag.lastY = event.clientY;
    applyDragTransform(drag);
    const target = findDropTarget(event.clientX, drag);
    const changed =
      !target ||
      !drag.target ||
      target.side !== drag.target.side ||
      target.index !== drag.target.index;
    drag.target = target;
    if (target && changed) {
      // The re-order preview commits synchronously, so the neighbors part
      // right away and the drop never lags a render behind the pointer.
      flushSync(() => {
        setDragPreview({ name: drag.name, side: target.side, index: target.index });
      });
    }
  };

  const handleDragUp = (event: PointerEvent) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) {
      return;
    }
    dragRef.current = null;
    cancelAnimationFrame(drag.pickupRaf);
    document.body.classList.remove("app-drag-active");
    if (!drag.moved || !drag.target) {
      return;
    }
    // The click that follows the pointer-up must not activate the app.
    suppressClickRef.current = true;
    // Keep the FLIP window open across the settle spring and the snapshot
    // that materializes a cross-side drop's preview.
    lastDragActivityRef.current = performance.now();
    const { element, offsetX, offsetY, scale, target } = drag;
    if (target.side === drag.side) {
      // Same-section drop: the preview slot is the final slot, so the button
      // springs from its ghost position into place.
      element.classList.remove("app-dragging");
      commitAppDrag(drag, target);
      startSettleSpring(element, offsetX, offsetY, scale);
      setDragPreview(null);
    } else {
      // Cross-side (pin/unpin): the button has already re-rendered into the
      // other section; keep that preview until the refreshed snapshot makes
      // it real, so the app never snaps back (or vanishes) mid-transition.
      element.classList.remove("app-dragging");
      element.style.transform = "";
      commitAppDrag(drag, target);
      pendingPreviewClearRef.current = true;
    }
  };

  dragMoveRef.current = handleDragMove;
  dragUpRef.current = handleDragUp;

  useEffect(() => {
    const move = (event: PointerEvent) => dragMoveRef.current(event);
    const up = (event: PointerEvent) => dragUpRef.current(event);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, []);

  useEffect(
    () => () => {
      document.body.classList.remove("app-drag-active");
      // An unmount mid-resize-drag would otherwise leave the width
      // pipeline in measure-only mode forever (no width updates apply).
      resizeDragActive.active = false;
      resizeMeasureOnly.active = false;
      document.body.classList.remove("taskbar-resizing");
    },
    [],
  );

  // A cross-side drop keeps its preview until the refreshed snapshot makes
  // it real (see handleDragUp); this clears it on the next snapshot.
  const pendingPreviewClearRef = useRef(false);
  useEffect(() => {
    if (!pendingPreviewClearRef.current) {
      return;
    }
    pendingPreviewClearRef.current = false;
    setDragPreview(null);
  }, [snapshot]);

  const beginAppDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) {
      return;
    }
    const button = (event.target as HTMLElement).closest<HTMLElement>("[data-app-entry]");
    if (!button) {
      return;
    }
    dragRef.current = {
      pointerId: event.pointerId,
      name: button.dataset.appName ?? "",
      appId: Number(button.dataset.appId ?? -1),
      side: button.dataset.appSide === "pinned" ? "pinned" : "running",
      element: button,
      startX: event.clientX,
      startY: event.clientY,
      lastX: event.clientX,
      lastY: event.clientY,
      offsetX: 0,
      offsetY: 0,
      scale: 1,
      slotLeft: 0,
      slotTop: 0,
      pickupStart: 0,
      pickupRaf: 0,
      moved: false,
      target: null,
    };
  };

  const suppressClickAfterDrag = (event: React.MouseEvent) => {
    if (!suppressClickRef.current) {
      return;
    }
    suppressClickRef.current = false;
    event.preventDefault();
    event.stopPropagation();
  };

  // Drag-synced motion: while a drag is in flight (or right after a drop),
  // every commit FLIP-animates the buttons that changed slots — the "swipe"
  // as the dragged app moves past its neighbors — and compensates the
  // dragged button's ghost offset for its own slot movement so it keeps
  // tracking the pointer. Commits without a drag (pill resizes, snapshot
  // refreshes) only refresh the positions map, so buttons snap instead of
  // sliding.
  const previousPositionsRef = useRef<Map<string, { left: number; top: number }>>(new Map());
  const flipRafRef = useRef(0);
  const lastDragActivityRef = useRef(0);
  useLayoutEffect(() => {
    const center = centerRef.current;
    if (!center) {
      return;
    }
    const drag = dragRef.current;
    // FLIP-animate shifts only while a drag is in flight (or its settle
    // spring is still running). Every other commit that moves the buttons —
    // pill recentering when a media title changes the width, snapshot
    // refreshes — must snap: animating it here is the "apps slide in ~1s
    // after a resize" artifact.
    const dragActive =
      (drag?.moved ?? false) ||
      performance.now() - lastDragActivityRef.current < SETTLE_DURATION_MS + 60;
    const buttons = Array.from(center.querySelectorAll<HTMLElement>("[data-app-entry]"));
    const shifted: HTMLElement[] = [];
    const positions = new Map<string, { left: number; top: number }>();
    for (const el of buttons) {
      const name = el.dataset.appName ?? "";
      const left = el.offsetLeft;
      const top = el.offsetTop;
      positions.set(name, { left, top });
      if (!dragActive) {
        continue;
      }
      const previous = previousPositionsRef.current.get(name);
      if (!previous || el === drag?.element) {
        continue;
      }
      const dLeft = left - previous.left;
      const dTop = top - previous.top;
      if (dLeft || dTop) {
        el.style.transition = "none";
        el.style.transform = `translate(${-dLeft}px, ${-dTop}px)`;
        shifted.push(el);
      }
    }
    if (drag?.moved) {
      const dLeft = drag.element.offsetLeft - drag.slotLeft;
      const dTop = drag.element.offsetTop - drag.slotTop;
      if (dLeft || dTop) {
        drag.slotLeft += dLeft;
        drag.slotTop += dTop;
        // Layout offsets are pre-zoom local units; the ghost offsets
        // accumulate viewport (post-zoom) pixels — scale the slot movement
        // before compensating, so the ghost keeps tracking the pointer.
        const ui = uiScale();
        drag.offsetX -= dLeft * ui;
        drag.offsetY -= dTop * ui;
        applyDragTransform(drag);
      }
    }
    previousPositionsRef.current = positions;
    if (shifted.length > 0) {
      cancelAnimationFrame(flipRafRef.current);
      flipRafRef.current = requestAnimationFrame(() => {
        for (const el of shifted) {
          el.style.transition = "";
          el.style.transform = "";
        }
      });
    }
  });

  // While a drag is in flight, the displayed sequences preview the drop: the
  // dragged app sits at the target slot and its neighbors part around it —
  // including across the pinned/running boundary, where the app renders as a
  // virtual member of the other section instead of vanishing.
  const previewed = (() => {
    if (!dragPreview) {
      return { pinned: sortedPinned, running: sortedActive };
    }
    const name = dragPreview.name;
    const pinNames = sortedPinned
      .map((entry) => entry.app.processName)
      .filter((entry) => entry !== name);
    const runNames = sortedActive
      .map((app) => app.processName)
      .filter((entry) => entry !== name);
    const targetNames = dragPreview.side === "pinned" ? pinNames : runNames;
    const index = Math.min(Math.max(dragPreview.index, 0), targetNames.length);
    targetNames.splice(index, 0, name);
    const asPinnedEntry = (entryName: string) => {
      const entry =
        sortedPinned.find((candidate) => candidate.app.processName === entryName) ?? null;
      if (entry) {
        return entry;
      }
      const app = sortedActive.find((candidate) => candidate.processName === entryName);
      return app
        ? { key: entryName, launchPath: null, app, instances: instances.get(entryName) ?? [] }
        : null;
    };
    const asRunningApp = (entryName: string) =>
      sortedActive.find((candidate) => candidate.processName === entryName) ??
      sortedPinned.find((candidate) => candidate.app.processName === entryName)?.app ??
      null;
    return {
      pinned: pinNames
        .map(asPinnedEntry)
        .filter((entry): entry is NonNullable<typeof entry> => entry !== null),
      running: runNames
        .map(asRunningApp)
        .filter((app): app is NonNullable<typeof app> => app !== null),
    };
  })();

  // One flat keyed list (divider included) so React preserves the DOM nodes
  // by process name across section changes — the dragged button never
  // unmounts mid-drag, not even when the preview moves it across sides.
  const centerItems: Array<
    | {
        kind: "app";
        key: string;
        app: TaskbarApp;
        instances: TaskbarApp[];
        launchPath: string | null;
        isPinned: boolean;
        position: number;
      }
    | { kind: "divider" }
  > = [];
  const pinnedItems = previewed.pinned.map((entry, position) => ({
    kind: "app" as const,
    key: entry.app.processName,
    app: entry.app,
    instances: entry.instances,
    launchPath: entry.launchPath,
    isPinned: true,
    position,
  }));
  const runningItems = previewed.running.map((app, position) => ({
    kind: "app" as const,
    key: app.processName,
    app,
    instances: instances.get(app.processName) ?? [app],
    launchPath: null,
    isPinned: false,
    position,
  }));
  const dividerItem = { kind: "divider" as const };
  if (pinnedItems.length > 0) {
    if (pinnedSideLeft) {
      centerItems.push(...pinnedItems, dividerItem, ...runningItems);
    } else {
      centerItems.push(...runningItems, dividerItem, ...pinnedItems);
    }
  } else {
    centerItems.push(...runningItems);
  }

  return (
    <main className="taskbar-shell">
      <div className="liquid-glass" />
      <div
        className="taskbar-resize-handle"
        title="Drag to resize the taskbar"
        onPointerDown={beginResizeDrag}
      />
      <nav className="taskbar-content" aria-label="Glassbar taskbar">
        <div className="taskbar-left" aria-label="Gadgets">
          {enabledGadgets.length > 0 && (
            <div className="taskbar-gadget-shell">
              {enabledGadgets.map(({ id, Component }) => (
                <div className="taskbar-gadget" key={id} data-gadget={id}>
                  <Component />
                </div>
              ))}
            </div>
          )}
        </div>

        <div
          className="taskbar-center"
          aria-label="Applications"
          ref={centerRef}
          onPointerDown={beginAppDrag}
          onClickCapture={suppressClickAfterDrag}
        >
          {centerItems.map((item) =>
            item.kind === "divider" ? (
              <span key="divider" className="taskbar-divider" aria-hidden="true" />
            ) : (
              <AppTaskbarButton
                key={item.key}
                app={item.app}
                instances={item.instances}
                launchPath={item.launchPath}
                position={item.position}
                isPinned={item.isPinned}
                onTogglePin={handlePinToggle}
              />
            ),
          )}
        </div>

<div className="taskbar-right">
          <div className="taskbar-gadget-shell">
          {media?.title && <MediaGadget media={media} />}
          </div>
          <ControlCenter
            open={isSettingsOpen}
            onOpenChange={onSettingsOpenChange}
            trigger={
              <IconButton
                className="settings-icon-button"
                aria-label="Glassbar control center"
                onPointerEnter={() => void prewarmMenuGlass(7)}
              >
                <Cog size={22} />
              </IconButton>
            }
          />
        </div>
      </nav>
    </main>
  );
}

