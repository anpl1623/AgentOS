import { useId, useRef } from "react";

import { ErrorBanner, Icon, type IconName, LiveDot } from "../../components/common";
import { useNow } from "../../components/hooks";
import {
  type AuditFindings,
  findingsSummary,
  gapsNote,
  verificationAnnouncement,
  verificationLine,
  verificationTone,
  verificationWord,
} from "./auditModel";
import type { AuditState } from "./hooks";
import { Section } from "./parts";

/** What the seal and the bar beside the button show. */
type SealState = "idle" | "verifying" | "ok" | "warn" | "danger";

const SEAL_ICON: Record<SealState, IconName> = {
  idle: "shield",
  verifying: "shield",
  ok: "check",
  warn: "alert",
  danger: "alert",
};

/**
 * The audit log: a button that checks the whole chain, and what it found.
 *
 * The check is the one deliberate act of trust on this screen, so its answer
 * is drawn to be seen: while it runs a band of light crosses the bar under the
 * button; when it lands the bar fills in the result's hue and the seal beside
 * it pops once, keyed on when the check finished, so a second check that finds
 * the same thing still lands visibly. A break is drawn as a failure, gaps
 * alone as attention, and only a chain with neither as well.
 */
export function AuditLog({ audit }: { audit: AuditState }) {
  const now = useNow();
  const { last, verifying, error, verify } = audit;
  const state: SealState = verifying ? "verifying" : last ? verificationTone(last) : "idle";
  // A result remembered from an earlier visit is drawn still; only one that
  // lands while the screen is open pops and fills, since only that is news.
  const shownOnArrival = useRef(last?.at ?? null);
  const fresh = last !== null && last.at !== shownOnArrival.current ? "" : undefined;

  return (
    <Section
      id="settings-audit"
      intro="Every tool call, approval and refusal is sealed into a hash chain as it happens. Verifying recomputes the chain from its first record."
    >
      {error ? <ErrorBanner message={`The chain could not be verified: ${error}`} /> : null}
      <div className="panel settings-audit" data-state={state}>
        <div className="settings-audit-head">
          <span
            key={last?.at ?? "never"}
            className="settings-audit-seal"
            data-state={state}
            data-fresh={fresh}
            aria-hidden="true"
          >
            <Icon name={SEAL_ICON[state]} size={22} />
          </span>
          <div className="settings-audit-status">
            {/* The sentence is withheld while a check runs, so the same result
                twice is still a change the region announces. */}
            <span
              className="settings-audit-word"
              data-state={state}
              role="status"
              aria-live="polite"
              aria-busy={verifying}
            >
              {last && !verifying ? (
                <>
                  <span className="visually-hidden">audit chain </span>
                  {findingsSummary(last) ?? "Intact"}
                  <span className="visually-hidden"> {verificationAnnouncement(last)}</span>
                </>
              ) : null}
            </span>
            {verifying ? (
              <span className="settings-audit-word" data-state={state}>
                <LiveDot tone="info" />
                Verifying…
              </span>
            ) : last === null ? (
              <span className="settings-audit-word" data-state={state}>
                {verificationWord(null)}
              </span>
            ) : null}
            <span className="faint settings-audit-line">{verificationLine(last, now)}</span>
          </div>
          {/* aria-disabled, not disabled, here and on the header's button: a
              focused button that becomes disabled drops focus to the top of
              the window. The hook ignores a press while a check runs. */}
          <button
            type="button"
            className="settings-button"
            aria-disabled={verifying}
            onClick={verify}
          >
            <Icon name="refresh" size={14} />
            {verifying ? "Verifying…" : "Verify the chain"}
          </button>
        </div>
        <div className="settings-audit-track" aria-hidden="true">
          {state === "idle" ? null : (
            <span
              key={verifying ? "running" : (last?.at ?? "never")}
              className={verifying ? "settings-audit-fill sheen" : "settings-audit-fill"}
              data-state={state}
              data-fresh={fresh}
            />
          )}
        </div>
        <div className="panel-body">
          <p className="field-note settings-audit-explainer">
            Recomputes every hash and checks each record still points at the one before it, then
            that the records are complete: every tool call that started ended, every approval
            granted was asked for, and every run an agent delegated to left records of its own.
          </p>
          {last && !verifying ? <AuditFindingsList last={last} /> : null}
        </div>
      </div>
    </Section>
  );
}

function AuditFindingsList({ last }: { last: AuditFindings }) {
  return (
    <>
      <Findings
        title="Chain breaks"
        tone="error"
        note="The chain does not verify here: a record was altered, removed or reordered."
        found={last.breaks}
      />
      <Findings title="Coverage gaps" tone="warn" note={gapsNote(last)} found={last.gaps} />
    </>
  );
}

/** One kind of finding from the full check, under a heading that says what it means. */
function Findings({
  title,
  tone,
  note,
  found,
}: {
  title: string;
  tone: "error" | "warn";
  note: string;
  found: string[];
}) {
  const id = useId();
  if (found.length === 0) return null;
  return (
    <div className="settings-findings" role="group" aria-labelledby={id}>
      <p className="field-note" id={id}>
        <strong>{title}.</strong> {note}
      </p>
      <div className="stack stagger">
        {found.map((problem, index) => (
          <div key={`${index}:${problem}`} className={`banner ${tone} flush settings-finding`}>
            {problem}
          </div>
        ))}
      </div>
    </div>
  );
}
