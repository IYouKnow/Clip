import { Scissors } from "lucide-react";
import { WindowControls } from "./WindowControls";

export function TitleBar() {
  return (
    <header className="flex h-10 shrink-0 items-center justify-between border-b border-line bg-sidebar pl-3 pr-1.5">
      <div
        data-tauri-drag-region
        className="flex flex-1 items-center gap-2 self-stretch"
      >
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
    </header>
  );
}
