import { Search, X } from "lucide-react";
import { useState } from "react";
import type { LucideIcon } from "lucide-react";

export interface SettingsPane {
  id: string;
  label: string;
  icon: LucideIcon;
}

interface SidebarProps {
  panes: SettingsPane[];
  activeId: string;
  onSelect: (id: string) => void;
}

export function Sidebar({ panes, activeId, onSelect }: SidebarProps) {
  const [query, setQuery] = useState("");

  return (
    <aside className="settings-sidebar">
      <label className="settings-search">
        <Search size={13} strokeWidth={2.2} />
        <input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.currentTarget.value)}
          placeholder="Search"
          aria-label="Search settings"
        />
        {query.length > 0 && (
          <button
            type="button"
            className="settings-search-clear"
            aria-label="Clear search"
            onClick={() => setQuery("")}
          >
            <X size={12} strokeWidth={2.2} />
          </button>
        )}
      </label>
      <nav aria-label="Settings pages">
        {panes.map((pane) => {
          const Icon = pane.icon;
          return (
            <button
              key={pane.id}
              type="button"
              className="settings-sidebar-item"
              data-active={pane.id === activeId}
              onClick={() => onSelect(pane.id)}
            >
              <Icon size={15} strokeWidth={2} />
              <span>{pane.label}</span>
            </button>
          );
        })}
      </nav>
    </aside>
  );
}