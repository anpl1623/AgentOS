import { type KeyboardEvent, useEffect, useRef } from "react";

import { CopyButton, ErrorBanner, Icon, Loading, SkeletonRows } from "../../components/common";
import { api } from "../../sdk/client";
import { clock } from "../../sdk/format";
import { useAsync } from "../../sdk/useAsync";
import type { Navigate } from "../route";
import { shortHash } from "./activityModel";
import { needsReveal } from "./feedSignals";
import { RECORD_ID } from "./FeedRow";
import { scrollContainer } from "./useFollow";

const RECORD_TITLE_ID = "activity-record-title";

/**
 * One record as the chain stores it: the payload in full and both hashes.
 *
 * Beside the feed when the screen is wide enough, beneath it otherwise. Focus
 * moves to its heading when it opens, so a keyboard operator who opened it
 * from deep in the feed is taken to what they asked for; the page scrolls to
 * it only when it is not already in view, so a record that opened beside the
 * feed does not drag the row that opened it away. Escape closes it.
 */
export function Record({
  id,
  navigate,
  onClose,
}: {
  id: string;
  navigate: Navigate;
  onClose: () => void;
}) {
  const record = useAsync(() => api.auditRecord(id), [id]);
  const heading = useRef<HTMLHeadingElement>(null);
  const data = record.data;
  const runId = data?.run_id ?? null;

  useEffect(() => {
    const target = heading.current;
    if (target === null) return;
    if (needsReveal(target.getBoundingClientRect(), viewOf(target))) {
      target.scrollIntoView({ block: "start" });
    }
    target.focus({ preventScroll: true });
  }, [id]);

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    onClose();
  };

  return (
    <section
      id={RECORD_ID}
      className="activity-record"
      aria-labelledby={RECORD_TITLE_ID}
      onKeyDown={onKeyDown}
    >
      <div className="record-head">
        <p className="record-eyebrow">
          <Icon name="link" size={14} />
          Audit record
        </p>
        <h2 id={RECORD_TITLE_ID} ref={heading} tabIndex={-1} className="record-title">
          {data ? `#${data.sequence} · ${data.kind} · ${clock(data.at)}` : "Record"}
        </h2>
        <div className="record-actions">
          {runId !== null ? (
            <button type="button" onClick={() => navigate({ name: "tasks", runId })}>
              Open run
              <Icon name="chevron-right" size={16} />
            </button>
          ) : null}
          <button type="button" className="ghost" onClick={onClose}>
            <Icon name="x" size={16} />
            Close
          </button>
        </div>
      </div>
      {record.error ? <ErrorBanner message={record.error} /> : null}
      {record.loading ? (
        <div className="record-body">
          <Loading what="record" />
          <SkeletonRows count={2} />
        </div>
      ) : null}
      {data ? (
        <div className="record-body">
          {/* Two links of the chain, the one before above this one: the line
           * between them is the hash this record carries of its neighbour. */}
          <dl className="record-chain">
            <div className="record-link">
              <dt>Record before it</dt>
              <dd>
                <span className="mono" title={data.prev_hash}>
                  {shortHash(data.prev_hash)}
                </span>
                <CopyButton text={data.prev_hash} label="Copy the previous hash" />
              </dd>
            </div>
            <div className="record-link current">
              <dt>This record</dt>
              <dd>
                <span className="mono" title={data.hash}>
                  {shortHash(data.hash)}
                </span>
                <CopyButton text={data.hash} label="Copy this record's hash" />
              </dd>
            </div>
          </dl>
          <pre className="code" tabIndex={0} role="region" aria-label="Record payload">
            {data.payload}
          </pre>
          <p className="field-note">
            This record's hash covers the payload above — the same JSON, laid out for reading — with
            its position, time and the hash before it, so a record edited after it was written no
            longer matches, and neither does anything after it.
          </p>
        </div>
      ) : null}
    </section>
  );
}

/**
 * The part of the screen the operator can see, from the element's scrolling
 * ancestor: its box, less the strip its scroll padding keeps for the pinned
 * header.
 */
function viewOf(element: HTMLElement): { top: number; bottom: number } {
  const scroller = scrollContainer(element);
  if (scroller === null) return { top: 0, bottom: window.innerHeight };
  const box = scroller.getBoundingClientRect();
  const padding = Number.parseFloat(getComputedStyle(scroller).scrollPaddingTop) || 0;
  return { top: box.top + padding, bottom: box.bottom };
}
