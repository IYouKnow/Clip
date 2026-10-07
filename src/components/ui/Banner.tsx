import { CircleAlert, Info, TriangleAlert, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { cx } from "../../lib/cx";

type Tone = "info" | "warn" | "error";

const TONES: Record<Tone, { className: string; icon: LucideIcon }> = {
  info: { className: "border-line bg-elevated text-ink-muted", icon: Info },
  warn: { className: "border-line-strong bg-elevated text-ink", icon: TriangleAlert },
  error: { className: "border-rec/40 bg-rec/10 text-rec", icon: CircleAlert },
};

export function Banner({
  tone = "info",
  children,
}: {
  tone?: Tone;
  children: ReactNode;
}) {
  const { className, icon: Icon } = TONES[tone];
  return (
    <div
      role={tone === "error" ? "alert" : undefined}
      className={cx("flex items-start gap-2.5 rounded-[var(--radius-control)] border px-3.5 py-2.5 text-[13px]", className)}
    >
      <Icon className="mt-0.5 size-4 shrink-0" />
      <div className="leading-relaxed">{children}</div>
    </div>
  );
}
