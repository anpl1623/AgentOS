import { useState } from "react";

import type { AuditRecordView } from "../../../bindings/AuditRecordView";
import { Visible } from "../../../components/ArgumentBlock";
import {
  CopyButton,
  EmptyState,
  ErrorBanner,
  Icon,
  Loading,
  SkeletonRows,
} from "../../../components/common";
import { api } from "../../../sdk/client";
import { clock } from "../../../sdk/format";
import { useAsync } from "../../../sdk/useAsync";
import { shortHash } from "./traceLook";

/**
 * The audit records the run wrote, in chain order: the evidence behind the
 * trace. Each opens the record as the chain stores it, with every id and hash
 * a copy away, because those are what a person pastes into a report.
 */
export function AuditRecords({
  records,
  loading,
  error,
  live,
}: {
  records: readonly AuditRecordView[] | null;
  loading: boolean;
  error: string | null;
  live: boolean;
}) {
  const [openId, setOpenId] = useState<string | null>(null);
  return (
    <section className="run-audit" aria-labelledby="run-audit">
      <div className="run-section-head">
        <h2 id="run-audit">Audit records</h2>
        {records !== null && records.length > 0 ? (
          <span className="run-count" aria-hidden="true">
            {records.length}
          </span>
        ) : null}
      </div>
      <p className="field-note run-section-note">
        Every record this run wrote to the hash chain, in chain order. Each record's hash covers
        the one before it.
      </p>
      {error ? <ErrorBanner message={`The audit records could not be read: ${error}`} /> : null}
      <div className="panel run-audit-list">
        {loading ? <SkeletonRows count={2} /> : null}
        {records !== null && records.length === 0 ? (
          <div className="tasks-empty-compact">
            <EmptyState
              icon="shield"
              title={
                live ? "Nothing has been written to the chain yet." : "This run wrote no records."
              }
            />
          </div>
        ) : null}
        {records !== null && records.length > 0 ? (
          <div role="list" aria-labelledby="run-audit">
            {records.map((record) => (
              <div role="listitem" key={record.id}>
                <AuditRow
                  record={record}
                  open={openId === record.id}
                  onToggle={() =>
                    setOpenId((current) => (current === record.id ? null : record.id))
                  }
                />
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </section>
  );
}

function AuditRow({
  record,
  open,
  onToggle,
}: {
  record: AuditRecordView;
  open: boolean;
  onToggle: () => void;
}) {
  const detailsId = `run-audit-${record.id}`;
  return (
    <>
      <button
        type="button"
        className={`row clickable run-audit-row${record.security_relevant ? " security" : ""}`}
        data-spotlight
        aria-expanded={open}
        aria-controls={open ? detailsId : undefined}
        onClick={onToggle}
      >
        <span className="run-audit-seq">#{record.sequence}</span>
        <time className="run-audit-time" dateTime={record.at}>
          {clock(record.at)}
        </time>
        <span className="run-audit-kind">
          {record.security_relevant ? (
            <>
              <span className="visually-hidden">security relevant: </span>
              <Icon name="shield" size={14} />
            </>
          ) : null}
          <span className="run-audit-kind-name">{record.kind}</span>
        </span>
        <span className="run-audit-hashes">
          <span className="visually-hidden">hash of the record before: </span>
          <span className="run-audit-prev" title={record.prev_hash}>
            {shortHash(record.prev_hash)}
          </span>
          <Icon name="chevron-right" size={12} />
          <span className="visually-hidden">, this record's hash: </span>
          <span className="run-audit-hash" title={record.hash}>
            {shortHash(record.hash)}
          </span>
        </span>
        <Icon name="chevron-down" size={14} className="run-audit-chevron" />
      </button>
      {open ? <AuditRecord id={record.id} detailsId={detailsId} /> : null}
    </>
  );
}

/** One record in full, read from the chain when it is opened. */
function AuditRecord({ id, detailsId }: { id: string; detailsId: string }) {
  const record = useAsync(() => api.auditRecord(id), [id]);
  const data = record.data;
  return (
    <div id={detailsId} className="panel-body run-audit-details">
      {record.error ? <ErrorBanner message={record.error} /> : null}
      {record.loading ? <Loading what="the record" /> : null}
      {data ? (
        <>
          <dl className="facts">
            <dt>Sequence</dt>
            <dd>#{data.sequence}</dd>
            <dt>Record id</dt>
            <dd>
              <span className="mono">{id}</span>
              <CopyButton text={id} label="Copy the record id" />
            </dd>
            <dt>Record before it</dt>
            <dd>
              <span className="mono">{data.prev_hash}</span>
              <CopyButton text={data.prev_hash} label="Copy the hash of the record before it" />
            </dd>
            <dt>This record</dt>
            <dd>
              <span className="mono">{data.hash}</span>
              <CopyButton text={data.hash} label="Copy this record's hash" />
            </dd>
          </dl>
          <pre className="code wrap" tabIndex={0} role="region" aria-label="Record payload">
            <Visible text={data.payload} />
          </pre>
        </>
      ) : null}
    </div>
  );
}
