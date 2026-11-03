import { useCallback, useEffect, useRef, useState } from "react";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";

/** The hover-open dwell (app previews) and the travel grace (all popups). */
export const HOVER_OPEN_DELAY_MS = 750;
export const HOVER_CLOSE_GRACE_MS = 350;

/**
 * Geometry hooks for the resize revalidation (the third guard below): the
 * source element's CURRENT rect and the open popup's rect, both read fresh
 * at revalidation time. Neither has a default — without them a panel simply
 * keeps the pre-existing behavior (DOM events + `hover-blur` only).
 */
export interface HoverPanelGeometry {
  /** The hovered element (button, gadget) at its live position. */
  sourceRect?: () => DOMRect | null;
  /** The popup this panel opened, at its live position. */
  popupRect?: () => DOMRect | null;
}

/**
 * The one hover machine for every popup anchored above the taskbar: the app
 * window previews (open after a hover dwell on the button) and the media
 * hover card (open on click) share it — the same grace numbers, the same
 * cancel rules, one chronological model. A popup opens only through
 * `open`/`scheduleOpen`, closes only through `close`/`scheduleClose`, and
 * never leaves a timer running across a state change.
 *
 * The invariant that keeps a popup from outliving its cursor: leaving the
 * source element ALWAYS arms the close grace, and only a demonstrable hover
 * (the source again, or the popup itself) cancels it. The delicate case is
 * a leave the webview never sees: the overlay goes click-through the
 * instant the cursor steps off every hit-testable region (button, open
 * popup, open menu), and the webview then stops receiving input — a
 * pending `pointerleave` may simply never be delivered. Three guards
 * cover that hole. First, the grace arming means every exit path leads
 * to a timer, not to an open-ended "still hovering" belief. Second, the
 * native hit-test loop — the only side that actually sees the cursor in
 * that blind state — emits one `hover-blur` edge per such crossing,
 * which this hook bridges to the same grace arming while the popup is
 * open. Reaching the popup (or the source) again cancels the grace, so
 * travel between them stays fluid. Third, a pill resize under a PARKED
 * cursor: the taskbar's buttons re-center, sliding the source (and the
 * preview anchored to it) away without any mouse input and without the
 * cursor leaving a hit rect (it can land on the neighboring button), so
 * neither a leave event nor `hover-blur` fires and the "still hovered"
 * belief goes stale — the popup parks itself open until the user happens
 * to hover another app. A resize is the one signal the webview does see:
 * on each one, the real cursor position (a native query, in screen
 * coordinates) re-tests the belief against the source's current rect and
 * the popup's — over the source re-anchors the panel to the moved
 * element, over the popup itself is demonstrated hover, anywhere else is
 * a leave like any other.
 */
