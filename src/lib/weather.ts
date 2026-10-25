import {
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudRain,
  CloudSnow,
  CloudSun,
  Sun,
  type LucideIcon,
} from "lucide-react";

export interface Weather {
  temperature: number;
  code: number;
  city: string | null;
}

/**
 * Fetches the current weather for the machine's approximate location:
 * IP geolocation (city-level) via GeoJS, then the current conditions from
 * Open-Meteo. Both are keyless and CORS-enabled. Returns null when offline
 * or on any failure, so the gadget can fall back to its placeholder.
 */
export async function getWeather(): Promise<Weather | null> {
  try {
    const geo = await fetch("https://get.geojs.io/v1/ip/geo.json").then((response) =>
      response.json(),
    );
    const latitude = Number.parseFloat(geo.latitude);
    const longitude = Number.parseFloat(geo.longitude);
    if (!Number.isFinite(latitude) || !Number.isFinite(longitude)) {
      return null;
    }
    const forecast = await fetch(
      `https://api.open-meteo.com/v1/forecast?latitude=${latitude}&longitude=${longitude}&current=temperature_2m,weather_code&temperature_unit=fahrenheit`,
    ).then((response) => response.json());
    const temperature = forecast?.current?.temperature_2m;
    const code = forecast?.current?.weather_code;
    if (typeof temperature !== "number" || typeof code !== "number") {
      return null;
    }
    return {
      temperature,
      code,
      city: typeof geo.city === "string" && geo.city ? geo.city : null,
    };
  } catch {
    return null;
  }
}

const ICON_BY_CODE: Array<[Set<number>, LucideIcon]> = [
  [new Set([0]), Sun],
  [new Set([1, 2]), CloudSun],
  [new Set([3]), Cloud],
  [new Set([45, 48]), CloudFog],
  [new Set([51, 53, 55, 56, 57]), CloudDrizzle],
  [new Set([61, 63, 65, 66, 67, 80, 81, 82]), CloudRain],
  [new Set([71, 73, 75, 77, 85, 86]), CloudSnow],
  [new Set([95, 96, 99]), CloudLightning],
];

export function weatherIcon(code: number | undefined): LucideIcon {
  if (typeof code !== "number") {
    return Cloud;
  }
  for (const [codes, icon] of ICON_BY_CODE) {
    if (codes.has(code)) {
      return icon;
    }
  }
  return Cloud;
}

const LABELS: Array<[Set<number>, string]> = [
  [new Set([0]), "Clear sky"],
  [new Set([1]), "Mostly clear"],
  [new Set([2]), "Partly cloudy"],
  [new Set([3]), "Overcast"],
  [new Set([45, 48]), "Foggy"],
  [new Set([51, 53, 55]), "Drizzle"],
  [new Set([56, 57]), "Freezing drizzle"],
  [new Set([61]), "Light rain"],
  [new Set([63]), "Rain"],
  [new Set([65, 66, 67]), "Heavy rain"],
  [new Set([80, 81, 82]), "Rain showers"],
  [new Set([71]), "Light snow"],
  [new Set([73, 85]), "Snow"],
  [new Set([75, 77, 86]), "Heavy snow"],
  [new Set([95]), "Thunderstorm"],
  [new Set([96, 99]), "Thunderstorm with hail"],
];

export function weatherLabel(code: number | undefined): string {
  if (typeof code !== "number") {
    return "Weather";
  }
  for (const [codes, label] of LABELS) {
    if (codes.has(code)) {
      return label;
    }
  }
  return "Weather";
}
