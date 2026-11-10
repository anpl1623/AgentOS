import { Icon } from "../../components/common";
import type { EventFamily, FamilyCount } from "./eventFamilies";

/**
 * The filters above the feed: text, exact kind and the security view on one
 * line, the family chips beneath.
 *
 * The security view sits with the line rather than among the families because
 * it is a different question: it reads the log itself for refusals and
 * escalations, where a family only narrows what is already held. Every chip is
 * a toggle button, so its state is announced as well as drawn; pressing the
 * chosen family again returns to all of them.
 */
export function FeedToolbar({
  text,
  onText,
  kind,
  kinds,
  onKind,
  family,
  families,
  onFamily,
  securityOnly,
  onSecurity,
  filtering,
  onClear,
  shown,
  held,
}: {
  text: string;
  onText: (next: string) => void;
  kind: string;
  kinds: readonly string[];
  onKind: (next: string) => void;
  family: EventFamily | "";
  families: readonly FamilyCount[];
  onFamily: (next: EventFamily | "") => void;
  securityOnly: boolean;
  onSecurity: (next: boolean) => void;
  filtering: boolean;
  onClear: () => void;
  /** Rows the filters keep, and rows held before them. */
  shown: number;
  held: number;
}) {
  const all = families.reduce((sum, each) => sum + each.count, 0);
  return (
    <div className="activity-filters">
      <div className="activity-toolbar">
        <label className="activity-search">
          <Icon name="search" size={16} />
          <input
            type="search"
            aria-label="Filter by kind or summary"
            placeholder="Filter by kind or summary"
            value={text}
            onChange={(event) => onText(event.target.value)}
          />
        </label>
        <select aria-label="Kind" value={kind} onChange={(event) => onKind(event.target.value)}>
          <option value="">Every kind</option>
          {kinds.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="chip chip-security"
          aria-pressed={securityOnly}
          onClick={() => onSecurity(!securityOnly)}
        >
          <Icon name="shield" size={14} />
          Security only
        </button>
        <div className="activity-tally">
          {/* Not live: it changes with every arriving event, and the log
           * region below already speaks while Follow is on. */}
          <span className="activity-count">
            {shown} of {held}
          </span>
          <button type="button" className="ghost" disabled={!filtering} onClick={onClear}>
            Clear
          </button>
        </div>
      </div>

      <div className="chip-group" role="group" aria-label="Event family">
        <button
          type="button"
          className="chip"
          aria-pressed={family === ""}
          onClick={() => onFamily("")}
        >
          All
          <span className="chip-count">{all}</span>
        </button>
        {families.map((each) => (
          <button
            key={each.family}
            type="button"
            className="chip"
            aria-pressed={family === each.family}
            onClick={() => onFamily(family === each.family ? "" : each.family)}
          >
            <Icon name={each.icon} size={14} />
            {each.label}
            <span className="chip-count">{each.count}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
