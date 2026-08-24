import { useCallback, useState } from "react";

import { ErrorBanner, Loading, Risk } from "../components/common";
import { api, describeError } from "../sdk/client";
import { useAsync } from "../sdk/useAsync";

export function Settings() {
  const settings = useAsync(() => api.settings(), []);
  const [verification, setVerification] = useState<string[] | null>(null);
  const [verifying, setVerifying] = useState(false);

  const verify = useCallback(async () => {
    setVerifying(true);
    try {
      setVerification(await api.verifyAudit());
    } finally {
      setVerifying(false);
    }
  }, []);

  if (settings.loading && !settings.data) return <Loading what="settings" />;
  if (settings.error) return <ErrorBanner message={settings.error} />;
  if (!settings.data) return null;

  const data = settings.data;

  return (
    <>
      <div className="page-head">
        <h1>Settings</h1>
      </div>
      <p className="page-sub">
        Everything AgentOS knows lives on this machine. Nothing here is uploaded anywhere.
      </p>

      <h2>Model providers</h2>
      {!data.keychain_available ? (
        <div className="banner warn">
          This machine has no usable keychain{data.keychain_reason ? ` (${data.keychain_reason})` : ""},
          so credentials must come from the environment. An agent cannot read them back — child
          processes get an allowlist that excludes them.
        </div>
      ) : null}
      <div className="panel">
        {data.providers.map((provider) => (
          <ProviderRow
            key={provider.id}
            provider={provider}
            keychain={data.keychain_available}
            onChanged={settings.reload}
          />
        ))}
      </div>

      <h2>Browser</h2>
      <div className="panel">
        <div className="panel-body">
          {data.browser_path ? (
            <div className="mono muted">{data.browser_path}</div>
          ) : (
            <div className="banner warn" style={{ marginBottom: 0, whiteSpace: "pre-wrap" }}>
              {data.browser_hint}
            </div>
          )}
        </div>
      </div>

      <h2>Audit log</h2>
      <div className="panel">
        <div className="panel-body">
          <div className="inline">
            <button type="button" disabled={verifying} onClick={() => void verify()}>
              {verifying ? "Verifying…" : "Verify the chain"}
            </button>
            {verification === null ? (
              <span className="faint">
                Recomputes every hash and checks each record still points at the one before it.
              </span>
            ) : verification.length === 0 ? (
              <span className="badge ok">intact</span>
            ) : (
              <span className="badge critical">{verification.length} problems</span>
            )}
          </div>
          {verification && verification.length > 0 ? (
            <div className="stack" style={{ marginTop: 12 }}>
              {verification.map((problem) => (
                <div key={problem} className="banner error" style={{ marginBottom: 0 }}>
                  {problem}
                </div>
              ))}
            </div>
          ) : null}
        </div>
      </div>

      <h2>Storage</h2>
      <div className="panel">
        <div className="panel-body">
          <dl className="facts" style={{ marginBottom: 0 }}>
            <dt>Data directory</dt>
            <dd className="mono">{data.data_dir}</dd>
            <dt>Workspaces</dt>
            <dd className="mono">{data.workspace}</dd>
            <dt>Database</dt>
            <dd className="mono">{data.database}</dd>
          </dl>
        </div>
      </div>

      <h2>Tools</h2>
      <div className="panel">
        {data.tools.map((tool) => (
          <div key={tool.name} className="row">
            <div className="row-main">
              <div className="row-title mono">{tool.name}</div>
              <div className="row-meta">
                <span>{tool.description}</span>
              </div>
            </div>
            {tool.returns_untrusted_data ? (
              <span className="badge medium" title="Output can be attacker-controlled, so it raises the approval bar for the rest of the run.">
                external
              </span>
            ) : null}
            <Risk level={tool.risk} />
          </div>
        ))}
      </div>
    </>
  );
}

function ProviderRow({
  provider,
  keychain,
  onChanged,
}: {
  provider: { id: string; configured: boolean; hint: string | null; source: string | null; note: string };
  keychain: boolean;
  onChanged: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await api.setProviderKey(provider.id, key);
      setKey("");
      setEditing(false);
      onChanged();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setBusy(false);
    }
  }, [provider.id, key, onChanged]);

  return (
    <div className="row" style={{ display: "block" }}>
      <div className="inline">
        <div className="row-main">
          <div className="row-title">{provider.id}</div>
          <div className="row-meta">
            {provider.configured ? (
              <span>
                {provider.hint} · via {provider.source}
              </span>
            ) : (
              <span>{provider.note}</span>
            )}
          </div>
        </div>
        {provider.configured ? <span className="badge ok">set</span> : <span className="badge neutral">not set</span>}
        {keychain ? (
          <button type="button" className="ghost" onClick={() => setEditing((open) => !open)}>
            {editing ? "Cancel" : provider.configured ? "Replace" : "Add key"}
          </button>
        ) : null}
        {provider.configured && provider.source === "system keychain" ? (
          <button
            type="button"
            className="ghost"
            onClick={() => void api.removeProviderKey(provider.id).then(onChanged)}
          >
            Remove
          </button>
        ) : null}
      </div>

      {editing ? (
        <div style={{ marginTop: 10 }}>
          <div className="field">
            <label htmlFor={`key-${provider.id}`}>
              API key · stored in the operating system keychain, never in the database or a log
            </label>
            <input
              id={`key-${provider.id}`}
              type="password"
              value={key}
              autoComplete="off"
              onChange={(event) => setKey(event.target.value)}
              placeholder="Paste the key"
            />
          </div>
          {error ? <ErrorBanner message={error} /> : null}
          <button type="button" className="primary" disabled={busy || key.trim() === ""} onClick={() => void save()}>
            {busy ? "Storing…" : "Store key"}
          </button>
        </div>
      ) : null}
    </div>
  );
}
