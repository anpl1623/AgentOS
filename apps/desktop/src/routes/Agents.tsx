import { type ReactNode, useCallback, useEffect, useId, useState } from "react";

import type { MemoryView } from "../bindings/MemoryView";
import type { PolicyView } from "../bindings/PolicyView";
import type { TaskSummary } from "../bindings/TaskSummary";
import type { ToolView } from "../bindings/ToolView";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { countHidden, hiddenWarning, visibleText } from "../components/argumentText";
import {
  Empty,
  Enabled,
  ErrorBanner,
  Loading,
  PageHeader,
  Risk,
  Row,
  SkeletonRows,
  Stale,
  Status,
  Tainted,
} from "../components/common";
import { cacheKey, useAsyncCached } from "../sdk/cache";
import { api, describeError } from "../sdk/client";
import { useDraft, useUnsavedGuard } from "../sdk/drafts";
import { ago, humanise } from "../sdk/format";
import { useAsync } from "../sdk/useAsync";
import {
  CREATE_GUARD,
  type CreateForm,
  type GrantRow,
  MEMORY_KINDS,
  POLICY_GUARD,
  PROVIDERS,
  type PolicyDraft,
  type Vision,
  beginPolicyDraft,
  confidenceLabel,
  createFormDirty,
  createInput,
  deadGrantText,
  filterAgents,
  filterMemories,
  grantRows,
  highestRisk,
  memoryRevision,
  newCreateForm,
  policyDirty,
  policySuperseded,
  reachLabel,
  reachTone,
  splitRule,
  takesBaseUrl,
  toggleTool,
} from "./agentsModel";
import type { Navigate, Route } from "./route";
import { truncate } from "./tasksModel";

export function Agents({
  route,
  navigate,
}: {
  route: Route & { name: "agents" };
  navigate: Navigate;
}) {
  if (route.agent) {
    return <AgentDetail name={route.agent} navigate={navigate} />;
  }
  return <AgentList navigate={navigate} />;
}

/**
 * A list of rows, named by its heading.
 *
 * ARIA roles rather than `<ul>` and `<li>`, as on the dashboard: the
 * stylesheets have no reset for a bare list. What a screen reader hears, the
 * name and the count, is the same.
 */
function List({
  labelledBy,
  label,
  children,
}: {
  labelledBy?: string;
  label?: string;
  children: ReactNode;
}) {
  return (
    <div role="list" aria-labelledby={labelledBy} aria-label={label}>
      {children}
    </div>
  );
}