export function useHoverPanel(geometry?: HoverPanelGeometry) {
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  // The anchor mirrors into a ref so the callbacks can tell open from
  // closed without taking `anchor` into their dependencies (they never
  // re-arm each other's timers through a stale closure).
  const anchorRef = useRef<DOMRect | null>(null);
  const openTimer = useRef(0);
  const closeTimer = useRef(0);
  // The geometry hooks mirror into a ref: the revalidation effect below
  // subscribes once per open session and must always call the latest
  // getters (callers pass inline closures that change every render).
  const geometryRef = useRef<HoverPanelGeometry | undefined>(geometry);
  geometryRef.current = geometry;

  useEffect(
    () => () => {
      window.clearTimeout(openTimer.current);
      window.clearTimeout(closeTimer.current);
    },
    [],
  );

  const commit = useCallback((rect: DOMRect | null) => {
    anchorRef.current = rect;
    setAnchor(rect);
  }, []);

  /** Opens now. Cancels both timers first, so a pending dwell or close can
   * never fire around a fresh state change. */
  const openNow = useCallback(
    (rect: DOMRect) => {
      window.clearTimeout(openTimer.current);
      window.clearTimeout(closeTimer.current);
      commit(rect);
    },
    [commit],
  );

  /** Closes now (the click-toggle path, a source menu taking over, unmount
   * semantics). Cancels both timers the same way. */
  const close = useCallback(() => {
    window.clearTimeout(openTimer.current);
    window.clearTimeout(closeTimer.current);
    commit(null);
  }, [commit]);

  /** The hover-dwell open (app previews): after `delayMs` with the pointer
   * still on the source, opens with the rect captured AT FIRE TIME (the
   * source can shift between the enter and the dwell). An already-open
   * panel holds instead of re-running the dwell — re-entering the source
   * after a cancelled close must just cancel the close, which the callers
   * do explicitly before calling this. */
  const scheduleOpen = useCallback(
    (
      source: DOMRect | (() => DOMRect | null),
      delayMs = HOVER_OPEN_DELAY_MS,
    ) => {
      if (anchorRef.current) {
        return;
      }
      window.clearTimeout(openTimer.current);
      window.clearTimeout(closeTimer.current);
      openTimer.current = window.setTimeout(() => {
        const rect = typeof source === "function" ? source() : source;
        if (rect) {
          commit(rect);
        }
      }, delayMs);
    },
    [commit],
  );

  /** A dwell still pending (the pointer left the source before it fired). */
  const cancelOpen = useCallback(
    () => window.clearTimeout(openTimer.current),
    [],
  );

  /** Arms the close grace: the popup closes in HOVER_CLOSE_GRACE_MS unless
   * something demonstrates the cursor reached the popup (or back onto the
   * source) before it elapses. */
  const scheduleClose = useCallback(() => {
    window.clearTimeout(openTimer.current);
    window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(
      () => commit(null),
      HOVER_CLOSE_GRACE_MS,
    );
  }, [commit]);

  /** The cursor demonstrably reached the popup (or the source) again. */
  const cancelClose = useCallback(
    () => window.clearTimeout(closeTimer.current),
    [],
  );

  // The authoritative native leave (the class comment): emitted once per
  // "cursor left every hit-testable region" crossing while the overlay has
  // gone click-through. Subscribed only while the popup is open, so no
  // closed panel ever sits on the event bus; `scheduleClose` is stable for
  // the hook's lifetime (it only depends on `commit`).
  const isOpen = anchor !== null;
  useEffect(() => {
    if (!isOpen) {
      return;
    }
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen("hover-blur", () => scheduleClose()).then((fn) => {
        unlisten = fn;
      }),
    );
    return () => unlisten?.();
  }, [isOpen, scheduleClose]);

  // The third guard (the class comment): re-test the "still hovered" belief
  // whenever the overlay resizes. Subscribed only while the popup is open,
   // with the geometry hooks read fresh at revalidation time (they live in
   // geometryRef); `openNow`/`cancelClose`/`scheduleClose` are stable.
  useEffect(() => {
    if (!isOpen) {
      return;
    }
    let timer = 0;
    let cancelled = false;
    const revalidate = async () => {
      const hooks = geometryRef.current;
      const source = hooks?.sourceRect?.() ?? null;
      const popup = hooks?.popupRect?.() ?? null;
      if (!source && !popup) {
        return;
      }
      let cursor: { x: number; y: number } | undefined;
      let outer: { x: number; y: number } | undefined;
      try {
        [cursor, outer] = await Promise.all([
          cursorPosition(),
          getCurrentWindow().outerPosition(),
        ]);
      } catch {
        // Without a cursor answer the belief cannot be re-tested — the
        // native `hover-blur` edge stays the fallback for the blind case.
        return;
      }
      if (cancelled || !cursor || !outer) {
        return;
      }
      // Screen physical px → viewport CSS px (the overlay's own space).
      const scale = window.devicePixelRatio || 1;
      const x = (cursor.x - outer.x) / scale;
      const y = (cursor.y - outer.y) / scale;
      const inside = (rect: DOMRect) =>
        x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
      if (source && inside(source)) {
        // Still on the source: re-anchor to the moved button — which also
        // heals the stale open-time anchor this session was committed with.
        openNow(source);
      } else if (popup && inside(popup)) {
        // The cursor demonstrably reached the popup itself.
        cancelClose();
      } else {
        // The cursor is demonstrably elsewhere: the same exit as a
        // pointerleave — arm the grace rather than closing out from under
        // a possibly-traveling cursor.
        scheduleClose();
      }
    };
    const schedule = () => {
      window.clearTimeout(timer);
      // The pill resize spring squashes-and-stretches .taskbar-shell for
      // ~320ms while it settles; a revalidation during that window would
      // read transformed (squashed) button rects and re-anchor the popup
      // to a position that no longer exists once the spring lands. Wait
      // out the full spring: 400ms covers the longest settle plus slack.
      timer = window.setTimeout(() => void revalidate(), 400);
    };
    window.addEventListener("resize", schedule);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      window.removeEventListener("resize", schedule);
    };
  }, [isOpen, openNow, cancelClose, scheduleClose]);

  /** The popup's hover bridge — its DOM pointerenter/leave and, for the app
   * previews, the native thumbnail popup's own `preview-hover` events. */
  const onPopupHover = useCallback(
    (hovering: boolean) => (hovering ? cancelClose() : scheduleClose()),
    [cancelClose, scheduleClose],
  );

  return {
    anchor,
    isOpen,
    open: openNow,
    close,
    scheduleOpen,
    cancelOpen,
    scheduleClose,
    cancelClose,
    onPopupHover,
  };
}
