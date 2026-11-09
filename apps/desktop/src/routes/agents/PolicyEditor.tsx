import { useCallback, useEffect, useRef, useState } from "react";

import type { PolicyView } from "../../bindings/PolicyView";
import { ErrorBanner, Icon } from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { useDraft, useUnsavedGuard } from "../../sdk/drafts";
import { CodeEditor } from "./CodeEditor";
import {
  POLICY_GUARD,
  type PolicyDraft,
  beginPolicyDraft,
  checkedText,
  policyDirty,
  policySuperseded,
} from "./policyModel";
import { PolicySummary } from "./PolicySummary";

/**
 * An agent's policy: read as a summary, or edited as YAML.
 *
 * The open editor is a draft, guarded against leaving with unsaved changes,
 * and it knows which stored version it began from, so a save made elsewhere
 * while it was open is said before it is overwritten. Checking asks the engine
 * to compile the text without installing it; saving installs it, and the
 * engine's refusal is shown verbatim.
 */
export function PolicyEditor({
  agentId,
  policy,
  onSaved,
}: {
  agentId: string;
  policy: PolicyView | null;
  onSaved: () => void;
}) {
  // The open editor is the draft: `null` is closed.
  const [draft, setDraft, discardDraft] = useDraft<PolicyDraft | null>(`policy:${agentId}`, null);
  useUnsavedGuard(policyDirty(draft), POLICY_GUARD);
  // As with the create form: a confirmed departure was told the edits would
  // be discarded, so they are.
  useEffect(() => discardDraft, [discardDraft]);

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A check answers for the text it was given; once the text changes it says
  // nothing about what is in the box, so it is shown only while they match.
  const [checked, setChecked] = useState<{ document: string; text: string } | null>(null);

  // Closing the editor unmounts the button that was pressed; focus goes back
  // to Edit, where the operator came from, rather than falling to the page.
  const editButton = useRef<HTMLButtonElement>(null);
  const [returnFocus, setReturnFocus] = useState(false);
  useEffect(() => {
    if (returnFocus && draft === null) {
      editButton.current?.focus();
      setReturnFocus(false);
    }
  }, [returnFocus, draft]);

  // An editor opened and left untouched while the policy was saved elsewhere
  // holds nothing of the operator's, so it moves to the new version.
  const superseded = policySuperseded(draft, policy);
  const dirty = policyDirty(draft);
  useEffect(() => {
    if (superseded && !dirty && policy !== null) setDraft(beginPolicyDraft(policy));
  }, [superseded, dirty, policy, setDraft]);

  const document = draft?.document ?? "";

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await api.setPolicy(agentId, document);
      discardDraft();
      setChecked(null);
      setReturnFocus(true);
      onSaved();
    } catch (failure) {
      // The engine's refusal, verbatim: a malformed origin pattern is a
      // compile error with its own explanation, and this is where an
      // operator meets it.
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  }, [agentId, document, discardDraft, onSaved]);

  const check = useCallback(async () => {
    setError(null);
    setChecked(null);
    try {
      const result = await api.checkPolicy(document);
      if (result.valid && result.summary) {
        setChecked({ document, text: checkedText(result.summary) });
      } else {
        setError(result.error ?? "This policy does not compile.");
      }
    } catch (failure) {
      setError(describeError(failure));
    }
  }, [document]);

  if (!policy) {
    return (
      <div className="banner warn flush">This agent has no policy, so every action is denied.</div>
    );
  }

  if (draft === null) {
    return (
      <PolicySummary
        policy={policy}
        editRef={editButton}
        onEdit={() => setDraft(beginPolicyDraft(policy))}
      />
    );
  }

  return (
    <div className="panel policy-card">
      <div className="panel-body">
        {superseded && dirty ? (
          <div className="banner warn">
            This policy was saved elsewhere after you began editing version {draft.baseVersion}; it
            is now version {policy.version}. Saving replaces that version with the text below.
          </div>
        ) : null}
        <CodeEditor
          id="policy"
          label="Policy (YAML)"
          autoFocus
          value={document}
          onChange={(next) =>
            setDraft((previous) => (previous === null ? previous : { ...previous, document: next }))
          }
          meta={
            <>
              <span className="code-editor-meta">editing version {draft.baseVersion}</span>
              {dirty ? <span className="code-editor-dirty">Unsaved changes</span> : null}
            </>
          }
        />
        <div role="status" className="policy-feedback">
          {checked !== null && checked.document === document ? (
            <div className="banner info">{checked.text}</div>
          ) : null}
        </div>
        {error ? <ErrorBanner message={error} /> : null}
        <div className="inline">
          <button type="button" className="primary" disabled={busy} onClick={() => void save()}>
            {busy ? "Saving…" : "Save policy"}
          </button>
          <button type="button" onClick={() => void check()}>
            <Icon name="check" size={15} />
            Check
          </button>
          <button
            type="button"
            className="ghost"
            onClick={() => {
              discardDraft();
              setError(null);
              setChecked(null);
              setReturnFocus(true);
            }}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
