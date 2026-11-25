// Small building blocks on top of Radix primitives, styled in styles/ui.css.
import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Dialog as RDialog, Progress as RProgress, Select as RSelect, Switch as RSwitch, Tooltip as RTooltip } from "radix-ui";
import { CaretDown, Check, X } from "@phosphor-icons/react";

type Variant = "primary" | "secondary" | "ghost" | "danger" | "quiet";
type Size = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  icon?: ReactNode;
  trailing?: ReactNode;
  busy?: boolean;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", icon, trailing, busy, className = "", children, disabled, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      className={`btn btn--${variant} btn--${size} ${busy ? "is-busy" : ""} ${className}`}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      {...rest}
    >
      {busy ? <span className="spinner" aria-hidden /> : icon}
      {children && <span className="btn__label">{children}</span>}
      {trailing}
    </button>
  );
});

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  shortcut?: string;
  tone?: "default" | "danger" | "active";
  size?: "sm" | "md";
  side?: "top" | "bottom" | "left" | "right";
}

/** Icon-only button. Always has an accessible name and a tooltip. */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, shortcut, tone = "default", size = "md", side = "bottom", className = "", children, ...rest },
  ref,
) {
  return (
    <Tip label={label} shortcut={shortcut} side={side}>
      <button ref={ref} aria-label={label} className={`iconbtn iconbtn--${size} iconbtn--${tone} ${className}`} {...rest}>
        {children}
      </button>
    </Tip>
  );
});

export function Tip({
  label,
  shortcut,
  side = "bottom",
  children,
}: {
  label: ReactNode;
  shortcut?: string;
  side?: "top" | "bottom" | "left" | "right";
  children: ReactNode;
}) {
  return (
    <RTooltip.Root>
      <RTooltip.Trigger asChild>{children}</RTooltip.Trigger>
      <RTooltip.Portal>
        <RTooltip.Content className="tip" side={side} sideOffset={6} collisionPadding={8}>
          {label}
          {shortcut && <Kbd>{shortcut}</Kbd>}
        </RTooltip.Content>
      </RTooltip.Portal>
    </RTooltip.Root>
  );
}

const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform || navigator.userAgent);
export const MOD = isMac ? "⌘" : "Ctrl";

export function Kbd({ children }: { children: string }) {
  return <kbd className="kbd">{children.replaceAll("Mod", MOD)}</kbd>;
}

export function ProgressBar({ value, label, tone = "gold" }: { value: number | null; label?: string; tone?: "gold" | "teal" }) {
  const indeterminate = value === null;
  return (
    <RProgress.Root
      className={`progress progress--${tone} ${indeterminate ? "is-indeterminate" : ""}`}
      value={indeterminate ? null : Math.round(value * 100)}
      aria-label={label}
    >
      <RProgress.Indicator
        className="progress__bar"
        style={indeterminate ? undefined : { transform: `scaleX(${Math.max(0.004, Math.min(1, value))})` }}
      />
    </RProgress.Root>
  );
}

/** Three-step rating used for model speed / accuracy. */
export function Meter({ value, label }: { value: 1 | 2 | 3; label: string }) {
  return (
    <span className="meter" role="img" aria-label={`${label}: ${value} of 3`}>
      <span className="meter__label">{label}</span>
      <span className="meter__bars" aria-hidden>
        {[1, 2, 3].map((i) => (
          <span key={i} className={i <= value ? "is-on" : ""} />
        ))}
      </span>
    </span>
  );
}

export function Switch({ checked, onChange, label, id }: { checked: boolean; onChange: (v: boolean) => void; label: string; id: string }) {
  return (
    <RSwitch.Root id={id} className="switch" checked={checked} onCheckedChange={onChange} aria-label={label}>
      <RSwitch.Thumb className="switch__thumb" />
    </RSwitch.Root>
  );
}

export interface SelectOption {
  value: string;
  label: ReactNode;
  hint?: ReactNode;
}

export function Select({
  value,
  onChange,
  options,
  label,
  id,
}: {
  value: string;
  onChange: (v: string) => void;
  options: SelectOption[];
  label: string;
  id?: string;
}) {
  return (
    <RSelect.Root value={value} onValueChange={onChange}>
      <RSelect.Trigger id={id} className="field field--select" aria-label={label}>
        <RSelect.Value />
        <RSelect.Icon className="field__caret">
          <CaretDown size={14} />
        </RSelect.Icon>
      </RSelect.Trigger>
      <RSelect.Portal>
        <RSelect.Content className="menu" position="popper" sideOffset={6} collisionPadding={10}>
          <RSelect.Viewport className="menu__viewport">
            {options.map((o) => (
              <RSelect.Item key={o.value} value={o.value} className="menu__item">
                <span className="menu__check">
                  <RSelect.ItemIndicator>
                    <Check size={14} weight="bold" />
                  </RSelect.ItemIndicator>
                </span>
                <span className="menu__text">
                  <RSelect.ItemText>{o.label}</RSelect.ItemText>
                  {o.hint && <span className="menu__hint">{o.hint}</span>}
                </span>
              </RSelect.Item>
            ))}
          </RSelect.Viewport>
        </RSelect.Content>
      </RSelect.Portal>
    </RSelect.Root>
  );
}

export function Modal({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  size = "md",
  className = "",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  size?: "sm" | "md" | "lg" | "xl";
  className?: string;
}) {
  return (
    <RDialog.Root open={open} onOpenChange={onOpenChange}>
      <RDialog.Portal>
        <RDialog.Overlay className="overlay" />
        <RDialog.Content className={`modal modal--${size} ${className}`} aria-describedby={description ? undefined : undefined}>
          <header className="modal__head">
            <div>
              <RDialog.Title className="modal__title display">{title}</RDialog.Title>
              {description ? (
                <RDialog.Description className="modal__desc">{description}</RDialog.Description>
              ) : (
                <RDialog.Description className="visually-hidden">{typeof title === "string" ? title : "Dialog"}</RDialog.Description>
              )}
            </div>
            <RDialog.Close asChild>
              <button className="iconbtn iconbtn--md" aria-label="Close">
                <X size={18} />
              </button>
            </RDialog.Close>
          </header>
          <div className="modal__body">{children}</div>
          {footer && <footer className="modal__foot">{footer}</footer>}
        </RDialog.Content>
      </RDialog.Portal>
    </RDialog.Root>
  );
}

export function Badge({ tone = "neutral", children }: { tone?: "neutral" | "gold" | "teal" | "danger" | "ok"; children: ReactNode }) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = -1;
  do {
    v /= 1000;
    i++;
  } while (v >= 1000 && i < units.length - 1);
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}
