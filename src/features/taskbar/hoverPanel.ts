import { useCallback, useEffect, useRef, useState } from "react";

/** The hover-open dwell (app previews) and the travel grace (all popups). */
export const HOVER_OPEN_DELAY_MS = 750;
export const HOVER_CLOSE_GRACE_MS = 350;

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
 * pending `pointerleave` may simply never be delivered. Two guards cover
 * that hole. First, the grace arming means every exit path leads to a
 * timer, not to an open-ended "still hovering" belief. Second, the native
 * hit-test loop — the only side that actually sees the cursor in that
 * blind state — emits one `hover-blur` edge per such crossing, which this
 * hook bridges to the same grace arming while the popup is open. Reaching
 * the popup (or the source) again cancels the grace, so travel between
 * them stays fluid.
 */
export function useHoverPanel() {
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  // The anchor mirrors into a ref so the callbacks can tell open from
  // closed without taking `anchor` into their dependencies (they never
  // re-arm each other's timers through a stale closure).
  const anchorRef = useRef<DOMRect | null>(null);
  const openTimer = useRef(0);
  const closeTimer = useRef(0);

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
