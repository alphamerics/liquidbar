import type { ComponentType } from "react";

/**
 * A taskbar gadget renders its own icon + text content inline. It is mounted
 * inside a `.taskbar-gadget` wrapper in the taskbar's left zone; all enabled
 * gadgets share the taskbar's single gadget shell (one pill, no dividers).
 */
export type GadgetComponent = ComponentType;

export interface GadgetDefinition {
  /** Stable settings id: built-ins use a fixed id, custom gadgets the file name. */
  id: string;
  /** Display name shown in the settings window. */
  name: string;
  Component: GadgetComponent;
  /** True when the gadget was discovered in the custom gadgets folder. */
  custom: boolean;
}
