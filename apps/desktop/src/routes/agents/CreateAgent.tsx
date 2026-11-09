import { type ReactNode, useCallback, useEffect, useId, useRef, useState } from "react";

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { ToolView } from "../../bindings/ToolView";
import { ErrorBanner, Icon, Risk } from "../../components/common";
import { usePop } from "../../components/motion";
import { api, describeError } from "../../sdk/client";
import { highestRisk, toolCountLabel } from "./agentsModel";
import {
  type CreateForm,
  PROVIDERS,
  type Vision,
  createInput,
  createProgress,
  nameProblem,
  takesBaseUrl,
  toggleTool,
} from "./createModel";
import { ToolPicker } from "./ToolPicker";

/**
 * The create-agent form, as three numbered steps: who it is, which model it
 * runs on, and which tools it is offered. Each step's marker turns to a check
 * once it has what it needs, and Create waits until the form can be sent, so
 * the form says what is missing rather than letting the runtime say it later.
 *
 * Every edit goes through the draft the list holds, so a field typed into
 * survives the form being unmounted by anything but a confirmed departure.
 */
export function CreateAgent({
  form,
  update,
  agents,
  catalogue,
  catalogueError,
  onCancel,
  onCreated,
}: {
  form: CreateForm;
  update: (next: (previous: CreateForm | null) => CreateForm | null) => void;
  /** The agents already on screen, to catch a name already taken; `null` while loading. */
  agents: readonly AgentSummary[] | null;
  catalogue: readonly ToolView[] | null;
  catalogueError: string | null;
  onCancel: () => void;
  onCreated: (name: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const titleId = useId();
  const nameNote = useId();
  const visionNote = useId();
  const nameField = useRef<HTMLInputElement>(null);

  // The form was opened to be filled in; start where filling in starts.
  useEffect(() => {
    nameField.current?.focus();
  }, []);

  const set = useCallback(
    <K extends keyof CreateForm>(field: K, value: CreateForm[K]) =>
      update((previous) => (previous === null ? previous : { ...previous, [field]: value })),
    [update],
  );

  const submit = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const created = await api.createAgent(createInput(form));
      onCreated(created.name);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  }, [form, onCreated]);

  const problem = nameProblem(form.name, agents);
  const progress = createProgress(form, agents);
  const highest = catalogue ? highestRisk(form.tools, catalogue) : null;

  return (
    <section className="panel create-agent" aria-labelledby={titleId}>
      <header className="create-head">
        <span className="create-mark" aria-hidden="true">
          <Icon name="sparkles" size={18} />
        </span>
        <div>
          <h2 id={titleId} className="create-title">
            New agent
          </h2>
          <p className="create-lede">
            Name it, choose the model it runs on, and offer it tools. Its policy decides every
            call it makes.
          </p>
        </div>
      </header>

      <ol className="create-steps">
        <Step number={1} title="Identity" done={progress.identity}>
          <div className="field">
            <label htmlFor="agent-name">Name</label>
            <input
              id="agent-name"
              ref={nameField}
              value={form.name}
              autoComplete="off"
              spellCheck={false}
              placeholder="sales"
              aria-invalid={problem !== null}
              aria-describedby={nameNote}
              onChange={(event) => set("name", event.target.value)}
            />
            <p
              id={nameNote}
              className={problem ? "field-note field-problem" : "field-note"}
              aria-live="polite"
            >
              {problem ?? "Its workspace folder takes this name."}
            </p>
          </div>
          <div className="field flush">
            <label htmlFor="agent-instructions">Instructions</label>
            <textarea
              id="agent-instructions"
              rows={3}
              value={form.instructions}
              onChange={(event) => set("instructions", event.target.value)}
            />
          </div>
        </Step>

        <Step number={2} title="Model" done={progress.model}>
          <fieldset className="field">
            <legend>Provider</legend>
            <div className="segmented provider-choice">
              {PROVIDERS.map((id) => (
                <button
                  key={id}
                  type="button"
                  aria-pressed={form.provider === id}
                  onClick={() => set("provider", id)}
                >
                  {id}
                </button>
              ))}
            </div>
          </fieldset>
          {/* Beside the provider it belongs to, so the note under the model
              and vision fields stays next to the field it describes. */}
          {takesBaseUrl(form.provider) ? (
            <div className="field">
              <label htmlFor="agent-base-url">Base URL (optional)</label>
              <input
                id="agent-base-url"
                value={form.baseUrl}
                placeholder="http://localhost:11434/v1"
                onChange={(event) => set("baseUrl", event.target.value)}
              />
            </div>
          ) : null}
          <div className="grid two">
            <div className="field">
              <label htmlFor="agent-model">Model</label>
              <input
                id="agent-model"
                value={form.model}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => set("model", event.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor="agent-vision">Vision</label>
              <select
                id="agent-vision"
                aria-describedby={visionNote}
                value={form.vision}
                onChange={(event) => set("vision", event.target.value as Vision)}
              >
                <option value="default">Provider default</option>
                <option value="yes">This model can see images</option>
                <option value="no">Text only</option>
              </select>
            </div>
          </div>
          <p id={visionNote} className="field-note step-note">
            Vision is whether this model can be shown a screenshot. Sending one is a separate
            grant (<code>computer.vision</code>, <code>browser.vision</code>) that the policy
            still decides.
          </p>
        </Step>

        <Step number={3} title="Tools" done={progress.tools}>
          <ToolPicker
            catalogue={catalogue}
            catalogueError={catalogueError}
            selected={form.tools}
            onToggle={(tool) =>
              update((previous) =>
                previous === null
                  ? previous
                  : { ...previous, tools: toggleTool(previous.tools, tool) },
              )
            }
          />
        </Step>
      </ol>

      {error ? (
        <div className="create-error">
          <ErrorBanner message={error} />
        </div>
      ) : null}

      <footer className="create-foot">
        <div className="create-summary">
          <span className="create-summary-line">
            <strong>{toolCountLabel(form.tools.length)}</strong> offered
            {highest !== null ? (
              <>
                <span aria-hidden="true"> · up to</span>
                <span className="visually-hidden">, highest</span> <Risk level={highest} />
              </>
            ) : null}
          </span>
          <span className="create-summary-note">
            <Icon name="shield" size={14} />
            Its starter policy denies everything except reading inside its own workspace.
          </span>
        </div>
        <div className="create-actions">
          <button type="button" className="ghost" onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className="primary"
            disabled={busy || !progress.ready}
            onClick={() => void submit()}
          >
            {busy ? "Creating…" : "Create agent"}
          </button>
        </div>
      </footer>
    </section>
  );
}

/**
 * One numbered step. The marker becomes a check when the step has what it
 * needs, and the heading says so in words for anyone who cannot see it.
 *
 * The marker pops when the step is completed, not when the form opens with a
 * step its defaults already satisfy: a pop says something just changed, and
 * nothing has.
 */
function Step({
  number,
  title,
  done,
  children,
}: {
  number: number;
  title: string;
  done: boolean;
  children: ReactNode;
}) {
  const { key, popping } = usePop(done ? 1 : 0);
  return (
    <li className={done ? "create-step done" : "create-step"}>
      <span className="step-rail" aria-hidden="true">
        <span key={key} className={done && popping ? "step-marker pop" : "step-marker"}>
          {done ? <Icon name="check" size={14} /> : number}
        </span>
      </span>
      <div className="step-body">
        <h3 className="step-title">
          {title}
          {done ? <span className="visually-hidden">, complete</span> : null}
        </h3>
        {children}
      </div>
    </li>
  );
}
