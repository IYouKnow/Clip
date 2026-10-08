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
};

export function Sidebar({ view, onSelect, theme, onThemeChange, replaying }: Props) {
  return (
    <aside className="flex w-[220px] shrink-0 flex-col border-r border-line bg-sidebar">
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
