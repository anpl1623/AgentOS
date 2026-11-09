import { useId, useState } from "react";

import { ErrorBanner, Icon } from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { humanise } from "../../sdk/format";
import { ConfidenceField } from "./MemoryRows";
import { MEMORY_KINDS } from "./memoryModel";

/**
 * Write a note into an agent's memory: the words first, then what kind of
 * note it is and how far to trust it, then Remember.
 */
export function RememberForm({
  agentId,
  onRemembered,
}: {
  agentId: string;
  onRemembered: () => void;
}) {
  const [kind, setKind] = useState<string>("fact");
  const [content, setContent] = useState("");
  const [confidence, setConfidence] = useState(1);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const kindId = useId();
  const contentId = useId();
  const confidenceId = useId();

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.remember({ agent_id: agentId, kind, content, confidence });
      setContent("");
      setConfidence(1);
      onRemembered();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel form remember">
      <div className="panel-body">
        <div className="field">
          <label htmlFor={contentId}>Remember</label>
          <textarea
            id={contentId}
            rows={2}
            placeholder="A fact, decision or preference this agent should keep in mind"
            value={content}
            onChange={(event) => setContent(event.target.value)}
          />
        </div>
        <div className="remember-controls">
          <div className="field">
            <label htmlFor={kindId}>Kind</label>
            <select id={kindId} value={kind} onChange={(event) => setKind(event.target.value)}>
              {MEMORY_KINDS.map((each) => (
                <option key={each} value={each}>
                  {humanise(each)}
                </option>
              ))}
            </select>
          </div>
          <ConfidenceField id={confidenceId} value={confidence} onChange={setConfidence} />
        </div>
        {error ? <ErrorBanner message={error} /> : null}
        <div className="inline">
          <button
            type="button"
            className="primary"
            disabled={busy || content.trim() === ""}
            onClick={() => void submit()}
          >
            <Icon name="plus" size={16} />
            {busy ? "Saving…" : "Remember"}
          </button>
          <span className="faint">Recorded as your own note, whatever it says.</span>
        </div>
      </div>
    </div>
  );
}
