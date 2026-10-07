import type { ReactNode } from "react";

type Props = {
  title: string;
  description?: string;
  children?: ReactNode;
};

export function PageHeader({ title, description, children }: Props) {
  return (
    <header
      data-tauri-drag-region
      className="sticky top-0 z-10 flex items-start justify-between gap-4 border-b border-line bg-canvas px-6 py-4"
    >
      <div data-tauri-drag-region>
        <h1 data-tauri-drag-region className="text-xl font-semibold">
          {title}
        </h1>
        {description && (
          <p data-tauri-drag-region className="mt-1 text-[13px] text-ink-muted">
            {description}
          </p>
        )}
      </div>
      {children && <div className="flex items-center gap-2">{children}</div>}
    </header>
  );
}
