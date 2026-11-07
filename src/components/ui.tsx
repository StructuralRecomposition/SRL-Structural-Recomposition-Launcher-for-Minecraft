import { useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon, type IconName } from "./Icon";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "md" | "sm";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: IconName;
  loading?: boolean;
  children?: ReactNode;
}

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  loading,
  children,
  className = "",
  disabled,
  ...rest
}: ButtonProps) {
  return (
    <button
      className={`btn btn--${variant} btn--${size} ${className}`}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      {...rest}
    >
      {loading ? <span className="spinner" aria-hidden="true" /> : icon ? <Icon name={icon} size={16} /> : null}
      {children ? <span>{children}</span> : null}
    </button>
  );
}

export function IconButton({
  icon,
  label,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string }) {
  return (
    <button
      type="button"
      className={`icon-btn ${className}`}
      aria-label={label}
      title={label}
      {...rest}
    >
      <Icon name={icon} size={17} />
    </button>
  );
}

export function Badge({
  children,
  tone = "neutral",
}: {
  children: ReactNode;
  tone?: "neutral" | "success" | "warning" | "danger" | "accent" | "info";
}) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function Card({
  children,
  className = "",
  as = "div",
}: {
  children: ReactNode;
  className?: string;
  as?: "div" | "section" | "article";
}) {
  const Tag = as;
  return <Tag className={`card ${className}`}>{children}</Tag>;
}

export function EmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: IconName;
  title: string;
  description: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty-state">
      <div className="empty-state__icon">
        <Icon name={icon} size={26} />
      </div>
      <h3>{title}</h3>
      <p>{description}</p>
      {action ? <div className="empty-state__action">{action}</div> : null}
    </div>
  );
}

export function ProgressBar({
  value,
  label,
  tone = "accent",
}: {
  value: number | null;
  label?: string;
  tone?: "accent" | "danger" | "warning";
}) {
  const indeterminate = value === null;
  return (
    <div className="progress-wrap">
      <div
        className={`progress ${indeterminate ? "progress--indeterminate" : ""} progress--${tone}`}
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={indeterminate ? undefined : Math.round(value)}
        aria-label={label ?? "进度"}
      >
        <div className="progress__bar" style={indeterminate ? undefined : { width: `${value}%` }} />
      </div>
      {label ? <span className="progress__label">{label}</span> : null}
    </div>
  );
}

export function InfoHint({ text }: { text: string }) {
  const ref = useRef<HTMLButtonElement>(null);
  const [tip, setTip] = useState<{ left: number; top: number; below: boolean } | null>(null);

  function show() {
    const rect = ref.current?.getBoundingClientRect();
    if (!rect) return;
    const below = rect.top < 96;
    const center = rect.left + rect.width / 2;
    const left = Math.min(Math.max(center, 140), window.innerWidth - 140);
    setTip({
      left,
      top: below ? rect.bottom + 8 : rect.top - 8,
      below,
    });
  }

  function hide() {
    setTip(null);
  }

  return (
    <>
      <button
        ref={ref}
        type="button"
        className="info-hint"
        aria-label={`说明：${text}`}
        title={text}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        ?
      </button>
      {tip
        ? createPortal(
            <div
              className={`info-hint__bubble ${tip.below ? "info-hint__bubble--below" : ""}`}
              style={{ left: tip.left, top: tip.top }}
              role="tooltip"
            >
              {text}
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

export function Field({
  label,
  info,
  error,
  children,
  htmlFor,
}: {
  label: string;
  info?: string;
  hint?: string;
  error?: string | null;
  children: ReactNode;
  htmlFor?: string;
}) {
  return (
    <div className={`field ${error ? "field--error" : ""}`}>
      <div className="field__label-row">
        <label className="field__label" htmlFor={htmlFor}>
          {label}
        </label>
        {info ? <InfoHint text={info} /> : null}
      </div>
      {children}
      {error ? <p className="field__error">{error}</p> : null}
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  description,
  id,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  description?: string;
  id: string;
}) {
  return (
    <div className="toggle-row">
      <div>
        <label className="toggle-row__label" htmlFor={id}>
          {label}
        </label>
        {description ? <p className="toggle-row__desc">{description}</p> : null}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        className={`toggle ${checked ? "toggle--on" : ""}`}
        onClick={() => onChange(!checked)}
      >
        <span className="toggle__thumb" />
      </button>
    </div>
  );
}

export function StatusDot({ status }: { status: "ok" | "warning" | "error" }) {
  return <span className={`status-dot status-dot--${status}`} aria-hidden="true" />;
}

export function Notice({
  tone = "info",
  title,
  children,
}: {
  tone?: "info" | "warning" | "danger" | "success";
  title?: string;
  children: ReactNode;
}) {
  const icon = tone === "danger" || tone === "warning" ? "alert" : tone === "success" ? "check" : "info";
  return (
    <div className={`notice ${tone === "info" ? "" : `notice--${tone}`}`.trim()}>
      <Icon name={icon} size={18} />
      <div>
        {title ? <strong>{title}</strong> : null}
        <p>{children}</p>
      </div>
    </div>
  );
}
