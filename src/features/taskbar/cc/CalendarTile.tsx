import { Subview } from "./Subview";
import { CalendarMonth } from "./CalendarMonth";

/**
 * The Calendar sub-view (opened from the Calendar pill, like the Wi-Fi and
 * Focus views): the current month — navigable — with today highlighted.
 */
export function CalendarTile({ onBack }: { onBack: () => void }) {
  return (
    <Subview
      title="Calendar"
      onBack={onBack}
      contentClassName="cc-cal"
      headerClassName="cc-cal-header"
    >
      <CalendarMonth />
    </Subview>
  );
}
