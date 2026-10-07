import type { ReactNode } from "react";

type Props = {
  label: string;
  value: ReactNode;
  hint?: string;
};

export function StatTile({ label, value, hint }: Props) {
  return (
    <div className="rounded-[var(--radius-control)] border border-line bg-canvas px-3 py-2.5">
      <div className="text-[11px] uppercase tracking-wider text-ink-faint">{label}</div>
      <div className="mt-1 text-lg font-semibold tabular-nums">{value}</div>
      {hint && <div className="mt-0.5 text-[12px] text-ink-muted">{hint}</div>}
    </div>
  );
}
