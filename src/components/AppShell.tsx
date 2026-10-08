import type { ReactNode } from "react";
import { TitleBar } from "./TitleBar";

type Props = {
  sidebar: ReactNode;
  children: ReactNode;
};

export function AppShell({ sidebar, children }: Props) {
  return (
    <div className="flex h-full flex-col overflow-hidden bg-canvas text-ink">
      <TitleBar />
      <div className="flex min-h-0 flex-1">
        {sidebar}
        <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">{children}</main>
      </div>
    </div>
  );
}
