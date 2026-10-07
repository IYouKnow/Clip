import { Scissors } from "lucide-react";
import { NAV, type View } from "../lib/nav";
import type { Theme } from "../lib/theme";
import { NavItem } from "./NavItem";
import { StatusPill } from "./StatusPill";
import { ThemeToggle } from "./ThemeToggle";
import { WindowControls } from "./WindowControls";

type Props = {
  view: View;
  onSelect: (view: View) => void;
  theme: Theme;
  onThemeChange: (theme: Theme) => void;
  replaying: boolean;
};

export function Sidebar({ view, onSelect, theme, onThemeChange, replaying }: Props) {
  return (
    <aside className="flex w-[220px] shrink-0 flex-col border-r border-line bg-sidebar">
      <div
        data-tauri-drag-region
        className="flex h-12 items-center justify-between gap-2 pl-3 pr-1.5"
      >
        <div data-tauri-drag-region className="flex items-center gap-2">
          <span
            data-tauri-drag-region
            className="grid size-5 place-items-center rounded-[6px] bg-accent text-accent-ink"
          >
            <Scissors className="size-3" />
          </span>
          <span
            data-tauri-drag-region
            className="text-[13px] font-semibold tracking-tight"
          >
            Clipper23
          </span>
        </div>
        <WindowControls />
      </div>

      <nav aria-label="Main" className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-2">
        {NAV.map((entry) => (
          <NavItem
            key={entry.id}
            entry={entry}
            active={view === entry.id}
            onSelect={() => onSelect(entry.id)}
          />
        ))}
      </nav>

      <div className="flex flex-col gap-2 border-t border-line p-2">
        <StatusPill replaying={replaying} />
        <ThemeToggle theme={theme} onChange={onThemeChange} />
      </div>
    </aside>
  );
}
