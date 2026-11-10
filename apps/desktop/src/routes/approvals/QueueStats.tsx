import { LiveDot, StatTile } from "../../components/common";
import { type QueueSummary, type Tally, riskStatTone, riskWord, tallyLine } from "./deskModel";

/**
 * The figures over the queue: the worst risk waiting, how many requests come
 * from runs that read untrusted data, how long the oldest has held its run,
 * and how the recent decisions went. Only risk and taint take a status hue,
 * and only when there is something to see; the rest stay quiet.
 *
 * The frame is a size container, so the four figures go two by two whenever
 * a row of four will not fit, rather than three and an orphan; it holds
 * nothing positioned, so containing it changes nothing else.
 */
export function QueueStats({
  summary,
  loaded,
  recent,
  tally,
  onRecent,
}: {
  summary: QueueSummary;
  /** The pending list has been read; until then the queue's figures are unknown. */
  loaded: boolean;
  /** How many recent decisions are listed, or `null` while they load. */
  recent: number | null;
  tally: Tally;
  onRecent: () => void;
}) {
  const highest = summary.highest;
  return (
    <div className="approvals-stats-frame">
      <div className="grid stats approvals-stats">
        <StatTile
          label="Highest risk"
          icon="zap"
          value={loaded ? (highest === null ? "None" : riskWord(highest)) : null}
          tone={riskStatTone(highest)}
          hint={highest === null ? "nothing is waiting" : "read first: the queue is sorted by risk"}
        />
        <StatTile
          label="Untrusted input"
          icon="alert"
          value={loaded ? summary.tainted : null}
          tone={summary.tainted > 0 ? "warn" : "accent"}
          hint={
            summary.onlyTaint > 0
              ? `${summary.onlyTaint} asked only because of it`
              : summary.tainted > 0
                ? "its proposals may be steered"
                : "no request read untrusted data"
          }
        />
        <StatTile
          label="Longest blocked"
          icon="clock"
          value={loaded ? (summary.oldest ?? "—") : null}
          hint={summary.oldest === null ? "no run is waiting" : "its run waits while you decide"}
        />
        <StatTile
          label="Decided"
          icon="check"
          value={recent}
          hint={tallyLine(tally)}
          onClick={onRecent}
        />
      </div>
    </div>
  );
}

/**
 * The count beside the title, which pins with it while the queue scrolls. It
 * pulses while a run is held waiting, because that is happening now; an empty
 * queue's dot is still.
 */
export function WaitingPill({ waiting, loaded }: { waiting: number; loaded: boolean }) {
  if (!loaded) return null;
  return waiting > 0 ? (
    <span className="approvals-waiting">
      <LiveDot tone="warn" />
      {waiting === 1 ? "1 waiting on you" : `${waiting} waiting on you`}
    </span>
  ) : (
    <span className="approvals-waiting calm">
      <LiveDot tone="ok" pulse={false} />
      Nothing waiting
    </span>
  );
}
