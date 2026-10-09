import { Monitor, Moon, Sun, type LucideIcon } from "lucide-react";
import { cx } from "../lib/cx";
import type { Theme } from "../lib/theme";

type Props = {
  theme: Theme;
  collapsed?: boolean;
  onChange: (theme: Theme) => void;
};

const OPTIONS: { value: Theme; label: string; icon: LucideIcon }[] = [
  { value: "system", label: "System theme", icon: Monitor },
  { value: "light", label: "Light theme", icon: Sun },
  { value: "dark", label: "Dark theme", icon: Moon },
];

export function ThemeToggle({ theme, collapsed, onChange }: Props) {
  return (
    <div
      role="radiogroup"
      aria-label="Theme"
      className={cx(
        "flex items-center gap-0.5 rounded-[var(--radius-control)] border border-line bg-canvas p-0.5",
        collapsed ? "flex-col" : "flex-row",
      )}
    >
      {OPTIONS.map(({ value, label, icon: Icon }) => {
        const active = theme === value;
        return (
          <button
            key={value}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={label}
            title={label}
            onClick={() => onChange(value)}
            className={cx(
              "grid cursor-pointer place-items-center rounded-[6px] transition-colors duration-150",
              collapsed ? "size-7" : "h-7 flex-1",
              active ? "bg-elevated text-ink" : "text-ink-faint hover:text-ink",
            )}
          >
            <Icon className="size-3.5" />
          </button>
        );
      })}
    </div>
  );
}
