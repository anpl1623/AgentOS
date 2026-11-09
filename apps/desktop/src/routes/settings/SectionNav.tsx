import { type CSSProperties, useRef } from "react";

import { Icon } from "../../components/common";
import type { SectionEntry } from "./sectionsModel";
import { useMarker } from "./useSectionNav";

/**
 * The in-page navigation: one entry per section, with the section being read
 * marked.
 *
 * One element, two shapes, chosen by the stylesheet: a sticky column beside
 * the sections where there is room, and a sticky strip of chips that scrolls
 * sideways where there is not. Either way the current entry sits on one
 * marker that slides to it, as the sidebar's does, so following the page is
 * seen as movement down the list.
 *
 * Entries are buttons rather than links: the window's address is the hash
 * router's, and a fragment link would be read as a move to another screen.
 * The current one is `aria-current="location"`, a place within this page.
 */
export function SectionNav({
  sections,
  active,
  onJump,
}: {
  sections: readonly SectionEntry[];
  active: string | null;
  onJump: (id: string) => void;
}) {
  const track = useRef<HTMLDivElement>(null);
  const { box, ready } = useMarker(track, active);

  return (
    <nav className="settings-nav" aria-label="Settings sections">
      <p className="settings-nav-title" aria-hidden="true">
        On this page
      </p>
      <div className="settings-nav-track" ref={track}>
        {box === null ? null : (
          <span
            className="settings-nav-marker"
            aria-hidden="true"
            data-ready={ready ? "" : undefined}
            style={
              {
                "--x": `${box.left}px`,
                "--y": `${box.top}px`,
                "--w": `${box.width}px`,
                "--h": `${box.height}px`,
              } as CSSProperties
            }
          />
        )}
        <ul className="settings-nav-list">
          {sections.map((entry) => (
            <li key={entry.id}>
              <button
                type="button"
                className="settings-nav-item"
                aria-current={entry.id === active ? "location" : undefined}
                onClick={() => onJump(entry.id)}
              >
                <Icon name={entry.icon} size={16} />
                <span className="settings-nav-label">{entry.label}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </nav>
  );
}
