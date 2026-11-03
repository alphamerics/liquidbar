import type { KeyboardEvent } from "react";
import { useState } from "react";

/** The bare modifier keys never form a combo on their own. */
const MODIFIER_CODES = new Set([
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
]);

/** Named key codes, canonicalized to the tokens the native parser accepts. */
const KEY_NAMES: Record<string, string> = {
  Space: "Space",
  Tab: "Tab",
  Enter: "Enter",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  PrintScreen: "PrintScreen",
  Minus: "-",
  Equal: "=",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
};

/** The main (non-modifier) key of a keyboard event as its combo token. */
function mainKey(event: KeyboardEvent): string | null {
  const code = event.code;
  if (MODIFIER_CODES.has(code)) {
    return null;
  }
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) {
    return letter[1];
  }
  const digit = /^(?:Digit|Numpad)([0-9])$/.exec(code);
  if (digit) {
    return digit[1];
  }
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) {
    return code;
  }
  return KEY_NAMES[code] ?? null;
}

/** Builds the "+"-joined combo string from a keyboard event, or null when
 * the event is a bare modifier press (no combo yet). */
function comboFromEvent(event: KeyboardEvent): string | null {
  const main = mainKey(event);
  if (!main) {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.metaKey) {
    parts.push("Win");
  }
  parts.push(main);
  return parts.join("+");
}

interface HotkeyInputProps {
  /** The current combo, e.g. "Ctrl+Alt+Shift+H". */
  value: string;
  onChange: (value: string) => void;
}

/** A click-to-record shortcut control: while recording, the next key press
 * (with at least one modifier) becomes the combo; Escape cancels. */
export function HotkeyInput({ value, onChange }: HotkeyInputProps) {
  const [recording, setRecording] = useState(false);

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") {
      setRecording(false);
      event.currentTarget.blur();
      return;
    }
    const combo = comboFromEvent(event);
    // A combo needs at least one modifier (the Ctrl/Alt/Shift/Win prefix):
    // a bare key would swallow that key system-wide.
    if (combo && combo.split("+").length > 1) {
      onChange(combo);
      setRecording(false);
      event.currentTarget.blur();
    }
  };

  return (
    <button
      type="button"
      className="settings-hotkey-input"
      data-recording={recording}
      title="Click, then press the new shortcut (Escape cancels)"
      onClick={() => setRecording(true)}
      onBlur={() => setRecording(false)}
      onKeyDown={handleKeyDown}
    >
      {recording ? "Press keys…" : value}
    </button>
  );
}
