import { CalendarDays } from "lucide-react";
import { useEffect, useState } from "react";

/**
 * EXAMPLE — custom gadget.
 *
 * Custom gadgets are NOT part of the app source. Copy a file like this one
 * into Glassbar's app data folder (Settings > Gadgets > "Open folder"; the
 * same folder as settings.json), press Reload in the settings, and the
 * gadget is transpiled and loaded at runtime — no rebuild needed.
 *
 * The contract:
 *  - default-export a React component that renders the gadget's inline
 *    content (icon + text, side by side). It is mounted inside a
 *    `.taskbar-gadget` pill in the taskbar's left zone.
 *  - optionally export `gadget = { name: "..." }` for the display name;
 *    otherwise the file name is used (e.g. "MyGadget.tsx" -> "My Gadget").
 *  - the file name without extension is the gadget's settings id.
 *
 * Available imports: react, lucide-react (every icon),
 * @tauri-apps/api/core (invoke), @tauri-apps/api/event (listen).
 * Use `className="taskbar-gadget-text"` for the text so the taskbar sizes
 * its pill to fit.
 */

export const gadget = { name: "Date" };

export default function DateGadget() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <>
      <CalendarDays size={18} strokeWidth={2} />
      <span
        className="taskbar-gadget-text"
        title={now.toLocaleDateString(undefined, {
          weekday: "long",
          year: "numeric",
          month: "long",
          day: "numeric",
        })}
      >
        {now.toLocaleDateString(undefined, {
          weekday: "short",
          month: "short",
          day: "numeric",
        })}
      </span>
    </>
  );
}