/** One entry of a {@link List}. A `Row` that is a button cannot also be the item. */
function Item({ children }: { children: ReactNode }) {
  return <div role="listitem">{children}</div>;
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

function AgentList({ navigate }: { navigate: Navigate }) {
  const agents = useAsyncCached(cacheKey("list_agents"), () => api.listAgents());
  const catalogue = useAsyncCached(cacheKey("list_tools"), () => api.listTools());
  const [query, setQuery] = useState("");

  // The open form is the draft: `null` is closed. One object, so one guard
  // covers every field of it.
  const [form, setForm, discardForm] = useDraft<CreateForm | null>("agent:new", null);
  useUnsavedGuard(createFormDirty(form), CREATE_GUARD);
  // Leaving was either confirmed against the guard's "discard it?" or had
  // nothing to lose, so the draft goes with the screen. Kept, it would come
  // back on the next visit as a form the operator was told was thrown away.
  useEffect(() => discardForm, [discardForm]);

  // Navigating to the new agent waits for the commit that discards the form,
  // because the guard is unregistered by an effect; navigating from the click
  // handler would ask whether to discard a form that has just been created.
  const [created, setCreated] = useState<string | null>(null);
  useEffect(() => {
    if (created !== null) navigate({ name: "agents", agent: created });
  }, [created, navigate]);

  const shown = agents.data ? filterAgents(agents.data, query) : [];

  return (
    <>
      <PageHeader
        title="Agents"
        subtitle="Each agent has its own instructions, its own workspace, and its own policy."
        actions={
          <button
            type="button"
            className="primary"
            aria-expanded={form !== null}
            onClick={() => (form === null ? setForm(newCreateForm()) : discardForm())}
          >
            {form === null ? "New agent" : "Cancel"}
          </button>
        }
      />

      {form !== null ? (
        <CreateAgent
          form={form}
          update={setForm}
          catalogue={catalogue.data}
          catalogueError={catalogue.error}
          onCreated={(name) => {
            discardForm();
            agents.reload();
            setCreated(name);
          }}
        />
      ) : null}

      {agents.error ? <ErrorBanner message={agents.error} /> : null}
      <div className="toolbar">
        <input
          type="search"
          aria-label="Filter agents by name, provider or model"
          placeholder="Filter by name, provider or model"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
      </div>
      <h2 id="agents-list" className="visually-hidden">
        All agents
      </h2>
      <div className="panel">
        {agents.loading ? (
          <>
            <Loading what="agents" />
            <SkeletonRows count={2} />
          </>
        ) : null}
        {agents.data?.length === 0 ? <Empty>No agents yet.</Empty> : null}
        {agents.data && agents.data.length > 0 && shown.length === 0 ? (
          <Empty>No agent matches “{query.trim()}”.</Empty>
        ) : null}
        {shown.length > 0 ? (
          <List labelledBy="agents-list">
            {shown.map((agent) => {
              const highest = catalogue.data ? highestRisk(agent.tools, catalogue.data) : null;
              return (
                <Item key={agent.id}>
                  <Row onActivate={() => navigate({ name: "agents", agent: agent.name })}>
                    <div className="row-main">
                      <div className="row-title">{agent.name}</div>
                      <div className="row-meta">
                        <span>
                          {agent.provider}/{agent.model}
                        </span>
                        <span>
                          {agent.tools.length === 1 ? "1 tool" : `${agent.tools.length} tools`}
                        </span>
                        <span>created {ago(agent.created_at)}</span>
                      </div>
                    </div>
                    {highest !== null ? (
                      <span>
                        <span className="visually-hidden">highest granted </span>
                        <Risk level={highest} />
                      </span>
                    ) : null}
                    <Enabled status={agent.status} />
                  </Row>
                </Item>
              );
            })}
          </List>
        ) : null}
      </div>
    </>
  );
}

function CreateAgent({
  form,
  update,
  catalogue,
  catalogueError,
  onCreated,
}: {
  form: CreateForm;
  update: (next: (previous: CreateForm | null) => CreateForm | null) => void;
  catalogue: readonly ToolView[] | null;
  catalogueError: string | null;
  onCreated: (name: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const visionNote = useId();

  // Every edit goes through the draft, so a field typed into survives the
  // form being unmounted by anything but a confirmed departure.
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

  return (
    <div className="panel form">
      <div className="panel-body">
        <div className="field">
          <label htmlFor="agent-name">Name</label>
          <input
            id="agent-name"
            value={form.name}
            onChange={(event) => set("name", event.target.value)}
            placeholder="sales"
          />
        </div>

        <div className="field">
          <label htmlFor="agent-instructions">Instructions</label>
          <textarea
            id="agent-instructions"
            rows={3}
            value={form.instructions}
            onChange={(event) => set("instructions", event.target.value)}
          />
        </div>

        <div className="grid two">
          <div className="field">
            <label htmlFor="agent-provider">Provider</label>
            <select
              id="agent-provider"
              value={form.provider}
              onChange={(event) => set("provider", event.target.value)}
            >
              {PROVIDERS.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="agent-model">Model</label>
            <input
              id="agent-model"
              value={form.model}
              onChange={(event) => set("model", event.target.value)}
            />
          </div>
        </div>

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
          <p id={visionNote} className="field-note">
            Whether this model can be shown a screenshot. Sending one is a separate grant (
            <code>computer.vision</code>, <code>browser.vision</code>) that the policy still
            decides.
          </p>
        </div>

        <fieldset className="field">
          <legend>Tools</legend>
          <p className="field-note">
            Granting a tool only offers it to the model; the policy still decides each call.
          </p>
          {catalogueError ? (
            <ErrorBanner message={`The tool catalogue could not be read: ${catalogueError}`} />
          ) : null}
          {catalogue === null && catalogueError === null ? <Loading what="tools" /> : null}
          <div className="checks">
            {catalogue?.map((tool) => {
              const on = form.tools.includes(tool.name);
              return (
                <label key={tool.name} className={on ? "check on" : "check"}>
                  <input
                    type="checkbox"
                    checked={on}
                    onChange={() =>
                      update((previous) =>
                        previous === null
                          ? previous
                          : { ...previous, tools: toggleTool(previous.tools, tool.name) },
                      )
                    }
                  />
                  <span>
                    <span className="check-name">{tool.name}</span>
                    <span className="check-note">
                      {tool.risk} risk
                      {tool.returns_untrusted_data ? " · reads external data" : ""}
                    </span>
                  </span>
                </label>
              );
            })}
          </div>
        </fieldset>

        {error ? <ErrorBanner message={error} /> : null}

        <div className="inline">
          <button
            type="button"
            className="primary"
            disabled={busy || form.name.trim() === ""}
            onClick={() => void submit()}
          >
            {busy ? "Creating…" : "Create agent"}
          </button>
          <span className="faint">
            Its starter policy denies everything except reading inside its own workspace.
          </span>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

function AgentDetail({ name, navigate }: { name: string; navigate: Navigate }) {
  const agent = useAsyncCached(cacheKey("get_agent", name), () => api.getAgent(name), [name]);
  const catalogue = useAsyncCached(cacheKey("list_tools"), () => api.listTools());
  const id = agent.data?.summary.id ?? null;
  // Not cached: the report is the engine's reading of the policy as stored
  // now, and an answer carried over from an earlier visit could describe a
  // policy that has since been replaced.
  const grants = useAsync(() => (id === null ? Promise.resolve(null) : api.grantReport(id)), [id]);

  // When the data on screen was read, for the stale marker. Set on each new
  // answer rather than each render, so a failed refresh leaves it where it was.
  const [loadedAt, setLoadedAt] = useState(() => Date.now());
  const loaded = agent.data;
  useEffect(() => {
    if (loaded !== null) setLoadedAt(Date.now());
  }, [loaded]);

  const [toggling, setToggling] = useState(false);
  const [toggleError, setToggleError] = useState<string | null>(null);

  const reloadAgent = agent.reload;
  const reloadGrants = grants.reload;
  const onPolicySaved = useCallback(() => {
    reloadAgent();
    reloadGrants();
  }, [reloadAgent, reloadGrants]);

  const parent = { label: "Agents", onActivate: () => navigate({ name: "agents" }) };

  if (agent.data === null) {
    return (
      <>
        <PageHeader title={name} parent={parent} />
        {agent.error ? <ErrorBanner message={agent.error} /> : <Loading what={name} />}
      </>
    );
  }

  const { summary, instructions, policy, recent_tasks, workspace } = agent.data;
  const rows = grantRows(summary.tools, catalogue.data, grants.data, policy);

  const toggle = async () => {
    setToggling(true);
    setToggleError(null);
    try {
      await api.setAgentEnabled(summary.name, summary.status !== "enabled");
      agent.reload();
    } catch (failure) {
      setToggleError(describeError(failure));
    } finally {
      setToggling(false);
    }
  };

  return (
    <>
      <PageHeader
        title={summary.name}
        parent={parent}
        subtitle={
          <>
            {summary.provider}/{summary.model} · {summary.max_steps} steps per run
            {agent.stale ? (
              <>
                {" · "}
                <Stale since={loadedAt} />
              </>
            ) : null}
          </>
        }
      />
      {agent.error ? <ErrorBanner message={agent.error} /> : null}

      <div className="inline spaced">
        <Enabled status={summary.status} />
        <button type="button" disabled={toggling} onClick={() => void toggle()}>
          {summary.status === "enabled" ? "Disable" : "Enable"}
        </button>
      </div>
      {toggleError ? <ErrorBanner message={toggleError} /> : null}

      <h2>Instructions</h2>
      <div className="panel">
        <div className="panel-body prose">{instructions}</div>
      </div>

      <h2 id="agent-tools">Tools</h2>
      <p className="field-note">
        Granting a tool offers it to the model; the policy decides each call. Beside each
        capability is how far the permission engine reads the saved policy as letting it reach.
        That reading can overstate and never understates: Allowed means allowed at the tool’s
        usual risk, Scoped that some resources may be allowed or asked about, Asks that every call
        waits for approval, and Denied that every call is refused.
      </p>
      {grants.error ? (
        <ErrorBanner message={`How far this policy reaches could not be read: ${grants.error}`} />
      ) : null}
      {catalogue.error ? (
        <ErrorBanner message={`The tool catalogue could not be read: ${catalogue.error}`} />
      ) : null}
      <div className="panel">
        {rows.length === 0 ? (
          <Empty>No tools granted.</Empty>
        ) : (
          <List labelledBy="agent-tools">
            {rows.map((row) => (
              <Item key={row.tool}>
                <GrantLine row={row} policy={policy} />
              </Item>
            ))}
          </List>
        )}
      </div>

      <h2>Permissions</h2>
      <PolicyEditor key={summary.id} agentId={summary.id} policy={policy} onSaved={onPolicySaved} />

      <MemoryPanel agentId={summary.id} />

      <h2>Workspace</h2>
      <div className="panel">
        <div className="panel-body mono muted">{workspace}</div>
      </div>

      <h2 id="agent-recent">Recent tasks</h2>
      <div className="panel">
        {recent_tasks.length === 0 ? (
          <Empty>Nothing yet.</Empty>
        ) : (
          <List labelledBy="agent-recent">
            {recent_tasks.map((task) => (
              <Item key={task.id}>
                <RecentTask task={task} navigate={navigate} />
              </Item>
            ))}
          </List>
        )}
      </div>
    </>
  );
}

/**
 * One granted tool: what it is, what it may cost, and how far the policy lets
 * each of its capabilities reach. A grant every call of which will be refused
 * says so on the row, in danger, with the reason.
 */
function GrantLine({ row, policy }: { row: GrantRow; policy: PolicyView | null }) {
  const dead = deadGrantText(row, policy);
  return (
    <Row>
      <div className="row-main">
        <div className="row-title mono">{row.tool}</div>
        {row.description ? (
          <div className="row-meta">
            <span>{row.description}</span>
          </div>
        ) : null}
        {row.capabilities.length > 0 ? (
          <div className="row-meta mono">
            {row.capabilities.map(({ capability, reach }) => (
              <span key={capability}>
                {capability}
                {reach !== null ? (
                  <>
                    {" "}
                    <span className={`verdict ${reachTone(reach)}`}>
                      <span className="visually-hidden">reach </span>
                      {reachLabel(reach)}
                    </span>
                  </>
                ) : null}
              </span>
            ))}
          </div>
        ) : null}
        {dead ? (
          <div className="row-meta">
            <span>{dead.sentence}</span>
          </div>
        ) : null}
      </div>
      {row.risk !== null ? <Risk level={row.risk} /> : null}
      {row.external ? (
        <span className="verdict warn">
          <span className="visually-hidden">reads </span>external
        </span>
      ) : null}
      {dead ? (
        <span className="verdict danger">
          <span className="visually-hidden">grant </span>
          {dead.label}
        </span>
      ) : null}
    </Row>
  );
}

/** A recent task. One that never ran has no trace to open, so it is not a button. */
function RecentTask({ task, navigate }: { task: TaskSummary; navigate: Navigate }) {
  const run = task.latest_run;
  return (
    <Row onActivate={run ? () => navigate({ name: "tasks", runId: run.id }) : undefined}>
      <div className="row-main">
        <div className="row-title">{task.objective}</div>
        <div className="row-meta">
          <span>{ago(task.created_at)}</span>
        </div>
      </div>
      {run ? null : (
        <span className="verdict never">
          <span className="visually-hidden">task </span>Never ran
        </span>
      )}
      <Status status={task.status} />
    </Row>
  );
}

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

function PolicyEditor({
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
        const count = result.summary.rules.length;
        const rules = count === 1 ? "1 rule" : `${count} rules`;
        setChecked({
          document,
          text: `Valid · default ${result.summary.default_effect} · ${rules}`,
        });
      } else {
        setError(result.error ?? "This policy does not compile.");
      }
    } catch (failure) {
      setError(describeError(failure));
    }
  }, [document]);

  if (!policy) {
    return (
      <div className="panel">
        <div className="panel-body">
          <div className="banner warn flush">
            This agent has no policy, so every action is denied.
          </div>
        </div>
      </div>
    );
  }

  if (draft === null) {
    return (
      <div className="panel">
        <div className="panel-body">
          <div className="inline">
            <span className="muted">version {policy.version}</span>
            <span className="muted">
              default <b>{policy.default_effect}</b>
            </span>
            {policy.max_risk ? (
              <span className="muted">
                ceiling <Risk level={policy.max_risk} />
              </span>
            ) : null}
            {policy.taint_enabled ? (
              <span className="muted">
                after reading untrusted data, <b>{policy.taint_threshold}</b>+ needs approval
              </span>
            ) : (
              <span className="verdict warn">Taint escalation off</span>
            )}
            <span className="right" />
            <button type="button" onClick={() => setDraft(beginPolicyDraft(policy))}>
              Edit
            </button>
          </div>
        </div>
        {policy.rules.length === 0 ? (
          <Empty>No rules. The default decides every call.</Empty>
        ) : (
          <List label="Rules">
            {policy.rules.map((rule, index) => {
              const line = splitRule(rule);
              return (
                <Item key={`${index}:${rule}`}>
                  <Row>
                    <div className="row-main mono">{line.capability}</div>
                    <span className="mono muted">{line.effect}</span>
                  </Row>
                </Item>
              );
            })}
          </List>
        )}
      </div>
    );
  }

  return (
    <div className="panel">
      <div className="panel-body">
        {superseded && dirty ? (
          <div className="banner warn">
            This policy was saved elsewhere after you began editing version {draft.baseVersion}; it
            is now version {policy.version}. Saving replaces that version with the text below.
          </div>
        ) : null}
        <div className="field">
          <label htmlFor="policy">Policy (YAML)</label>
          <textarea
            id="policy"
            rows={16}
            spellCheck={false}
            value={document}
            onChange={(event) => {
              const next = event.target.value;
              setDraft((previous) =>
                previous === null ? previous : { ...previous, document: next },
              );
            }}
          />
        </div>
        <div role="status">
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
            Check
          </button>
          <button
            type="button"
            className="ghost"
            onClick={() => {
              discardDraft();
              setError(null);
              setChecked(null);
            }}
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/** A memory being revised in place. */
interface MemoryEdit {
  id: string;
  content: string;
  confidence: number;
}

/** The confidence slider's step. */
const CONFIDENCE_STEP = 0.05;

/**
 * What the agent remembers, which shapes the prompt of every run it makes.
 *
 * Content and source are drawn through `visibleText`: a memory can be recorded
 * from a page the agent read, and an invisible or direction-changing character
 * in it would otherwise reach the operator as the character itself.
 */
function MemoryPanel({ agentId }: { agentId: string }) {
  const memories = useAsync(() => api.listMemories(agentId), [agentId]);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<string | null>(null);
  const [editing, setEditing] = useState<MemoryEdit | null>(null);
  const [forgetting, setForgetting] = useState<MemoryView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
    <>
      <h2 id="agent-memory">Memory</h2>
      <RememberForm agentId={agentId} onRemembered={memories.reload} />

      <div className="toolbar">
        <input
          type="search"
          aria-label="Search memories"
          placeholder="Search content or source"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <div className="segmented" role="group" aria-label="Kind">
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
        {memories.loading ? <Loading what="memories" /> : null}
        {memories.data?.length === 0 ? <Empty>This agent remembers nothing yet.</Empty> : null}
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
                    onCancel={() => setEditing(null)}
                    onSave={(content, confidence) => {
                      const revision = memoryRevision(memory, content, confidence);
                      if (revision === null) {
                        setEditing(null);
                        return;
                      }
                      void act(() =>
                        api.reviseMemory(memory.id, revision.content, revision.confidence),
                      ).then((ok) => {
                        if (ok) setEditing(null);
                      });
                    }}
                  />
                ) : (
                  <MemoryLine
                    memory={memory}
                    busy={busy}
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
            if (confirmed) void act(() => api.forgetMemory(memory.id));
          }}
        />
      ) : null}
    </>
  );
}

