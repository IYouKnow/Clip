import { Download, X } from "lucide-react";
import { useState } from "react";
import type { Updater } from "../lib/updater";
import { Button } from "./ui/Button";

type Props = {
  updater: Updater;
};

export function UpdateBar({ updater }: Props) {
  const [dismissed, setDismissed] = useState<string | null>(null);

  const { phase, version, progress, install } = updater;
  const visible = phase === "available" || phase === "downloading" || phase === "ready";
  if (!visible || (dismissed !== null && dismissed === version)) return null;

  const label =
    phase === "downloading"
      ? `Updating… ${progress ?? 0}%`
      : phase === "ready"
        ? "Restarting…"
        : "Update";

  return (
    <div
      data-tauri-drag-region="deep"
      className="flex h-8 shrink-0 items-center gap-2 border-b border-line bg-elevated px-2.5"
    >
      <Download className="size-3.5 shrink-0 text-accent" />
      <p className="min-w-0 truncate text-[12px] text-ink">
        <span className="font-medium">Trace {version}</span> is available.
      </p>
      <div className="ml-auto flex shrink-0 items-center gap-1">
        <Button
          variant="primary"
          size="xs"
          loading={phase === "downloading"}
          disabled={phase !== "available"}
          onClick={() => install()}
        >
          {label}
        </Button>
        <button
          type="button"
          aria-label="Dismiss update notice"
          title="Dismiss"
          onClick={() => setDismissed(version)}
          className="grid size-6 cursor-pointer place-items-center rounded-[var(--radius-control)] text-ink-faint transition-colors hover:bg-canvas hover:text-ink"
        >
          <X className="size-3.5" />
        </button>
      </div>
    </div>
  );
}
