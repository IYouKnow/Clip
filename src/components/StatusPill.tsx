import { cx } from "../lib/cx";

type Props = {
  replaying: boolean;
  collapsed: boolean;
};

export function StatusPill({ replaying, collapsed }: Props) {
  const label = replaying ? "Recording" : "Idle";
  const dot = (
    <span
      aria-hidden="true"
      className={cx(
        "size-2 shrink-0 rounded-full",
        replaying
          ? "bg-rec animate-[rec-pulse_1.6s_ease-in-out_infinite]"
          : "bg-ink-faint",
      )}
    />
  );

  if (collapsed) {
    return (
      <div
        role="status"
        title={label}
        className="grid h-9 place-items-center rounded-[var(--radius-control)] border border-line"
      >
        {dot}
        <span className="sr-only">{label}</span>
      </div>
    );
  }

  return (
    <div
      role="status"
      className="flex items-center gap-2 rounded-[var(--radius-control)] border border-line px-2.5 py-2 text-[12px] text-ink-muted"
    >
      {dot}
      {label}
    </div>
  );
}
