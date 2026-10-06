import { type KeyboardEvent, useEffect, useId, useMemo, useRef, useState } from "react";

import type { AgentSummary } from "../bindings/AgentSummary";
import type { Navigate } from "../routes/route";
import { api, describeError } from "../sdk/client";
import { useModal } from "../shell/modal";
import { NAV } from "../shell/nav";
import {
  type PaletteEntry,
  agentEntries,
  moveActive,
  rankEntries,
  screenEntries,
} from "../shell/palette";

/**
 * The command palette, opened with `Cmd/Ctrl+K`: a way to reach a screen or a
 * named agent without remembering which list holds it.
 *
 * It goes places and does nothing else. The rule and its reason are in
 * `shell/palette.ts`, where an entry is a route and not a callback.
 *
 * Agents are read through the existing client each time it opens, so a new or
 * renamed agent is there without the palette keeping a copy that could be
 * stale. Screens are listed at once and do not wait for that read.
 */
export function CommandPalette({
  navigate,
  onClose,
  modKey,
}: {
  navigate: Navigate;
  onClose: () => void;
  /** How the shortcut column writes the modifier: `⌘` or `Ctrl+`. */
  modKey: string;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const optionId = useId();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [agents, setAgents] = useState<readonly AgentSummary[]>([]);
  const [agentError, setAgentError] = useState<string | null>(null);

  useModal(dialog, onClose, input);

  useEffect(() => {
    let live = true;
    api
      .listAgents()
      .then((list) => {
        if (live) setAgents(list);
      })
      .catch((failure: unknown) => {
        if (live) setAgentError(describeError(failure));
      });
    return () => {
      live = false;
    };
  }, []);

  const entries = useMemo(
    () => rankEntries([...screenEntries(), ...agentEntries(agents)], query),
    [agents, query],
  );
  const current = Math.min(active, Math.max(entries.length - 1, 0));

  useEffect(() => {
    document.getElementById(`${optionId}-${current}`)?.scrollIntoView({ block: "nearest" });
  }, [current, optionId]);

  const choose = (entry: PaletteEntry | undefined) => {
    if (entry === undefined) return;
    onClose();
    navigate(entry.route);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      setActive(moveActive(current, event.key === "ArrowDown" ? 1 : -1, entries.length));
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(entries[current]);
    } else if ((event.metaKey || event.ctrlKey) && (event.key === "k" || event.key === "K")) {
      // The key that opened it closes it.
      event.preventDefault();
      onClose();
    }
  };

  const shortcut = (entry: PaletteEntry): string | null => {
    if (entry.group !== "Screens") return null;
    const index = NAV.findIndex((item) => item.route === entry.route.name);
    return index === -1 ? null : `${modKey}${index + 1}`;
  };

  return (
    <div
      className="palette-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div ref={dialog} className="palette" role="dialog" aria-modal="true" aria-label="Go to">
        <input
          ref={input}
          className="palette-input"
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={entries.length > 0 ? `${optionId}-${current}` : undefined}
          aria-label="Go to a screen or agent"
          placeholder="Go to a screen or agent…"
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
        />

        <div className="palette-list" id={listId} role="listbox" aria-label="Destinations">
          {/* Groups in the order the ranking put them, best match first. */}
          {[...new Set(entries.map((entry) => entry.group))].map((group) => {
            const members = entries
              .map((entry, index) => ({ entry, index }))
              .filter(({ entry }) => entry.group === group);
            if (members.length === 0) return null;
            const groupId = `${listId}-${group}`;
            return (
              <div key={group} role="group" aria-labelledby={groupId}>
                <div className="palette-group" id={groupId} role="presentation">
                  {group}
                </div>
                {members.map(({ entry, index }) => (
                  <div
                    key={entry.id}
                    id={`${optionId}-${index}`}
                    role="option"
                    aria-selected={index === current}
                    className={index === current ? "palette-entry active" : "palette-entry"}
                    onMouseMove={() => {
                      if (index !== current) setActive(index);
                    }}
                    // Keep focus in the input, where the arrow keys are read.
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => choose(entry)}
                  >
                    <span>{entry.label}</span>
                    <span className="faint">{entry.hint}</span>
                    <span className="spacer" />
                    {entry.badge !== null ? (
                      <span className="badge neutral">{entry.badge}</span>
                    ) : null}
                    {shortcut(entry) !== null ? (
                      <kbd aria-hidden="true">{shortcut(entry)}</kbd>
                    ) : null}
                  </div>
                ))}
              </div>
            );
          })}
        </div>

        {entries.length === 0 ? (
          <div className="empty" role="status">
            Nothing matches “{query.trim()}”.
          </div>
        ) : null}
        {agentError !== null ? (
          <div className="palette-hint" role="status">
            Agents could not be listed: {agentError}
          </div>
        ) : null}
        <div className="palette-hint" aria-hidden="true">
          <span>
            <kbd>↑</kbd> <kbd>↓</kbd> move
          </span>
          <span>
            <kbd>Enter</kbd> go
          </span>
          <span>
            <kbd>Esc</kbd> close
          </span>
        </div>
      </div>
    </div>
  );
}
