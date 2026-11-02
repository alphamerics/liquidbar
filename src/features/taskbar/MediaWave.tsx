import { useEffect, useRef } from "react";

const IDLE_LEVELS = [0.1, 0.1, 0.1, 0.1, 0.1];

/**
 * The five-bar audio waveform. Levels stream from the native monitor ~20
 * times per second while audio plays; bars update through direct refs so the
 * stream never re-renders the React tree. When the monitor goes idle it
 * emits one static frame and then stays silent, leaving these bars as-is.
 */
export function MediaWave() {
  const barRefs = useRef<Array<HTMLSpanElement | null>>([]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<number[]>("media-levels", (event) => {
          const levels = event.payload;
          if (!Array.isArray(levels) || levels.length !== 5) {
            return;
          }
          for (let index = 0; index < 5; index += 1) {
            const bar = barRefs.current[index];
            if (!bar) {
              continue;
            }
            const height = `${Math.round(Math.max(10, levels[index] * 100))}%`;
            if (bar.style.height !== height) {
              bar.style.height = height;
            }
          }
        }),
      )
      .then((fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  return (
    <span className="media-wave" aria-hidden="true">
      {IDLE_LEVELS.map((level, index) => (
        <span
          key={index}
          ref={(node) => {
            barRefs.current[index] = node;
          }}
          className="media-wave-bar"
          style={{ height: `${Math.round(level * 100)}%` }}
        />
      ))}
    </span>
  );
}
