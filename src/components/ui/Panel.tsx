import type { ReactNode } from "react";
import { cx } from "../../lib/cx";

type Props = {
  title?: string;
  action?: ReactNode;
  className?: string;
  bodyClassName?: string;
  children: ReactNode;
};

export function Panel({ title, action, className, bodyClassName, children }: Props) {
  return (
    <section
      className={cx(
        "rounded-[var(--radius-panel)] border border-line bg-surface",
        className,
      )}
    >
      {(title || action) && (
        <header className="flex items-center justify-between gap-3 border-b border-line px-4 py-3">
          {title && (
            <h2 className="text-[12px] font-semibold uppercase tracking-wider text-ink-muted">
              {title}
            </h2>
          )}
          {action}
        </header>
      )}
      <div className={cx("p-4", bodyClassName)}>{children}</div>
    </section>
  );
}
