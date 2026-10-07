import { Copy, Minus, Square, X } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { appWindow, useMaximized } from "../lib/window";

type ControlProps = {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: ReactNode;
};

function Control({ label, onClick, danger, children }: ControlProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cx(
        "grid size-8 cursor-pointer place-items-center rounded-[6px] text-ink-muted transition-colors duration-150 hover:bg-elevated hover:text-ink",
        danger && "hover:bg-rec hover:text-white",
      )}
    >
      {children}
    </button>
  );
}

export function WindowControls() {
  const maximized = useMaximized();

  return (
    <div className="flex items-center gap-0.5">
      <Control label="Minimize window" onClick={() => appWindow?.minimize()}>
        <Minus className="size-3.5" />
      </Control>
      <Control
        label={maximized ? "Restore window" : "Maximize window"}
        onClick={() => appWindow?.toggleMaximize()}
      >
        {maximized ? <Copy className="size-3" /> : <Square className="size-3" />}
      </Control>
      <Control label="Close window" danger onClick={() => appWindow?.close()}>
        <X className="size-3.5" />
      </Control>
    </div>
  );
}
