import { Fragment, useEffect, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import {
  Droplets,
  FolderOpen,
  LayoutGrid,
  PanelBottom,
  Puzzle,
  RotateCcw,
  RotateCw,
} from "lucide-react";
import {
  getSettings,
  openGadgetsFolder,
  resetSettings,
  setSettings,
  type GlassbarSettings,
} from "../../lib/shell";
import { useGadgets } from "../gadgets/registry";
import { ControlCenterPane } from "./ControlCenterPane";
import { Sidebar, type SettingsPane } from "./Sidebar";
import { SettingsGroup, OptionRow } from "./OptionRow";
import { SliderRow } from "./SliderRow";
import { ToggleSwitch } from "./ToggleSwitch";
import { RadioGroup, RadioOption } from "./RadioOption";

const PANES: SettingsPane[] = [
  { id: "taskbar", label: "Taskbar", icon: PanelBottom },
  { id: "control-center", label: "Control Center", icon: LayoutGrid },
  { id: "gadgets", label: "Gadgets", icon: Puzzle },
  { id: "liquid-glass", label: "Liquid Glass", icon: Droplets },
];

const TINT_PRESETS: Array<{ name: string; color: [number, number, number] }> = [
  { name: "Red", color: [1.0, 0.25, 0.25] },
  { name: "Blue", color: [0.15, 0.5, 1.0] },
  { name: "Purple", color: [0.65, 0.35, 0.95] },
  { name: "Pink", color: [1.0, 0.4, 0.7] },
  { name: "Orange", color: [1.0, 0.55, 0.15] },
  { name: "Green", color: [0.3, 0.85, 0.4] },
  { name: "Graphite", color: [0.65, 0.65, 0.7] },
];

const clamp01 = (channel: number) => Math.min(Math.max(channel, 0), 1);

function tintToHex(tint: [number, number, number]): string {
  return `#${tint
    .map((channel) => Math.round(clamp01(channel) * 255).toString(16).padStart(2, "0"))
    .join("")}`;
}

function hexToTint(hex: string): [number, number, number] {
  return [0, 2, 4].map(
    (offset) => Number.parseInt(hex.slice(1 + offset, 3 + offset), 16) / 255,
  ) as [number, number, number];
}

const sameTint = (a: [number, number, number], b: [number, number, number]) =>
  a.every((channel, index) => Math.abs(channel - b[index]) < 0.01);

export function SettingsWindow() {
  const [pane, setPane] = useState("taskbar");
  const [settings, setLocal] = useState<GlassbarSettings | null>(null);
  const gadgets = useGadgets();

  useEffect(() => {
    void getSettings().then((loaded) => {
      if (loaded) {
        setLocal(loaded);
      }
    });
  }, []);

  const update = (patch: Partial<GlassbarSettings>) => {
    setLocal((current) => {
      if (!current) {
        return current;
      }
      void setSettings(patch);
      return { ...current, ...patch };
    });
  };

  /** Patches one gadget's micro settings inside the per-gadget map. */
  const updateGadgetSettings = (id: string, patch: Record<string, unknown>) => {
    setLocal((current) => {
      if (!current) {
        return current;
      }
      const gadgetSettings = {
        ...current.gadgetSettings,
        [id]: { ...current.gadgetSettings[id], ...patch },
      };
      void setSettings({ gadgetSettings });
      return { ...current, gadgetSettings };
    });
  };

  const reset = async () => {
    const defaults = await resetSettings();
    if (defaults) {
      setLocal(defaults);
    }
  };

  // Re-runs the custom gadget loader in every window (including this one —
  // useGadgets listens for the same event), so dropped-in files appear
  // without restarting the app.
  const reloadGadgets = () => {
    void emit("custom-gadgets-changed");
  };

  if (!settings) {
    return <div className="settings-window">Loading…</div>;
  }

  const activePane = PANES.find((candidate) => candidate.id === pane) ?? PANES[0];

  return (
    <div className="settings-window">
      <Sidebar panes={PANES} activeId={activePane.id} onSelect={setPane} />
      <main className="settings-content">
        <h1 className="settings-content-title">{activePane.label}</h1>

        {activePane.id === "taskbar" && (
          <>
            <SettingsGroup title="General">
              <OptionRow label="Open on startup">
                <ToggleSwitch
                  checked={settings.launchOnStartup}
                  onChange={(launchOnStartup) => update({ launchOnStartup })}
                  label="Open on startup"
                />
              </OptionRow>
            </SettingsGroup>
            <SettingsGroup title="Layout">
              <OptionRow label="Reserve screen space">
                <ToggleSwitch
                  checked={settings.reserveSpace}
                  onChange={(reserveSpace) => update({ reserveSpace })}
                  label="Reserve screen space"
                />
              </OptionRow>
              <OptionRow label="Pinned apps on the left">
                <ToggleSwitch
                  checked={settings.pinnedSideLeft}
                  onChange={(pinnedSideLeft) => update({ pinnedSideLeft })}
                  label="Pinned apps on the left"
                />
              </OptionRow>
              <OptionRow label="Hide Windows taskbar">
                <ToggleSwitch
                  checked={settings.hideTaskbar}
                  onChange={(hideTaskbar) => update({ hideTaskbar })}
                  label="Hide Windows taskbar"
                />
              </OptionRow>
              <OptionRow label="Fully hide when not hovered">
                <ToggleSwitch
                  checked={settings.hideWhenUnhovered}
                  onChange={(hideWhenUnhovered) => update({ hideWhenUnhovered })}
                  label="Fully hide when not hovered"
                />
              </OptionRow>
              <OptionRow label="Reveal under fullscreen apps">
                <ToggleSwitch
                  checked={settings.revealInFullscreen}
                  onChange={(revealInFullscreen) => update({ revealInFullscreen })}
                  label="Reveal under fullscreen apps"
                />
              </OptionRow>
              <SliderRow
                label="Corner radius"
                value={settings.pillRadius}
                min={0}
                max={34}
                step={1}
                format={(value) => `${Math.round(value)} px`}
                onChange={(pillRadius) => update({ pillRadius })}
              />
              <SliderRow
                label="Icon size"
                value={settings.iconSize}
                min={24}
                max={72}
                step={1}
                format={(value) => `${Math.round(value)} px`}
                onChange={(iconSize) => update({ iconSize })}
              />
              <SliderRow
                label="Icon gap"
                value={settings.iconGap}
                min={0}
                max={24}
                step={1}
                format={(value) => `${Math.round(value)} px`}
                onChange={(iconGap) => update({ iconGap })}
              />
            </SettingsGroup>

            <SettingsGroup title="Resize animation">
              <SliderRow
                label="Squash & stretch"
                value={settings.resizeSquash}
                min={0}
                max={1}
                step={0.05}
                onChange={(resizeSquash) => update({ resizeSquash })}
              />
              <SliderRow
                label="Bounciness"
                value={settings.resizeBounce}
                min={0}
                max={1}
                step={0.05}
                onChange={(resizeBounce) => update({ resizeBounce })}
              />
              <SliderRow
                label="Duration"
                value={settings.resizeDuration}
                min={80}
                max={600}
                step={10}
                format={(value) => `${Math.round(value)} ms`}
                onChange={(resizeDuration) => update({ resizeDuration })}
              />
            </SettingsGroup>

            {!settings.reserveSpace && (
              <SettingsGroup title="Reveal strip">
                <SliderRow
                  label="Height"
                  value={settings.hoverStripHeight}
                  min={8}
                  max={200}
                  step={2}
                  format={(value) => `${Math.round(value)} px`}
                  onChange={(hoverStripHeight) =>
                    update({
                      hoverStripHeight,
                      hoverStripHoverHeight: Math.max(
                        settings.hoverStripHoverHeight,
                        hoverStripHeight,
                      ),
                    })
                  }
                />
                <SliderRow
                  label="Height while hovered"
                  value={settings.hoverStripHoverHeight}
                  min={8}
                  max={300}
                  step={2}
                  format={(value) => `${Math.round(value)} px`}
                  onChange={(hoverStripHoverHeight) =>
                    update({
                      hoverStripHoverHeight: Math.max(
                        hoverStripHoverHeight,
                        settings.hoverStripHeight,
                      ),
                    })
                  }
                />
              </SettingsGroup>
            )}
          </>
        )}

        {activePane.id === "control-center" && (
          <ControlCenterPane settings={settings} update={update} />
        )}

        {activePane.id === "gadgets" && (
          <>
            <SettingsGroup title="Taskbar gadgets">
              {gadgets.map((gadget) => (
                <Fragment key={gadget.id}>
                  <OptionRow
                    label={
                      <>
                        {gadget.name}
                        {gadget.custom && (
                          <span className="settings-gadget-badge">Custom</span>
                        )}
                      </>
                    }
                  >
                    <ToggleSwitch
                      checked={settings.enabledGadgets.includes(gadget.id)}
                      onChange={(checked) =>
                        update({
                          enabledGadgets: checked
                            ? [...settings.enabledGadgets, gadget.id]
                            : settings.enabledGadgets.filter((id) => id !== gadget.id),
                        })
                      }
                      label={`Enable ${gadget.name}`}
                    />
                  </OptionRow>
                  {gadget.id === "weather" &&
                    settings.enabledGadgets.includes("weather") && (
                      <OptionRow
                        className="settings-row-micro"
                        label="Temperature unit"
                      >
                        <div
                          className="settings-segmented"
                          role="radiogroup"
                          aria-label="Temperature unit"
                        >
                          {(["fahrenheit", "celsius"] as const).map((unit) => (
                            <button
                              key={unit}
                              type="button"
                              className="settings-segmented-option"
                              data-checked={
                                (settings.gadgetSettings.weather?.unit ??
                                  "fahrenheit") === unit
                              }
                              onClick={() => updateGadgetSettings("weather", { unit })}
                            >
                              {unit === "fahrenheit" ? "°F" : "°C"}
                            </button>
                          ))}
                        </div>
                      </OptionRow>
                    )}
                </Fragment>
              ))}
            </SettingsGroup>
            <SettingsGroup title="Custom gadgets">
              <OptionRow label="Gadget sources (.tsx / .jsx)">
                <div className="settings-button-row">
                  <button
                    type="button"
                    className="settings-mini-button"
                    onClick={() => void openGadgetsFolder()}
                  >
                    <FolderOpen size={13} strokeWidth={2} />
                    Open folder
                  </button>
                  <button
                    type="button"
                    className="settings-mini-button"
                    onClick={reloadGadgets}
                  >
                    <RotateCw size={13} strokeWidth={2} />
                    Reload
                  </button>
                </div>
              </OptionRow>
            </SettingsGroup>
            <p className="settings-pane-note">
              Drop .tsx files into the folder that opens above (the same folder
              as settings.json) and press Reload — they are transpiled and
              loaded at runtime, no rebuild needed. See
              src/features/gadgets/customGadget.example.tsx for the contract.
              Broken files show a warning gadget instead of failing silently.
            </p>
          </>
        )}

        {activePane.id === "liquid-glass" && (
          <>
            <SettingsGroup title="Liquid Glass">
              <SliderRow
                label="Refraction strength"
                value={settings.refractionStrength}
                min={0}
                max={30}
                step={0.5}
                onChange={(refractionStrength) => update({ refractionStrength })}
              />
              <SliderRow
                label="Edge band"
                value={settings.edgeBand}
                min={0}
                max={0.6}
                step={0.01}
                onChange={(edgeBand) => update({ edgeBand })}
              />
              <SliderRow
                label="Blur radius"
                value={settings.blurRadius}
                min={0}
                max={60}
                step={0.5}
                onChange={(blurRadius) => update({ blurRadius })}
              />
              <SliderRow
                label="Blur samples"
                value={settings.blurSamples}
                min={1}
                max={4}
                step={1}
                format={(value) => `${Math.round(value)} levels`}
                onChange={(blurSamples) => update({ blurSamples })}
              />
              <SliderRow
                label="Tint strength"
                value={settings.tintStrength}
                min={0}
                max={1}
                step={0.01}
                onChange={(tintStrength) => update({ tintStrength })}
              />
              <OptionRow label="Tint color">
                <RadioGroup>
                  {TINT_PRESETS.map((preset) => (
                    <RadioOption
                      key={preset.name}
                      title={preset.name}
                      color={tintToHex(preset.color)}
                      checked={sameTint(settings.tintColor, preset.color)}
                      onSelect={() => update({ tintColor: preset.color })}
                    />
                  ))}
                  <input
                    type="color"
                    className="settings-color-input"
                    aria-label="Custom tint color"
                    value={tintToHex(settings.tintColor)}
                    onChange={(event) => update({ tintColor: hexToTint(event.currentTarget.value) })}
                  />
                </RadioGroup>
              </OptionRow>
              <SliderRow
                label="Chromatic offset"
                value={settings.chromaticOffset}
                min={0}
                max={4}
                step={0.05}
                onChange={(chromaticOffset) => update({ chromaticOffset })}
              />
              <SliderRow
                label="Chromatic blend"
                value={settings.chromaticBlend}
                min={0}
                max={3}
                step={0.05}
                onChange={(chromaticBlend) => update({ chromaticBlend })}
              />
            </SettingsGroup>

            <SettingsGroup title="Outline">
              <SliderRow
                label="Strength"
                value={settings.outlineStrength}
                min={0}
                max={4}
                step={0.05}
                onChange={(outlineStrength) => update({ outlineStrength })}
              />
              <SliderRow
                label="Thickness"
                value={settings.outlineThickness}
                min={0}
                max={8}
                step={0.5}
                format={(value) => `${value.toFixed(1)} px`}
                onChange={(outlineThickness) => update({ outlineThickness })}
              />
              <SliderRow
                label="Light angle"
                value={settings.outlineAngle}
                min={0}
                max={360}
                step={5}
                format={(value) => `${Math.round(value)}°`}
                onChange={(outlineAngle) => update({ outlineAngle })}
              />
              <SliderRow
                label="Mirror"
                value={settings.outlineMirror}
                min={0}
                max={1}
                step={0.05}
                onChange={(outlineMirror) => update({ outlineMirror })}
              />
              <SliderRow
                label="Fade distance"
                value={settings.outlineFade}
                min={0}
                max={600}
                step={10}
                format={(value) => (value < 1 ? "auto" : `${Math.round(value)} px`)}
                onChange={(outlineFade) => update({ outlineFade })}
              />
            </SettingsGroup>

            <SettingsGroup title="Black border">
              <SliderRow
                label="Thickness"
                value={settings.borderThickness}
                min={0}
                max={8}
                step={0.5}
                format={(value) => (value < 0.25 ? "off" : `${value.toFixed(1)} px`)}
                onChange={(borderThickness) => update({ borderThickness })}
              />
              <SliderRow
                label="Opacity"
                value={settings.borderOpacity}
                min={0}
                max={1}
                step={0.05}
                onChange={(borderOpacity) => update({ borderOpacity })}
              />
            </SettingsGroup>

            <SettingsGroup title="Reveal animation">
              <SliderRow
                label="Turbulence strength"
                value={settings.turbulenceStrength}
                min={0}
                max={24}
                step={0.5}
                onChange={(turbulenceStrength) => update({ turbulenceStrength })}
              />
              <SliderRow
                label="Turbulence scale"
                value={settings.turbulenceScale}
                min={0.5}
                max={10}
                step={0.1}
                onChange={(turbulenceScale) => update({ turbulenceScale })}
              />
              <SliderRow
                label="Wobble"
                value={settings.wobble}
                min={0}
                max={2}
                step={0.05}
                onChange={(wobble) => update({ wobble })}
              />
            </SettingsGroup>
          </>
        )}

        <div className="settings-footer">
          <button
            type="button"
            className="settings-reset-button"
            onClick={() => void reset()}
          >
            <RotateCcw size={14} />
            Reset all settings
          </button>
        </div>
      </main>
    </div>
  );
}