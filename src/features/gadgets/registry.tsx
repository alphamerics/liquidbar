import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import * as React from "react";
import { TriangleAlert } from "lucide-react";
import {
  listCustomGadgetFiles,
  readCustomGadget,
  type CustomGadgetFile,
} from "../../lib/shell";
import type { GadgetComponent, GadgetDefinition } from "./types";
import ClockGadget from "./builtins/ClockGadget";
import KeyboardGadget from "./builtins/KeyboardGadget";
import WeatherGadget from "./builtins/WeatherGadget";

export const BUILTIN_GADGETS: GadgetDefinition[] = [
  { id: "weather", name: "Weather", Component: WeatherGadget, custom: false },
  { id: "clock", name: "Clock", Component: ClockGadget, custom: false },
  { id: "keyboard", name: "Keyboard", Component: KeyboardGadget, custom: false },
];

/** "DateGadget" / "my_gadget" -> "Date Gadget" / "My Gadget". */
function titleize(filename: string): string {
  return filename
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[-_]+/g, " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

/** What a custom gadget module may export. */
interface CustomGadgetExports {
  default?: GadgetComponent;
  gadget?: { name?: string };
}

let babelPromise: Promise<typeof import("@babel/standalone")> | null = null;
function loadBabel(): Promise<typeof import("@babel/standalone")> {
  // Babel (~800 KB gzipped) is code-split and only fetched when a custom
  // gadget actually exists.
  babelPromise ??= import("@babel/standalone");
  return babelPromise;
}

let iconsPromise: Promise<unknown> | null = null;
function loadIcons(): Promise<unknown> {
  // The full lucide icon set, lazily loaded for the same reason.
  iconsPromise ??= import("lucide-react") as Promise<unknown>;
  return iconsPromise;
}

/**
 * Transpiles a custom gadget's TSX with Babel and evaluates it as CommonJS,
 * so `export default` and `export const gadget` work naturally. The module
 * gets a `require` limited to an allowlist: React, the lucide icon set, and
 * the Tauri invoke/listen APIs. Gadgets share the app's React instance.
 */
async function evaluateCustomGadget(
  file: CustomGadgetFile,
  source: string,
): Promise<GadgetDefinition> {
  const [Babel, icons] = await Promise.all([loadBabel(), loadIcons()]);
  const transformed = Babel.transform(source, {
    filename: file.name,
    presets: [
      ["react", { runtime: "classic" }],
      ["typescript", { isTSX: true, allExtensions: true }],
    ],
    plugins: ["transform-modules-commonjs"],
  });
  const { code } = transformed;
  if (!code) {
    throw new Error("transpiling produced no code");
  }
  const moduleObject = { exports: {} as Record<string, unknown> };
  const requireModule = (specifier: string): unknown => {
    switch (specifier) {
      case "react":
        return React;
      case "lucide-react":
        return icons;
      case "@tauri-apps/api/core":
        return { invoke };
      case "@tauri-apps/api/event":
        return { listen };
      default:
        throw new Error(
          `"${specifier}" is not available to custom gadgets (allowed: react, lucide-react, @tauri-apps/api/core, @tauri-apps/api/event)`,
        );
    }
  };
  new Function("require", "module", "exports", code)(
    requireModule,
    moduleObject,
    moduleObject.exports,
  );
  const exports = moduleObject.exports as CustomGadgetExports;
  if (typeof exports.default !== "function") {
    throw new Error("must default-export a React component");
  }
  return {
    id: file.id,
    name: exports.gadget?.name ?? titleize(file.name),
    Component: exports.default,
    custom: true,
  };
}

/** A stand-in gadget shown on the taskbar while its source fails to load. */
function errorGadget(id: string, error: unknown): GadgetDefinition {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`Custom gadget "${id}" failed to load:`, error);
  return {
    id,
    name: titleize(id),
    Component: () => (
      <>
        <TriangleAlert size={18} strokeWidth={2} />
        <span
          className="taskbar-gadget-text"
          title={`Custom gadget "${id}" failed to load: ${message}`}
        >
          {id}
        </span>
      </>
    ),
    custom: true,
  };
}

/** Loads every custom gadget from the app data dir; broken ones error out. */
export async function loadCustomGadgets(): Promise<GadgetDefinition[]> {
  const files = await listCustomGadgetFiles();
  return Promise.all(
    files.map(async (file) => {
      try {
        const source = await readCustomGadget(file.path);
        if (source === null) {
          throw new Error("the file could not be read");
        }
        return await evaluateCustomGadget(file, source);
      } catch (error) {
        return errorGadget(file.id, error);
      }
    }),
  );
}

/**
 * Every available gadget: built-ins plus the custom ones from the app data
 * dir, reloaded whenever anything emits `custom-gadgets-changed` (the
 * settings window's Reload button does exactly that).
 */
export function useGadgets(): GadgetDefinition[] {
  const [gadgets, setGadgets] = useState<GadgetDefinition[]>(BUILTIN_GADGETS);
  const reload = useCallback(async () => {
    setGadgets([...BUILTIN_GADGETS, ...(await loadCustomGadgets())]);
  }, []);
  useEffect(() => {
    void reload();
    let unlisten: (() => void) | undefined;
    void listen("custom-gadgets-changed", () => {
      void reload();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [reload]);
  return gadgets;
}