function MemoryLine({
  memory,
  busy,
  onEdit,
  onForget,
}: {
  memory: MemoryView;
  busy: boolean;
  onEdit: () => void;
  onForget: () => void;
}) {
  const warning = hiddenWarning(countHidden(memory.content) + countHidden(memory.source));
  // Every row has an Edit and a Forget; a list of buttons read aloud needs to
  // say which memory each one is for.
  const named = truncate(visibleText(memory.content), 40);
  return (
    <Row className={memory.reaches_the_prompt ? undefined : "dimmed"}>
      <div className="row-main">
        <div className="row-title prose">{visibleText(memory.content)}</div>
        <div className="row-meta">
          <span className="mono">{visibleText(memory.source)}</span>
          <span>{ago(memory.updated_at)}</span>
          <span>{confidenceLabel(memory.confidence)}</span>
          {memory.source_untrusted ? (
            <Tainted
              label="recorded from an outside source"
              explanation="This memory was recorded from data outside the trust boundary."
            />
          ) : null}
        </div>
        {memory.reaches_the_prompt ? null : (
          <div className="row-meta">
            <span>
              Kept, but not retrieved before planning: the model is not shown it up front.
            </span>
          </div>
        )}
        {warning ? (
          <div className="row-meta">
            <span>{warning}</span>
          </div>
        ) : null}
      </div>
      <span className="verdict neutral">
        <span className="visually-hidden">kind </span>
        {humanise(memory.kind)}
      </span>
      <button type="button" disabled={busy} onClick={onEdit}>
        Edit<span className="visually-hidden"> {named}</span>
      </button>
      <button type="button" disabled={busy} onClick={onForget}>
        Forget<span className="visually-hidden"> {named}</span>
      </button>
    </Row>
  );
}

