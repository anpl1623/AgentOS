import { Icon, LiveDot } from "../../../components/common";
import type { RowMark } from "../tasksModel";

/**
 * The mark at the head of a task row: a live dot for running work, otherwise
 * an icon whose shape says the status. Decoration — the chip at the row's end
 * says the same in words.
 */
export function RowMarkIcon({ mark }: { mark: RowMark }) {
  return (
    <span className={`tasks-mark tone-${mark.tone}`} aria-hidden="true">
      {mark.icon === null ? <LiveDot tone="info" /> : <Icon name={mark.icon} size={14} />}
    </span>
  );
}
