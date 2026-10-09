import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { cx } from "../lib/cx";
import { NAV, type View } from "../lib/nav";
import type { Theme } from "../lib/theme";
import { NavItem } from "./NavItem";
import { StatusPill } from "./StatusPill";
import { ThemeToggle } from "./ThemeToggle";

type Props = {
  view: View;
  onSelect: (view: View) => void;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  replaying: boolean;
  collapsed: boolean;
  onToggle: () => void;
};

export function Sidebar({
  view,
  onSelect,
  theme,
  onThemeChange,
  replaying,
  collapsed,
  onToggle,
}: Props) {
  return (
    <aside
      className={cx(
        "flex shrink-0 flex-col border-r border-line bg-sidebar transition-[width] duration-200",
        collapsed ? "w-16" : "w-[220px]",
      )}
    >
      <div
        className={cx("flex h-10 shrink-0 items-center gap-2", collapsed ? "px-2" : "px-3")}
      >
        <div data-tauri-drag-region className="flex flex-1 items-center self-stretch">
          {!collapsed && (
            <span className="text-sm font-bold uppercase tracking-[0.2em] text-ink">
              Trace
            </span>
          )}
        </div>

        <button
          type="button"
          onClick={onToggle}
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          aria-pressed={collapsed}
          className="grid size-8 shrink-0 cursor-pointer place-items-center rounded-[var(--radius-control)] text-ink-muted transition-colors duration-150 hover:bg-elevated hover:text-ink"
        >
          {collapsed ? (
            <PanelLeftOpen className="size-4" />
          ) : (
            <PanelLeftClose className="size-4" />
          )}
        </button>
      </div>

      <nav
        aria-label="Main"
        className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 pt-1 pb-2"
      >
        {NAV.map((entry) => (
          <NavItem
            key={entry.id}
            entry={entry}
            active={view === entry.id}
            collapsed={collapsed}
            onSelect={() => onSelect(entry.id)}
          />
        ))}
      </nav>

      <div className="flex flex-col gap-2 border-t border-line p-2">
        <StatusPill replaying={replaying} collapsed={collapsed} />
        <ThemeToggle theme={theme} collapsed={collapsed} onChange={onThemeChange} />
      </div>
    </aside>
  );
}
