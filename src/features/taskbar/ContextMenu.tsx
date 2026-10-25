import * as ContextMenuPrimitive from "@radix-ui/react-context-menu";
import { useCallback, useId, useState } from "react";
import type { ReactNode } from "react";
import { LiquidMenuItems, type LiquidMenuItem } from "glass-ui";
import { useLiquidGlassSurface } from "glass-ui";

interface ContextMenuProps {
  children: ReactNode;
  onOpenChange?: (open: boolean) => void;
  onRefresh?: () => void;
  onExit?: () => void;
}

export function ContextMenu({ children, onOpenChange, onRefresh, onExit }: ContextMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const handleOpenChange = useCallback(
    (next: boolean) => {
      setOpen(next);
      onOpenChange?.(next);
    },
    [onOpenChange],
  );

  useLiquidGlassSurface(open, `[data-liquid-menu="${menuId}"]`, 18, menuId);

  const items: LiquidMenuItem[] = [
    { label: "Refresh shell", onSelect: onRefresh },
    { label: "", kind: "separator" },
    { label: "Exit Glassbar", onSelect: onExit },
  ];

  return (
    <ContextMenuPrimitive.Root onOpenChange={handleOpenChange}>
      <ContextMenuPrimitive.Trigger asChild>{children}</ContextMenuPrimitive.Trigger>
      <ContextMenuPrimitive.Portal>
        <ContextMenuPrimitive.Content
          className="liquid-menu-content"
          data-liquid-menu={menuId}
          style={{ width: 210, borderRadius: 18, padding: 5 }}
        >
          <LiquidMenuItems
            items={items}
            primitives={
              ContextMenuPrimitive as unknown as Parameters<typeof LiquidMenuItems>[0]["primitives"]
            }
          />
        </ContextMenuPrimitive.Content>
      </ContextMenuPrimitive.Portal>
    </ContextMenuPrimitive.Root>
  );
}