import type { ReactNode } from "react";

type Props = {
  label: string;
  value: string;
  onChange: (value: string) => void;
  children: ReactNode;
};

export function Select({ label, value, onChange, children }: Props) {
  return (
    <label className="flex flex-col gap-2">
      <span className="text-[13px] text-ink-muted">{label}</span>
      <select
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="h-9 cursor-pointer rounded-[var(--radius-control)] border border-line bg-elevated px-2.5 text-sm text-ink"
      >
        {children}
      </select>
    </label>
  );
}
