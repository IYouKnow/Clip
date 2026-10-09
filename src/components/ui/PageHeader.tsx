import type { ReactNode } from "react";
import { WindowControls } from "../WindowControls";

type Props = {
  title: string;
  description?: string;
  children?: ReactNode;
};

export function PageHeader({ title, description, children }: Props) {
  return (
    <header
      data-tauri-drag-region="deep"
      className="sticky top-0 z-10 flex items-start justify-between gap-4 border-b border-line bg-canvas px-6 py-4"
    >
      <div>
        <h1 className="text-xl font-semibold">{title}</h1>
        {description && (
          <p className="mt-1 text-[13px] text-ink-muted">{description}</p>
        )}
      </div>
      <div className="flex items-center gap-2">
        {children}
        <WindowControls />
      </div>
    </header>
  );
}
