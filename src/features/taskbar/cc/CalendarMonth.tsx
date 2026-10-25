import { ChevronLeft, ChevronRight } from "lucide-react";
import { useState } from "react";

const WEEKDAY_LETTERS = ["S", "M", "T", "W", "T", "F", "S"];

/**
 * The navigable month grid: the month header (prev/next arrows around the
 * month label), the weekday row and the day cells with today highlighted.
 * Shared by the control center's Calendar sub-view and the clock gadget's
 * calendar popup.
 */
export function CalendarMonth() {
  const [month, setMonth] = useState(() => {
    const now = new Date();
    return new Date(now.getFullYear(), now.getMonth(), 1);
  });
  const year = month.getFullYear();
  const monthIndex = month.getMonth();
  const monthLabel = month.toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });
  const firstWeekday = new Date(year, monthIndex, 1).getDay();
  const daysInMonth = new Date(year, monthIndex + 1, 0).getDate();
  const cells: Array<number | null> = [];
  for (let i = 0; i < firstWeekday; i++) {
    cells.push(null);
  }
  for (let day = 1; day <= daysInMonth; day++) {
    cells.push(day);
  }
  while (cells.length % 7 !== 0) {
    cells.push(null);
  }

  const shift = (delta: number) => setMonth(new Date(year, monthIndex + delta, 1));
  const isToday = (day: number) =>
    year === new Date().getFullYear() &&
    monthIndex === new Date().getMonth() &&
    day === new Date().getDate();

  return (
    <>
      <div className="cc-cal-head">
        <button
          type="button"
          className="cc-cal-nav cc-hit"
          aria-label="Previous month"
          onClick={() => shift(-1)}
        >
          <ChevronLeft size={15} />
        </button>
        <span className="cc-cal-title">{monthLabel}</span>
        <button
          type="button"
          className="cc-cal-nav cc-hit"
          aria-label="Next month"
          onClick={() => shift(1)}
        >
          <ChevronRight size={15} />
        </button>
      </div>
      <div className="cc-cal-weekdays" aria-hidden="true">
        {WEEKDAY_LETTERS.map((letter, index) => (
          <span key={index}>{letter}</span>
        ))}
      </div>
      <div className="cc-cal-days">
        {cells.map((day, index) =>
          day === null ? (
            <span key={index} />
          ) : (
            <span
              key={index}
              className="cc-cal-day"
              data-today={isToday(day) ? "true" : undefined}
            >
              {day}
            </span>
          ),
        )}
      </div>
    </>
  );
}
