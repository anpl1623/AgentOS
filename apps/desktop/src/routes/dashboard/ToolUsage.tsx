import type { CSSProperties } from "react";

import type { ToolUsageView } from "../../bindings/ToolUsageView";
import { Empty, ErrorBanner, Risk, Row, SkeletonRows } from "../../components/common";
import { ago, duration } from "../../sdk/format";
import {
  type MeterPart,
  TOOL_WINDOWS,
  type ToolRow,
  type ToolWindow,
  toolMeter,
  toolRows,
} from "./dashboardModel";
import { Item, List, Section } from "./Section";

/** Placeholder rows on first paint. */
const SKELETON_ROWS = 3;

/** The words for each part of a meter, in the legend's order. */
const PART_NAMES: Readonly<Record<MeterPart["kind"], string>> = {
  ran: "Ran",
  failed: "Failed",
  refused: "Refused",
  other: "Did not run",
};

/**
 * Each tool's calls over the last 7 or 30 days, as a row and a meter.
 *
 * The meter is the busiest tool's length scaled down, split by how the calls
 * ended, and it grows in from the left whenever the window changes. It is a
 * second way of saying what the row's words already say, so it is hidden from
 * assistive technology; the refused part is hatched as well as tinted, so it
 * still reads without colour.
 */
export function ToolUsage({
  usage,
  loading,
  error,
  days,
  setDays,
}: {
  usage: readonly ToolUsageView[] | null;
  loading: boolean;
  error: string | null;
  days: ToolWindow;
  setDays: (days: ToolWindow) => void;
}) {
  return (
    <Section
      id="dashboard-tools"
      title="Tools"
      icon="zap"
      tail={
        <div className="segmented" role="group" aria-label="Tool usage over">
          {TOOL_WINDOWS.map((window) => (
            <button
              key={window}
              type="button"
              aria-pressed={days === window}
              onClick={() => setDays(window)}
            >
              {window} days
            </button>
          ))}
        </div>
      }
      above={error ? <ErrorBanner message={`Tool usage could not be read: ${error}`} /> : null}
    >
      <div className="panel">
        {usage === null ? (
          loading ? (
            <SkeletonRows count={SKELETON_ROWS} />
          ) : null
        ) : (
          // Keyed on the window so a change of window grows the meters in
          // again; a poll answering for the same window does not.
          <ToolList key={days} usage={usage} days={days} />
        )}
      </div>
    </Section>
  );
}

/**
 * One row per tool called in the window.
 *
 * A tool refused more often than it ran carries a danger chip. Refusals are
 * otherwise drawn as the policy working, in the blocked tone; a tool that is
 * mostly refused is something else, an agent that keeps reaching for what it
 * may not have, and it is the row this panel exists to show.
 */
function ToolList({ usage, days }: { usage: readonly ToolUsageView[]; days: number }) {
  const rows = toolRows(usage);
  if (rows.length === 0) return <Empty>No tool has been called in the last {days} days.</Empty>;
  const busiest = Math.max(...rows.map((row) => row.calls));
  return (
    <>
      <List labelledBy="dashboard-tools" className="stagger">
        {rows.map((row) => (
          <Item key={row.tool}>
            <ToolLine row={row} busiest={busiest} />
          </Item>
        ))}
      </List>
      <Legend />
    </>
  );
}

function ToolLine({ row, busiest }: { row: ToolRow; busiest: number }) {
  const meter = toolMeter(row, busiest);
  return (
    <Row className={row.mostlyRefused ? "dash-tool alarming" : "dash-tool"}>
      <div className="row-main">
        {/* The alarm rides on the title line, so the meter beneath always
            runs the row's full width, however narrow the column. */}
        <div className="row-title dash-tool-title">
          <span className="mono">{row.tool}</span>
          {row.risk ? <Risk level={row.risk} /> : null}
          {row.mostlyRefused ? (
            <span className="verdict danger dash-tool-alarm">
              <span className="visually-hidden">usage </span>Refused more than it ran
            </span>
          ) : null}
        </div>
        <span className="dash-tool-meter" aria-hidden="true">
          <span
            className="dash-tool-meter-fill"
            style={{ "--w": `${meter.width}%` } as CSSProperties}
          >
            {meter.parts.map((part) => (
              <span
                key={part.kind}
                className={`dash-tool-part ${part.kind}`}
                style={{ "--n": part.count } as CSSProperties}
              />
            ))}
          </span>
        </span>
        <div className="row-meta">
          <span>
            {row.calls === 1 ? "1 call" : `${row.calls} calls`} · {row.refused} refused ·{" "}
            {duration(row.totalMs)}
            {row.lastUsedAt ? ` · ${ago(row.lastUsedAt)}` : null}
          </span>
          {row.risk === null ? <span>no longer offered</span> : null}
        </div>
      </div>
    </Row>
  );
}

/** What the meter's parts mean, drawn beneath the list. Hidden like the meters it explains. */
function Legend() {
  return (
    <div className="chart-legend dash-tool-legend" aria-hidden="true">
      {(Object.keys(PART_NAMES) as MeterPart["kind"][]).map((kind) => (
        <span key={kind} className="chart-legend-item">
          <span className={`dash-tool-swatch ${kind}`} />
          {PART_NAMES[kind]}
        </span>
      ))}
    </div>
  );
}
