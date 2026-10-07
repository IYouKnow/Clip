import { Button } from "./Button";
import { Modal } from "./Modal";

type Props = {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  danger?: boolean;
};

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  danger = false,
}: Props) {
  return (
    <Modal title={title} onClose={onCancel}>
      <h2 className="text-base font-semibold">{title}</h2>
      <p className="mt-2 text-[13px] leading-relaxed text-ink-muted">{message}</p>
      <div className="mt-5 flex justify-end gap-2">
        <Button variant="secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant={danger ? "danger" : "primary"} autoFocus onClick={onConfirm}>
          {confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}
