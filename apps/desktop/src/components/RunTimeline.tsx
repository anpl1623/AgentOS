import type { SpanTarget, Timeline, TimelineSpan } from "../routes/runModel";
import { clock } from "../sdk/format";
import {
  AXIS_HEIGHT,
  Axis,
  Figure,
  HatchDefs,
  Legend,
  type LegendItem,
  TRACK_GAP,
  TRACK_HEIGHT,
  Track,
  elapsed,
  type TrackMarker,
  useHatches,
} from "./charts";

const LEGEND: readonly LegendItem[] = [
  { label: "model turn", tone: "info" },
  { label: "call ran", tone: "ok" },
  { label: "call failed", tone: "danger" },
  { label: "refused", tone: "blocked", hatch: "refused" },
  { label: "did not run", tone: "neutral", hatch: "skipped" },
  { label: "waiting on you", tone: "warn", hatch: "waiting" },
];

const TAINT_LEGEND: LegendItem = {
  label: "first call after it read untrusted data",
  tone: "warn",
  marker: true,
};

/**
 * Where a run's time went, in three tracks over one axis.
 *
 * Model turns, tool calls, and the time the run spent stopped waiting for a
 * person. The last is why the chart exists: the trace lists every call's
 * duration but never the interval the run sat waiting for a decision, which
 * is the one delay the operator causes and the only one they can remove.
 *
 * A pure function of the timeline it is given, which is a pure function of
 * the trace: no animation, nothing measured, nothing kept between renders.
 * Renders nothing when there is no timeline, so a run with nothing to show
 * gets no empty chart.
 */
export function RunTimeline({
  timeline,
  onSelect,
}: {
  timeline: Timeline | null;
  /** Bring the row a span stands for into view. */
  onSelect: (target: SpanTarget) => void;
}) {
  const hatches = useHatches();
  if (timeline === null) return null;

  const domain = { start: timeline.start, end: timeline.end };
  const tracks: { label: string; spans: TimelineSpan[] }[] = [
    { label: "Model", spans: timeline.model },
    { label: "Tools", spans: timeline.tools },
    { label: "Waiting on you", spans: timeline.waiting },
  ];
  const axisY = tracks.length * (TRACK_HEIGHT + TRACK_GAP);
  const select = (spans: readonly TimelineSpan[]) => (id: string) => {
    const span = spans.find((candidate) => candidate.id === id);
    if (span) onSelect(span.target);
  };
  const legend = timeline.taint ? [...LEGEND, TAINT_LEGEND] : LEGEND;

  return (
    <Figure
      title="Timeline"
      caption={
        <>
          Model turns are timed from one step to the next, so they are an inference, not a
          measurement. Tool calls are as the runtime timed them. Select a span to find its row.
        </>
      }
      table={<SpanTable tracks={tracks} taint={timeline.taint} />}
    >
      <div className="chart">
        <svg role="group" aria-label="Where the run's time went" height={axisY + AXIS_HEIGHT}>
          <HatchDefs hatches={hatches} />
          <Axis y={axisY} domain={domain} />
          {tracks.map((track, index) => (
            <Track
              key={track.label}
              label={track.label}
              y={index * (TRACK_HEIGHT + TRACK_GAP)}
              domain={domain}
              spans={track.spans}
              hatches={hatches}
              markers={track.label === "Tools" && timeline.taint ? [timeline.taint] : []}
              onSelect={select(track.spans)}
            />
          ))}
        </svg>
      </div>
      <p className="chart-summary">{timeline.summary}</p>
      <Legend items={legend} hatches={hatches} />
    </Figure>
  );
}

/** The chart's facts as a table: one row per span, in track order. */
function SpanTable({
  tracks,
  taint,
}: {
  tracks: readonly { label: string; spans: TimelineSpan[] }[];
  taint: TrackMarker | null;
}) {
  return (
    <table>
      <thead>
        <tr>
          <th scope="col">Track</th>
          <th scope="col">From</th>
          <th scope="col">For</th>
          <th scope="col">What</th>
        </tr>
      </thead>
      <tbody>
        {tracks.flatMap((track) =>
          track.spans.map((span) => (
            <tr key={`${track.label}-${span.id}`}>
              <td>{track.label}</td>
              <td>{clock(new Date(span.from).toISOString())}</td>
              <td>{elapsed(span.to - span.from)}</td>
              <td>{span.label}</td>
            </tr>
          )),
        )}
        {taint ? (
          <tr>
            <td>Tools</td>
            <td>{clock(new Date(taint.at).toISOString())}</td>
            <td>—</td>
            <td>First call after the run {taint.label}</td>
          </tr>
        ) : null}
      </tbody>
    </table>
  );
}
