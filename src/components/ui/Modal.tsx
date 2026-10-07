import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

type Props = {
  title: string;
  onClose: () => void;
  children: ReactNode;
};

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function Modal({ title, onClose, children }: Props) {
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const root = document.getElementById("root");
    const previouslyFocused = document.activeElement as HTMLElement | null;
    root?.setAttribute("inert", "");

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;

      const panel = panelRef.current;
      if (!panel) return;
      const focusable = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE));
      if (focusable.length === 0) return;

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    const panel = panelRef.current;
    if (panel && !panel.contains(document.activeElement)) {
      panel.focus();
    }

    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      root?.removeAttribute("inert");
      previouslyFocused?.focus();
    };
  }, [onClose]);

  return createPortal(
    <div
      className="fixed inset-0 z-50 grid place-items-center bg-black/70 p-6"
      onMouseDown={onClose}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        className="w-full max-w-3xl rounded-[var(--radius-panel)] border border-line bg-surface p-4 shadow-xl outline-none"
        onMouseDown={(event) => event.stopPropagation()}
      >
        {children}
      </div>
    </div>,
    document.body,
  );
}
