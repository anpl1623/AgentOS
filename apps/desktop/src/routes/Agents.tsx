import { useCallback, useState } from "react";

import { Empty, Enabled, ErrorBanner, Loading, Risk, Status } from "../components/common";
import { api, describeError } from "../sdk/client";
import { ago } from "../sdk/format";
import { useAsync } from "../sdk/useAsync";
import type { Navigate, Route } from "./route";

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

function AgentList({ navigate }: { navigate: Navigate }) {
  const agents = useAsync(() => api.listAgents(), []);
  const [creating, setCreating] = useState(false);

  return (
    <>
      <div className="page-head">
        <h1>Agents</h1>
        <button type="button" className="primary" onClick={() => setCreating((open) => !open)}>
          {creating ? "Cancel" : "New agent"}
        </button>
      </div>
      <p className="page-sub">
        Each agent has its own instructions, its own workspace, and its own policy.
      </p>

      {creating ? (
        <CreateAgent
          onCreated={(name) => {
            setCreating(false);
            agents.reload();
            navigate({ name: "agents", agent: name });
          }}
        />
      ) : null}

      {agents.error ? <ErrorBanner message={agents.error} /> : null}
      <div className="panel">
        {agents.loading && !agents.data ? <Loading what="agents" /> : null}
        {agents.data?.length === 0 ? <Empty>No agents yet.</Empty> : null}
        {agents.data?.map((agent) => (
          <div
            key={agent.id}
            className="row clickable"
            onClick={() => navigate({ name: "agents", agent: agent.name })}
          >
            <div className="row-main">
              <div className="row-title">{agent.name}</div>
              <div className="row-meta">
                <span>
                  {agent.provider}/{agent.model}
                </span>
                <span>{agent.tools.length} tools</span>
                <span>created {ago(agent.created_at)}</span>
              </div>
            </div>
            <Enabled status={agent.status} />
          </div>
        ))}
      </div>
    </>
  );
}

const PROVIDERS = ["anthropic", "openai", "ollama", "mock"];

