import { cx } from "../lib/cx";

export function StatusPill({ replaying }: { replaying: boolean }) {
  return (
    <div
      role="status"
      className="flex items-center gap-2 rounded-[var(--radius-control)] border border-line px-2.5 py-2 text-[12px] text-ink-muted"
    >
      <span
        aria-hidden="true"
        className={cx(
          "size-2 shrink-0 rounded-full",
          replaying
            ? "bg-rec animate-[rec-pulse_1.6s_ease-in-out_infinite]"
            : "bg-ink-faint",
        )}
      />
      {replaying ? "Recording" : "Idle"}
    </div>
  );
}
