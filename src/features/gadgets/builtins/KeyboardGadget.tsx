import { Keyboard as KeyboardIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { getKeyboardLayout } from "../../../lib/shell";

/**
 * Keyboard gadget: the focused app's active input language, the same
 * indicator Windows shows in the corner of its own taskbar ("ENG" and
 * friends). Layout is per-thread, so switching apps can switch the display;
 * polling picks that up alongside manual layout switches.
 */
export default function KeyboardGadget() {
  const [short, setShort] = useState("");
  const [name, setName] = useState("Keyboard layout");
  useEffect(() => {
    let cancelled = false;
    const poll = () => {
      void getKeyboardLayout().then((layout) => {
        if (cancelled || !layout || !layout.short) {
          return;
        }
        setShort(layout.short);
        setName(layout.name || layout.short);
      });
    };
    poll();
    const timer = window.setInterval(poll, 500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  return (
    <>
      <KeyboardIcon size={18} strokeWidth={2} />
      <span className="taskbar-gadget-text" title={name}>
        {short || "—"}
      </span>
    </>
  );
}
