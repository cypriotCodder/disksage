import { ReactNode } from "react";

interface Props {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ConfirmDialog({ title, children, confirmLabel, busy, onConfirm, onCancel }: Props) {
  return (
    <div className="modal-backdrop" onClick={busy ? undefined : onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()} role="dialog" aria-modal>
        <h2>{title}</h2>
        <div className="modal-body">{children}</div>
        <div className="modal-actions">
          <button className="btn ghost" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button className="btn danger" onClick={onConfirm} disabled={busy}>
            {busy ? "Working…" : confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
