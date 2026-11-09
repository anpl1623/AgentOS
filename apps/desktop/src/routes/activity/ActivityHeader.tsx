import { Icon, LiveDot, PageHeader, Switch } from "../../components/common";
import { DEEP_LOAD } from "./activityModel";
import type { LiveStatus } from "./feedSignals";

/**
 * The screen's title, and the controls that stay in reach while the feed
 * scrolls: whether the stream is live, Follow, copying what is visible and
 * reading further back. The title row pins as a glass bar, so Follow can be
 * switched back on from anywhere in a long feed.
 */
export function ActivityHeader({
  runScoped,
  live,
  follow,
  onFollow,
  visible,
  copyState,
  onCopy,
  loadBusy,
  onLoad,
}: {
  /** The feed is narrowed to one run from the address. */
  runScoped: boolean;
  live: LiveStatus;
  follow: boolean;
  onFollow: (next: boolean) => void;
  /** How many events Copy would copy. */
  visible: number;
  copyState: "idle" | "copied" | "failed";
  onCopy: () => void;
  /** A read is already under way. */
  loadBusy: boolean;
  onLoad: () => void;
}) {
  return (
    <PageHeader
      eyebrow={runScoped ? "Audit log · one run" : "Audit log"}
      title="Activity"
      subtitle="Every action, permission decision, refusal and escalation, in the order the log recorded it."
      actions={
        <>
          <div className="live-control">
            <span className={`live-pill tone-${live.tone}${live.pulse ? "" : " idle"}`}>
              <LiveDot tone={live.tone} pulse={live.pulse} />
              <span className="live-pill-label">{live.label}</span>
              {live.note ? <span className="live-pill-note">{live.note}</span> : null}
            </span>
            <Switch checked={follow} onChange={onFollow} label="Follow" />
          </div>
          <button type="button" disabled={visible === 0} onClick={onCopy}>
            <Icon name={copyState === "copied" ? "check" : "copy"} size={16} />
            {copyState === "copied"
              ? `Copied ${visible}`
              : copyState === "failed"
                ? "Copy failed"
                : "Copy visible"}
          </button>
          <button type="button" disabled={loadBusy} onClick={onLoad}>
            <Icon name="clock" size={16} />
            Load {DEEP_LOAD}
          </button>
        </>
      }
    />
  );
}
