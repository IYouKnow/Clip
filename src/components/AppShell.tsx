import type { ReactNode } from "react";

type Props = {
  sidebar: ReactNode;
  bar?: ReactNode;
  children: ReactNode;
};

export function AppShell({ sidebar, bar, children }: Props) {
  return (
    <div className="flex h-full overflow-hidden bg-canvas text-ink">
      {sidebar}
      <div className="flex min-w-0 flex-1 flex-col">
        {bar}
        <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">{children}</main>
      </div>
    </div>
  );
}
