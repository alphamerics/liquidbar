import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Clock as ClockIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { GlassCard } from "glass-ui";
import { CalendarMonth } from "../../taskbar/cc/CalendarMonth";

/** The long-form date, shown as the trigger's tooltip. */
const fullDate = (date: Date) =>
  date.toLocaleDateString(undefined, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  });

/** Clock gadget: the local Windows system time, ticking every second.
 * Clicking it opens a calendar popup (the same month grid as the control
 * center's Calendar view) above the taskbar. */
export default function ClockGadget() {
  const [now, setNow] = useState(() => new Date());
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 1_000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <DropdownMenu.Root open={open} onOpenChange={setOpen}>
      <DropdownMenu.Trigger asChild>
        <button
          type="button"
          className="clock-gadget cc-hit"
          title={fullDate(now)}
          aria-label="Clock — open calendar"
        >
          <ClockIcon size={18} strokeWidth={2} />
          <span className="taskbar-gadget-text">
            {now.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })}
          </span>
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          className="clock-cal-content"
          side="top"
          align="start"
          sideOffset={13}
          collisionPadding={14}
          onCloseAutoFocus={(event) => event.preventDefault()}
        >
          <GlassCard className="cc-cal">
            <CalendarMonth />
          </GlassCard>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
