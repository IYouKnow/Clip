import { cx } from "../lib/cx";
import type { NavEntry } from "../lib/nav";

type Props = {
  entry: NavEntry;
  active: boolean;
  onSelect: () => void;
};

export function NavItem({ entry, active, onSelect }: Props) {
  const Icon = entry.icon;
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      className={cx(
        "flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-[var(--radius-control)] px-2.5 text-sm transition-colors duration-150",
        active
          ? "bg-elevated font-medium text-ink"
          : "text-ink-muted hover:bg-elevated/60 hover:text-ink",
      )}
    >
      <Icon className="size-4 shrink-0" />
      {entry.label}
    </button>
  );
}