function CreateAgent({ onCreated }: { onCreated: (name: string) => void }) {
  const tools = useAsync(() => api.listTools(), []);
  const [name, setName] = useState("");
  const [instructions, setInstructions] = useState(
    "Complete the operator's objective carefully and report what you did.",
  );
  const [provider, setProvider] = useState("anthropic");
  const [model, setModel] = useState("claude-opus-5");
  const [baseUrl, setBaseUrl] = useState("");
  const [granted, setGranted] = useState<string[]>(["filesystem.read", "filesystem.list"]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const toggle = useCallback((tool: string) => {
    setGranted((current) =>
      current.includes(tool) ? current.filter((name) => name !== tool) : [...current, tool],
    );
  }, []);

  const submit = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const created = await api.createAgent({
        name: name.trim(),
        instructions,
        provider,
        model,
        base_url: baseUrl.trim() === "" ? null : baseUrl.trim(),
        tools: granted,
      });
      onCreated(created.name);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  }, [name, instructions, provider, model, baseUrl, granted, onCreated]);

  return (
    <div className="panel" style={{ marginBottom: 16 }}>
      <div className="panel-body">
        <div className="field">
          <label htmlFor="name">Name</label>
          <input id="name" value={name} onChange={(event) => setName(event.target.value)} placeholder="sales" />
        </div>

        <div className="field">
          <label htmlFor="instructions">Instructions</label>
          <textarea
            id="instructions"
            rows={3}
            value={instructions}
            onChange={(event) => setInstructions(event.target.value)}
          />
        </div>

        <div className="grid two">
          <div className="field">
            <label htmlFor="provider">Provider</label>
            <select id="provider" value={provider} onChange={(event) => setProvider(event.target.value)}>
              {PROVIDERS.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label htmlFor="model">Model</label>
            <input id="model" value={model} onChange={(event) => setModel(event.target.value)} />
          </div>
        </div>

        {provider === "openai" || provider === "ollama" ? (
          <div className="field">
            <label htmlFor="base-url">Base URL (optional)</label>
            <input
              id="base-url"
              value={baseUrl}
              placeholder="http://localhost:11434/v1"
              onChange={(event) => setBaseUrl(event.target.value)}
            />
          </div>
        ) : null}

        <div className="field">
          <label>
            Tools · granting one only offers it to the model; the policy still decides each call
          </label>
          <div className="checks">
            {tools.data?.map((tool) => (
              <label key={tool.name} className={`check ${granted.includes(tool.name) ? "on" : ""}`}>
                <input
                  type="checkbox"
                  checked={granted.includes(tool.name)}
                  onChange={() => toggle(tool.name)}
                />
                <span>
                  <span className="check-name">{tool.name}</span>
                  <span className="check-note">
                    {tool.risk} risk
                    {tool.returns_untrusted_data ? " · reads external data" : ""}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </div>

        {error ? <ErrorBanner message={error} /> : null}

        <div className="inline">
          <button type="button" className="primary" disabled={busy || name.trim() === ""} onClick={() => void submit()}>
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

function AgentDetail({ name, navigate }: { name: string; navigate: Navigate }) {
  const agent = useAsync(() => api.getAgent(name), [name]);

  if (agent.loading && !agent.data) return <Loading what={name} />;
  if (agent.error) return <ErrorBanner message={agent.error} />;
  if (!agent.data) return null;

  const { summary, instructions, policy, recent_tasks, workspace } = agent.data;

  return (
    <>
      <div className="page-head">
        <h1>{summary.name}</h1>
        <button type="button" className="ghost" onClick={() => navigate({ name: "agents" })}>
          ← All agents
        </button>
      </div>
      <p className="page-sub">
        {summary.provider}/{summary.model} · {summary.max_steps} steps per run
      </p>

      <div className="inline" style={{ marginBottom: 16 }}>
        <Enabled status={summary.status} />
        <button
          type="button"
          onClick={() => {
            void api.setAgentEnabled(summary.name, summary.status !== "enabled").then(agent.reload);
          }}
        >
          {summary.status === "enabled" ? "Disable" : "Enable"}
        </button>
      </div>

      <h2>Instructions</h2>
      <div className="panel">
        <div className="panel-body" style={{ whiteSpace: "pre-wrap" }}>
          {instructions}
        </div>
      </div>

      <h2>Tools</h2>
      <div className="panel">
        <div className="panel-body inline">
          {summary.tools.length === 0 ? (
            <span className="muted">None granted.</span>
          ) : (
            summary.tools.map((tool) => (
              <span key={tool} className="tag">
                {tool}
              </span>
            ))
          )}
        </div>
      </div>

      <h2>Permissions</h2>
      <PolicyEditor agentId={summary.id} policy={policy} onSaved={agent.reload} />

      <h2>Workspace</h2>
      <div className="panel">
        <div className="panel-body mono muted">{workspace}</div>
      </div>

      <h2>Recent tasks</h2>
      <div className="panel">
        {recent_tasks.length === 0 ? <Empty>Nothing yet.</Empty> : null}
        {recent_tasks.map((task) => (
          <div
            key={task.id}
            className="row clickable"
            onClick={() =>
              task.latest_run ? navigate({ name: "tasks", runId: task.latest_run.id }) : undefined
            }
          >
            <div className="row-main">
              <div className="row-title">{task.objective}</div>
              <div className="row-meta">
                <span>{ago(task.created_at)}</span>
              </div>
            </div>
            <Status status={task.status} />
          </div>
        ))}
      </div>
    </>
  );
}

function PolicyEditor({
  agentId,
  policy,
  onSaved,
}: {
  agentId: string;
  policy: AgentPolicy | null;
  onSaved: () => void;
}) {
  const [document, setDocument] = useState(policy?.document ?? "");
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState<string | null>(null);

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await api.setPolicy(agentId, document);
      setEditing(false);
      setChecked(null);
      onSaved();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  }, [agentId, document, onSaved]);

  const check = useCallback(async () => {
    setError(null);
    const result = await api.checkPolicy(document);
    if (result.valid && result.summary) {
      setChecked(
        `Valid · default ${result.summary.default_effect} · ${result.summary.rules.length} rules`,
      );
    } else {
      setChecked(null);
      setError(result.error ?? "This policy does not compile.");
    }
  }, [document]);

  if (!policy) {
    return (
      <div className="panel">
        <div className="panel-body">
          <div className="banner warn" style={{ marginBottom: 0 }}>
            This agent has no policy, so every action is denied.
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="panel">
      <div className="panel-body">
        {!editing ? (
          <>
            <div className="inline" style={{ marginBottom: 10 }}>
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
                <span className="badge medium">taint escalation off</span>
              )}
              <span className="right" />
              <button type="button" onClick={() => setEditing(true)}>
                Edit
              </button>
            </div>
            <div className="stack">
              {policy.rules.map((rule) => (
                <div key={rule} className="mono muted">
                  {rule}
                </div>
              ))}
            </div>
          </>
        ) : (
          <>
            <div className="field">
              <label htmlFor="policy">Policy (YAML)</label>
              <textarea
                id="policy"
                rows={16}
                value={document}
                onChange={(event) => setDocument(event.target.value)}
              />
            </div>
            {checked ? <div className="banner info">{checked}</div> : null}
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
                  setDocument(policy.document);
                  setEditing(false);
                  setError(null);
                  setChecked(null);
                }}
              >
                Cancel
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

type AgentPolicy = NonNullable<Awaited<ReturnType<typeof api.getAgent>>["policy"]>;
