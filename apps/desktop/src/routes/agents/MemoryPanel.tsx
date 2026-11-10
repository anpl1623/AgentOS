import { useEffect, useState } from "react";

import type { MemoryView } from "../../bindings/MemoryView";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { visibleText } from "../../components/argumentText";
import {
  Empty,
  EmptyState,
  ErrorBanner,
  Icon,
  Loading,
  SkeletonRows,
} from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { humanise } from "../../sdk/format";
import { useAsync } from "../../sdk/useAsync";
import { Item, List } from "./lists";
import { type MemoryEdit, MemoryEditor, MemoryLine } from "./MemoryRows";
import { MEMORY_KINDS, filterMemories, memoryRevision } from "./memoryModel";
import { RememberForm } from "./RememberForm";
import { Section } from "./Section";

/**
 * What the agent remembers, which shapes the prompt of every run it makes:
 * a note to add, a filter, and each memory with its source, its confidence and
 * whether it reaches the prompt at all. Forgetting asks first.
 */
export function MemoryPanel({ agentId }: { agentId: string }) {
  const memories = useAsync(() => api.listMemories(agentId), [agentId]);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<string | null>(null);
  const [editing, setEditing] = useState<MemoryEdit | null>(null);
  const [forgetting, setForgetting] = useState<MemoryView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The memory whose Edit button takes focus back once its editor closes: the
  // editor's own buttons unmount with it, and focus would fall to the page.
  // Held for one commit only, after the row's own effect has used it, so a
  // row that comes back later under a changed filter does not steal focus.
  const [returnTo, setReturnTo] = useState<string | null>(null);
  useEffect(() => {
    if (returnTo !== null) setReturnTo(null);
  }, [returnTo]);

  const closeEditor = (id: string) => {
    setEditing(null);
    setReturnTo(id);
  };

  const shown = memories.data ? filterMemories(memories.data, query, kind) : [];

  const act = async (work: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await work();
      memories.reload();
      return true;
    } catch (failure) {
      setError(describeError(failure));
      return false;
    } finally {
      setBusy(false);
    }
  };

  return (
    <Section
      id="agent-memory"
      title="Memory"
      icon="sparkles"
      aside={
        memories.data ? <span className="section-count">{memories.data.length}</span> : undefined
      }
    >
      <RememberForm agentId={agentId} onRemembered={memories.reload} />

      <div className="toolbar memory-toolbar">
        <div className="agents-search">
          <Icon name="search" size={16} />
          <input
            type="search"
            aria-label="Search memories"
            placeholder="Search content or source"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        <div className="segmented memory-kinds" role="group" aria-label="Kind">
          <button type="button" aria-pressed={kind === null} onClick={() => setKind(null)}>
            All
          </button>
          {MEMORY_KINDS.map((each) => (
            <button
              key={each}
              type="button"
              aria-pressed={kind === each}
              onClick={() => setKind(each)}
            >
              {humanise(each)}
            </button>
          ))}
        </div>
      </div>

      {memories.error ? <ErrorBanner message={memories.error} /> : null}
      {error ? <ErrorBanner message={error} /> : null}
      <div className="panel">
        {memories.loading ? (
          <>
            <Loading what="memories" />
            <SkeletonRows count={2} />
          </>
        ) : null}
        {memories.data?.length === 0 ? (
          <EmptyState icon="sparkles" title="Nothing remembered yet">
            Notes written above, and what the agent records as it works, are listed here with
            where each came from.
          </EmptyState>
        ) : null}
        {memories.data && memories.data.length > 0 && shown.length === 0 ? (
          <Empty>No memory matches.</Empty>
        ) : null}
        {shown.length > 0 ? (
          <List labelledBy="agent-memory">
            {shown.map((memory) => (
              <Item key={memory.id}>
                {editing?.id === memory.id ? (
                  <MemoryEditor
                    memory={memory}
                    draft={editing}
                    busy={busy}
                    onChange={setEditing}
                    onCancel={() => closeEditor(memory.id)}
                    onSave={(content, confidence) => {
                      const revision = memoryRevision(memory, content, confidence);
                      if (revision === null) {
                        closeEditor(memory.id);
                        return;
                      }
                      void act(() =>
                        api.reviseMemory(memory.id, revision.content, revision.confidence),
                      ).then((ok) => {
                        if (ok) closeEditor(memory.id);
                      });
                    }}
                  />
                ) : (
                  <MemoryLine
                    memory={memory}
                    busy={busy}
                    refocus={returnTo === memory.id}
                    onEdit={() =>
                      setEditing({
                        id: memory.id,
                        content: memory.content,
                        confidence: memory.confidence,
                      })
                    }
                    onForget={() => setForgetting(memory)}
                  />
                )}
              </Item>
            ))}
          </List>
        ) : null}
      </div>

      {forgetting !== null ? (
        <ConfirmDialog
          title="Forget this memory?"
          message={
            `“${visibleText(forgetting.content)}”\n\n` +
            "It is deleted from this agent's memory, and the audit log records that you forgot it."
          }
          confirmLabel="Forget"
          cancelLabel="Keep"
          onAnswer={(confirmed) => {
            const memory = forgetting;
            setForgetting(null);
            if (!confirmed) return;
            void act(() => api.forgetMemory(memory.id)).then((ok) => {
              // The row, and the Forget button focus went back to, are about
              // to leave the list; the section's heading is the nearest place
              // that stays. Not scrolled to: the list is where the eye is.
              if (ok) document.getElementById("agent-memory")?.focus({ preventScroll: true });
            });
          }}
        />
      ) : null}
    </Section>
  );
}
