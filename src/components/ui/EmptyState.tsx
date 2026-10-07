import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

type Props = {
  icon?: LucideIcon;
  title: string;
  description?: string;
  action?: ReactNode;
};

export function EmptyState({ icon: Icon, title, description, action }: Props) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 rounded-[var(--radius-control)] border border-dashed border-line px-6 py-12 text-center">
      {Icon && <Icon className="size-5 text-ink-faint" />}
      <p className="text-sm font-medium text-ink">{title}</p>
      {description && <p className="max-w-sm text-[13px] text-ink-muted">{description}</p>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}