function MemoryEditor({
  memory,
  draft,
  busy,
  onChange,
  onCancel,
  onSave,
}: {
  memory: MemoryView;
  draft: MemoryEdit;
  busy: boolean;
  onChange: (next: MemoryEdit) => void;
  onCancel: () => void;
  onSave: (content: string, confidence: number) => void;
}) {
  const contentId = useId();
  const confidenceId = useId();
  return (
    <Row className="stacked">
      <div className="field">
        <label htmlFor={contentId}>Revise this {humanise(memory.kind)}</label>
        <textarea
          id={contentId}
          rows={3}
          value={draft.content}
          onChange={(event) => onChange({ ...draft, content: event.target.value })}
        />
      </div>
      <ConfidenceField
        id={confidenceId}
        value={draft.confidence}
        onChange={(confidence) => onChange({ ...draft, confidence })}
      />
      <div className="inline">
        <button
          type="button"
          className="primary"
          disabled={busy || draft.content.trim() === ""}
          onClick={() => onSave(draft.content, draft.confidence)}
        >
          {busy ? "Saving…" : "Save"}
        </button>
        <button type="button" className="ghost" onClick={onCancel}>
          Cancel
        </button>
      </div>
    </Row>
  );
}

function ConfidenceField({
  id,
  value,
  onChange,
}: {
  id: string;
  value: number;
  onChange: (value: number) => void;
}) {
  const label = confidenceLabel(value);
  return (
    <div className="field">
      <label htmlFor={id}>Confidence</label>
      <div className="inline">
        <input
          id={id}
          type="range"
          min={0}
          max={1}
          step={CONFIDENCE_STEP}
          value={value}
          aria-valuetext={label}
          onChange={(event) => onChange(Number(event.target.value))}
        />
        <output htmlFor={id} className="muted">
          {label}
        </output>
      </div>
    </div>
  );
}

function RememberForm({ agentId, onRemembered }: { agentId: string; onRemembered: () => void }) {
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
    <div className="panel form">
      <div className="panel-body">
        <div className="grid two">
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
        <div className="field">
          <label htmlFor={contentId}>Remember</label>
          <textarea
            id={contentId}
            rows={2}
            value={content}
            onChange={(event) => setContent(event.target.value)}
          />
        </div>
        {error ? <ErrorBanner message={error} /> : null}
        <div className="inline">
          <button
            type="button"
            className="primary"
            disabled={busy || content.trim() === ""}
            onClick={() => void submit()}
          >
            {busy ? "Saving…" : "Remember"}
          </button>
          <span className="faint">Recorded as your own note, whatever it says.</span>
        </div>
      </div>
    </div>
  );
}
