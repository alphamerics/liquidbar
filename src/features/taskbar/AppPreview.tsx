import { useCallback, useEffect, useId, useRef } from "react";
import type { CSSProperties } from "react";
import { createPortal } from "react-dom";
import { activateWindow, setWindowPreview, type TaskbarApp } from "../../lib/shell";
import { useLiquidGlassSurface } from "glass-ui";
import { AppIcon } from "./AppIcon";

const PREVIEW_WIDTH = 264;
const VIEWPORT_MARGIN = 12;
// The glass frame's corner radius in physical px (the value handed to
// useLiquidGlassSurface is not DPI-scaled, unlike the surface's rect).
const GLASS_RADIUS_PX = 20;

interface AppPreviewProps {
  app: TaskbarApp;
  anchor: DOMRect;
  onHover: (hovering: boolean) => void;
}

/**
 * The hover preview popup: a liquid glass box floating above the app button
 * with a live DWM thumbnail of the app's window inside it. The image area's
 * rect is pushed to the native side, which hosts the actual thumbnail; the
 * glass frame and title row live in the webview.
 */
export function AppPreview({ app, anchor, onHover }: AppPreviewProps) {
  const previewId = useId();
  const imageRef = useRef<HTMLDivElement | null>(null);
  const frame = useRef(0);
  const hoverRef = useRef(onHover);
  hoverRef.current = onHover;

  // "preview:" prefix makes the renderer drive the shared persistent preview
  // surface (pre-warmed, re-anchored per open) instead of a new menu surface.
  useLiquidGlassSurface(
    true,
    `[data-app-preview="${previewId}"]`,
    GLASS_RADIUS_PX,
    `preview${previewId}`,
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    // The thumbnail popup sits above the webview and reports its own mouse
    // hover so the preview stays open while the cursor is over the image.
    void import("@tauri-apps/api/event").then(({ listen }) =>
      listen<boolean>("preview-hover", (event) =>
        hoverRef.current(event.payload === true),
      ).then((fn) => {
        unlisten = fn;
      }),
    );
    return () => unlisten?.();
  }, []);

  const push = useCallback(() => {
    const node = imageRef.current;
    if (!node) {
      return;
    }
    const rect = node.getBoundingClientRect();
    if (rect.width < 1 || rect.height < 1) {
      return;
    }
    const scale = window.devicePixelRatio || 1;
    // Nested border-radius rule, in physical px: the capture's corner
    // radius must be the glass frame's own rounding minus the popup's
    // padding inset, so the curves stay concentric. The glass frame's
    // radius is DPI-unscaled (see useLiquidGlassSurface), while the
    // padding is CSS px and is scaled here.
    const container = node.parentElement;
    const containerStyle = container ? getComputedStyle(container) : null;
    const paddingPx = (parseFloat(containerStyle?.paddingLeft ?? "") || 0) * scale;
    void setWindowPreview(app.id, {
      x: rect.left * scale,
      y: rect.top * scale,
      width: rect.width * scale,
      height: rect.height * scale,
      radius: Math.max(GLASS_RADIUS_PX - paddingPx, 0),
    });
  }, [app.id]);
  const pushSettled = useCallback(() => {
    if (frame.current) {
      return;
    }
    frame.current = requestAnimationFrame(() => {
      frame.current = requestAnimationFrame(() => {
        frame.current = 0;
        push();
      });
    });
  }, [push]);
  useEffect(() => {
    pushSettled();
    const timer = window.setInterval(pushSettled, 500);
    return () => {
      if (frame.current) {
        cancelAnimationFrame(frame.current);
      }
      window.clearInterval(timer);
      void setWindowPreview(null, null);
    };
  }, [pushSettled]);
  // Re-anchor: a pill resize can slide the anchor button (and this preview
  // with it) sideways while open; push the fresh image rect so the native
  // thumbnail follows immediately instead of within the 500ms interval tick.
  useEffect(() => {
    pushSettled();
  }, [anchor, pushSettled]);

  const left = Math.min(
    Math.max(anchor.left + anchor.width / 2 - PREVIEW_WIDTH / 2, VIEWPORT_MARGIN),
    window.innerWidth - PREVIEW_WIDTH - VIEWPORT_MARGIN,
  );
  const style: CSSProperties = {
    left,
    bottom: window.innerHeight - anchor.top + 13,
    width: PREVIEW_WIDTH,
  };

  // Portal to document.body: the pill resize spring squashes-and-stretches
  // .taskbar-shell with a CSS transform every frame, and a transformed
  // ancestor becomes the containing block for position:fixed descendants —
  // the preview would ride the squash, its rect would change every frame,
  // and each change would re-anchor the preview glass (spawning a native
  // preview spring that kills the pill spring mid-flight, leaving the
  // taskbar stranded mid-resize). Rendered at body level the preview's
  // position:fixed coordinates track the real viewport and stay still
  // through the whole spring; the hoverPanel's post-settle revalidation
  // re-anchors it once, after the pill has settled.
  return createPortal(
    <div
      className="app-preview"
      data-app-preview={previewId}
      data-app-id={app.id}
      style={style}
      onPointerEnter={() => onHover(true)}
      onPointerLeave={() => onHover(false)}
      onClick={() => void activateWindow(app.id)}
    >
      <div className="app-preview-image" ref={imageRef}>
        <span className="app-icon app-preview-placeholder">
          <AppIcon app={app} color="#2779d8" />
        </span>
      </div>
      <div className="app-preview-title">
        <span className="app-icon app-preview-icon">
          <AppIcon app={app} color="#2779d8" />
        </span>
        <span className="app-preview-title-text">{app.title}</span>
      </div>
    </div>,
    document.body,
  );
}
