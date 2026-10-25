import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getSettings, type GlassbarSettings } from "../../../lib/shell";
import { getWeather, weatherIcon, weatherLabel, type Weather } from "../../../lib/weather";

/** The weather gadget's micro settings: the display temperature unit. */
type Unit = NonNullable<GlassbarSettings["gadgetSettings"]["weather"]>["unit"];

const toDisplay = (fahrenheit: number, unit: Unit): number =>
  unit === "celsius" ? ((fahrenheit - 32) * 5) / 9 : fahrenheit;

/** Weather gadget: current conditions for the machine's location. */
export default function WeatherGadget() {
  // Weather refreshes every 30 minutes; the refresh cycle for shell state
  // (2s) is far too frequent for a forecast endpoint.
  const [weather, setWeather] = useState<Weather | null>(null);
  const [unit, setUnit] = useState<Unit>("fahrenheit");
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const next = await getWeather();
      if (!cancelled && next) {
        setWeather(next);
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 30 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);
  // The temperature is always fetched in fahrenheit and converted for
  // display, so a unit toggle takes effect instantly (no refetch wait).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void getSettings().then((loaded) => {
      if (loaded?.gadgetSettings?.weather?.unit) {
        setUnit(loaded.gadgetSettings.weather.unit);
      }
    });
    void listen<GlassbarSettings>("settings-changed", (event) => {
      const unit = event.payload?.gadgetSettings?.weather?.unit;
      if (unit) {
        setUnit(unit);
      }
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const Icon = weatherIcon(weather?.code);
  const label = weather
    ? weather.city
      ? `${weather.city} — ${weatherLabel(weather.code)}`
      : weatherLabel(weather.code)
    : "Weather";

  return (
    <>
      <Icon size={18} strokeWidth={2} />
      <span className="taskbar-gadget-text" title={label}>
        {weather
          ? `${Math.round(toDisplay(weather.temperature, unit))}°${unit === "celsius" ? "C" : "F"}`
          : "—"}
      </span>
    </>
  );
}
