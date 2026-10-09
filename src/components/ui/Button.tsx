import { LoaderCircle } from "lucide-react";
import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cx } from "../../lib/cx";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "xs" | "sm" | "md";

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  icon?: ReactNode;
};

const VARIANTS: Record<Variant, string> = {
  primary: "border border-transparent bg-accent text-accent-ink hover:opacity-90",
  secondary: "border border-line bg-transparent text-ink hover:bg-elevated",
  ghost:
    "border border-transparent bg-transparent text-ink-muted hover:bg-elevated hover:text-ink",
  danger: "border border-rec/40 bg-transparent text-rec hover:bg-rec/10",
};

const SIZES: Record<Size, string> = {
  xs: "h-6 gap-1 px-2 text-[12px]",
  sm: "h-8 gap-1.5 px-3 text-[13px]",
  md: "h-9 gap-2 px-4 text-sm",
};

export function Button({
  variant = "secondary",
  size = "md",
  loading = false,
  icon,
  className,
  children,
  disabled,
  type = "button",
  ...rest
}: Props) {
  return (
    <button
      type={type}
      disabled={disabled || loading}
      className={cx(
        "inline-flex cursor-pointer items-center justify-center rounded-[var(--radius-control)] font-medium transition-colors duration-150 disabled:pointer-events-none disabled:opacity-45",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...rest}
    >
      {loading ? <LoaderCircle className="size-4 animate-spin" /> : icon}
      {children}
    </button>
  );
}
