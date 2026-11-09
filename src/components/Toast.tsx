import { Icon } from "./Icon";
import { t as tr } from "../lib/i18n";


export type ToastTone = "info" | "success" | "warning" | "error";

export interface ToastItem {
  id: string;
  message: string;
  detail?: string;
  tone: ToastTone;
}

const icons = {
  info: "info",
  success: "check",
  warning: "alert",
  error: "alert",
} as const;

export function ToastHost({
  items,
  onDismiss,
}: {
  items: ToastItem[];
  onDismiss: (id: string) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div className="toast-host" role="status" aria-live="polite">
      {items.map((t) => (
        <div key={t.id} className={`toast toast--${t.tone}`}>
          <Icon name={icons[t.tone]} size={17} />
          <div className="toast__content">
            <strong>{t.message}</strong>
            {t.detail ? <span>{t.detail}</span> : null}
          </div>
          <button className="toast__close" aria-label={tr("关闭提示")} onClick={() => onDismiss(t.id)}>
            <Icon name="close" size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}
