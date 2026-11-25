import { useState, type ReactNode } from "react";
import { api } from "../../lib/api";

/// The primitive every Settings screen is built from: a label and its
/// explanation on the left, one control on the right, a hairline between
/// siblings.
export function SettingCard({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="set-card">
      {title && <h3 className="set-card-title">{title}</h3>}
      <div className="set-rows">{children}</div>
    </section>
  );
}

export function SettingRow({
  label,
  desc,
  control,
}: {
  label: string;
  desc?: ReactNode;
  control: ReactNode;
}) {
  return (
    <div className="set-row">
      <div className="set-row-text">
        <strong>{label}</strong>
        {desc && <p>{desc}</p>}
      </div>
      <div className="set-row-control">{control}</div>
    </div>
  );
}

export function Toggle({
  on,
  onChange,
  label,
  disabled,
}: {
  on: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className="switch" aria-label={label}>
      <input type="checkbox" checked={on} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className="switch-track" />
    </label>
  );
}

/// A state, not an action, for things that have no switch.
export function StatusPill({
  tone,
  children,
  onClick,
}: {
  tone: "good" | "warn" | "neutral";
  children: ReactNode;
  onClick?: () => void;
}) {
  const cls = `set-pill is-${tone}`;
  return onClick ? (
    <button type="button" className={cls} onClick={onClick}>{children}</button>
  ) : (
    <span className={cls}>{children}</span>
  );
}

/// A system permission, as a toggle. An app cannot revoke a permission,
/// only ask for it: off to on asks, on to off opens the system's own page,
/// and a refusal (the system asks only once) opens that page too.
export function PermissionToggle({
  granted,
  label,
  pane,
  onRequest,
  onChanged,
}: {
  granted: boolean;
  label: string;
  /// The System Settings anchor, e.g. "accessibility".
  pane: string;
  /// Only called when not yet granted; returns the new state.
  onRequest: () => Promise<boolean>;
  onChanged: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const flip = () => {
    if (granted) {
      void api.openPrivacySettings(pane);
      return;
    }
    setBusy(true);
    void onRequest()
      .then((ok) => {
        // Already denied once: the prompt will not appear again, so send
        // them where the answer can actually be changed.
        if (!ok) void api.openPrivacySettings(pane);
      })
      .catch(() => void api.openPrivacySettings(pane))
      .finally(() => {
        setBusy(false);
        onChanged();
      });
  };
  return (
    <label className={`switch${busy ? " is-busy" : ""}`} aria-label={label}>
      <input type="checkbox" checked={granted} disabled={busy} onChange={flip} />
      <span className="switch-track" />
    </label>
  );
}

export function RowAction({ children, onClick }: { children: ReactNode; onClick: () => void }) {
  return (
    <button type="button" className="set-row-action" onClick={onClick}>
      {children}
    </button>
  );
}

export function RowButton({ children, onClick }: { children: ReactNode; onClick: () => void }) {
  return (
    <button type="button" className="set-row-button" onClick={onClick}>
      {children}
    </button>
  );
}
