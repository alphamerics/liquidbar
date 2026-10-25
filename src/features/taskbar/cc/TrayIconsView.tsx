import { AppWindow } from "lucide-react";
import { type TrayIcon } from "../../../lib/shell";
import { Subview } from "./Subview";

/**
 * The custom tray UI, built from the notification-area icons enumerated
 * from Explorer's tray toolbars. Each row carries the icon's own image,
 * tooltip and owning application.
 *
 * Left-click forwards the equivalent tray activation message to the owning
 * application. Right-click sends the tray context-menu event to the
 * application's registered callback window, so the application displays its
 * own native menu — nothing is reconstructed or emulated here.
 */
export function TrayIconsView({
  icons,
  dispatching,
  onBack,
  onLeftClick,
  onContextMenu,
}: {
  icons: TrayIcon[] | null;
  dispatching: string | null;
  onBack: () => void;
  onLeftClick: (key: string) => void;
  onContextMenu: (key: string) => void;
}) {
  return (
    <Subview title="Tray icons" onBack={onBack}>
      {icons === null ? (
        <span className="cc-empty">Scanning…</span>
      ) : icons.length === 0 ? (
        <span className="cc-empty">No notification-area icons found</span>
      ) : (
        icons.map((icon) => (
          <TrayIconRow
            key={icon.key}
            icon={icon}
            dispatching={dispatching === icon.key}
            onLeftClick={onLeftClick}
            onContextMenu={onContextMenu}
          />
        ))
      )}
    </Subview>
  );
}

function TrayIconRow({
  icon,
  dispatching,
  onLeftClick,
  onContextMenu,
}: {
  icon: TrayIcon;
  dispatching: boolean;
  onLeftClick: (key: string) => void;
  onContextMenu: (key: string) => void;
}) {
  const name = icon.tooltip ?? icon.processName;
  return (
    <div
      className="cc-net-row cc-hit"
      title={icon.tooltip ?? icon.processName}
      onClick={() => {
        if (dispatching) {
          return;
        }
        onLeftClick(icon.key);
      }}
      onContextMenu={(event) => {
        event.preventDefault();
        if (dispatching) {
          return;
        }
        onContextMenu(icon.key);
      }}
    >
      <span className="cc-net-circle">
        {dispatching ? (
          <span className="cc-net-loader" />
        ) : icon.icon ? (
          <img src={icon.icon} alt="" className="cc-net-icon" draggable={false} />
        ) : (
          <AppWindow size={16} className="cc-net-icon" />
        )}
      </span>
      <span className="cc-net-name" title={name}>
        {name}
      </span>
      <span className="cc-net-process">{icon.processName}</span>
    </div>
  );
}
