import type { ReactNode } from "react";

type Props = {
  sidebar: ReactNode;
  children: ReactNode;
};

export function AppShell({ sidebar, children }: Props) {
  return (
    <div className="flex h-full overflow-hidden bg-canvas text-ink">
      {sidebar}
      <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">{children}</main>
    </div>
  );
}
