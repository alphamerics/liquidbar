import type { ReactNode } from "react";
import { GlassCard } from "glass-ui";
import { ViewHeader } from "./ViewHeader";

/**
 * A sub-view's shared shell: the header (back to the main panel plus the
 * title) above one glass container holding the content. Every view revealed
 * by a chevron (Wi-Fi, Bluetooth, Sound, tray icons, Focus) renders through
 * this, so the header and the container always appear together.
 */
export function Subview({
  title,
  onBack,
  contentClassName = "cc-netlist",
  headerClassName,
  children,
}: {
  title: string;
  onBack: () => void;
  /** The glass container's layout class (list, sound card, …). */
  contentClassName?: string;
  /** Extra class for the header row (e.g. to match a shaped container). */
  headerClassName?: string;
  children: ReactNode;
}) {
  return (
    <>
      <ViewHeader title={title} onBack={onBack} className={headerClassName} />
      <GlassCard className={contentClassName}>{children}</GlassCard>
    </>
  );
}
