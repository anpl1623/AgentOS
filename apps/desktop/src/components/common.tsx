import type { ReactNode } from "react";

import { humanise } from "../sdk/format";

/** A risk level, coloured by severity. The one thing on screen that should shout. */
export function Risk({ level }: { level: string }) {
  return <span className={`badge ${level}`}>{level}</span>;
}

/** A run state, coloured by disposition. */
export function State({ state }: { state: string }) {
  const tone =
    state === "completed"
      ? "ok"
      : state === "failed"
        ? "high"
        : state === "cancelled"
          ? "medium"
          : ["idle"].includes(state)
            ? "neutral"
            : "live";
  return <span className={`badge ${tone}`}>{humanise(state)}</span>;
}

/** A task status. */
export function Status({ status }: { status: string }) {
  const tone =
    status === "succeeded"
      ? "ok"
      : status === "failed"
        ? "high"
        : status === "cancelled"
          ? "medium"
          : status === "running"
            ? "live"
            : "neutral";
  return <span className={`badge ${tone}`}>{status}</span>;
}

/**
 * Whether an agent may be given work.
 *
 * Deliberately not the run-state badge: an *enabled* agent is not a *running*
 * one, and showing "running" beside an idle agent would be a lie about what the
 * machine is doing.
 */
export function Enabled({ status }: { status: string }) {
  const on = status === "enabled";
  return <span className={`badge ${on ? "ok" : "neutral"}`}>{on ? "enabled" : "disabled"}</span>;
}

/**
 * The marker for a run that has read untrusted data.
 *
 * Shown wherever such a run appears, not only on the approval card. A person
 * scanning a list should be able to see which work was influenced by something
 * the operator did not write.
 */
export function Tainted({ label = "read untrusted data" }: { label?: string }) {
  return (
    <span className="taint" title="This run has read data from outside the trust boundary.">
      <span aria-hidden="true">⚠</span>
      {label}
    </span>
  );
}

export function Panel({ children }: { children: ReactNode }) {
  return <div className="panel">{children}</div>;
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="empty">{children}</div>;
}

export function Loading({ what }: { what: string }) {
  return <div className="spinner">Loading {what}…</div>;
}

export function ErrorBanner({ message }: { message: string }) {
  return <div className="banner error">{message}</div>;
}

/** A labelled number for the dashboard. */
export function Stat({
  value,
  label,
  tone,
}: {
  value: string | number;
  label: string;
  tone?: "ok" | "warn" | "danger";
}) {
  const colour =
    tone === "ok"
      ? "var(--ok)"
      : tone === "warn"
        ? "var(--warn)"
        : tone === "danger"
          ? "var(--danger)"
          : undefined;
  return (
    <div className="stat">
      <div className="stat-value" style={colour ? { color: colour } : undefined}>
        {value}
      </div>
      <div className="stat-label">{label}</div>
    </div>
  );
}

/** The section heading used throughout. */
export function Section({ title, action }: { title: string; action?: ReactNode }) {
  return (
    <div className="page-head">
      <h2 style={{ margin: "24px 0 10px" }}>{title}</h2>
      {action ? <div style={{ marginTop: 20 }}>{action}</div> : null}
    </div>
  );
}
