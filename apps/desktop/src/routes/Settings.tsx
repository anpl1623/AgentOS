import { type ReactNode, useCallback, useEffect, useId, useState } from "react";

import type { AgentSummary } from "../bindings/AgentSummary";
import type { IntegrationTestView } from "../bindings/IntegrationTestView";
import type { IntegrationView } from "../bindings/IntegrationView";
import type { ProviderView } from "../bindings/ProviderView";
import type { SchedulerView } from "../bindings/SchedulerView";
import type { ToolView } from "../bindings/ToolView";
import { ErrorBanner, Loading, PageHeader, Risk, Row, SkeletonRows } from "../components/common";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { useCopy, useNow } from "../components/hooks";
import { cacheKey, useAsyncCached } from "../sdk/cache";
import { api, describeError } from "../sdk/client";
import { useUnsavedGuard } from "../sdk/drafts";
import { useVisibleInterval } from "../sdk/live";
import { useAsync } from "../sdk/useAsync";
import type { Navigate } from "./route";
import {
  type AccountRow,
  type CatalogueEntry,
  type CredentialRow,
  type Verification,
  accountRows,
  bindReady,
  catalogue,
  countEntries,
  credentialReady,
  credentialRows,
  grantLine,
  hostArgument,
  labelProblem,
  offersPrivateNetwork,
  readPacing,
  schedulerFacts,
  schedulerNotices,
  schedulerSummary,
  testLine,
  verificationAnnouncement,
  verificationLine,
} from "./settingsModel";

/**
 * How often the scheduler's counts are re-read while the screen is visible.
 *
 * The scheduler can stop on its own, and schedules come due while the screen
 * is open; a panel that only read once would keep saying "running" over a
 * scheduler that is not.
 */
const SCHEDULER_MS = 10_000;

/**
 * The last full verification of the audit chain, kept for the life of the
 * window rather than the life of the screen.
 *
 * The line under the button answers "have I checked at all since I opened
 * this?", and leaving Settings for a moment must not reset that to "no".
 * Held in memory only: a result read back from disk after a restart would be
 * a claim about a chain that may since have changed.
 */
let lastVerification: Verification | null = null;

/**
 * The machine-level controls, and the one question only this screen answers:
 * who on this machine has been granted each tool.
 *
 * Ordered by what an operator comes here to change. Secrets first: the model
 * providers', the integration accounts agents act as, and then the ones bound
 * to a network origin; then the browser and the scheduler, which decide what can run at all; then the evidence and
 * where it is kept; then the catalogue.
 */
export function Settings(_props: { navigate?: Navigate | undefined }) {
  const settings = useAsyncCached(cacheKey("settings"), () => api.settings());
  const agents = useAsyncCached(cacheKey("list_agents"), () => api.listAgents());
  const data = settings.data;

  // Nothing until the first answer, so the panels that load on their own do
  // not paint first and then slide down under the ones that waited.
  if (data === null && settings.loading) return <Loading what="settings" />;

  return (
    <>
      <PageHeader
        title="Settings"
        subtitle={
          "Everything AgentOS knows lives on this machine. Nothing here is uploaded anywhere."
        }
      />

      {/* Content whenever there is some; the bare banner only when there is
          nothing to show beneath it. */}
      {settings.error ? (
        <ErrorBanner
          message={
            data ? `Settings could not be refreshed: ${settings.error}` : settings.error
          }
        />
      ) : null}

      {data ? (
        <Section id="settings-providers" title="Model providers">
          {!data.keychain_available ? (
            <div className="banner warn" role="status">
              This machine has no usable keychain
              {data.keychain_reason ? ` (${data.keychain_reason})` : ""}, so credentials must come
              from the environment. An agent cannot read them back — child processes get an
              allowlist that excludes them.
            </div>
          ) : null}
          <div className="panel">
            <List labelledBy="settings-providers">
              {data.providers.map((provider) => (
                <Item key={provider.id}>
                  <ProviderRow
                    provider={provider}
                    keychain={data.keychain_available}
                    onChanged={settings.reload}
                  />
                </Item>
              ))}
            </List>
          </div>
        </Section>
      ) : null}

      <Integrations keychain={data?.keychain_available ?? null} />
      <NetworkCredentials keychain={data?.keychain_available ?? null} />

      {data ? (
        <Section id="settings-browser" title="Browser">
          <div className="panel">
            <div className="panel-body">
              {data.browser_path ? (
                <div className="mono muted">{data.browser_path}</div>
              ) : (
                <div className="banner warn flush prose" role="status">
                  {data.browser_hint}
                </div>
              )}
            </div>
          </div>
        </Section>
      ) : null}

      <Scheduler />
      <AuditLog />

      {data ? (
        <Section id="settings-storage" title="Storage">
          <div className="panel">
            <div className="panel-body">
              <dl className="facts flush">
                <StoragePath label="Data directory" what="data directory" path={data.data_dir} />
                <StoragePath label="Workspaces" what="workspaces" path={data.workspace} />
                <StoragePath label="Database" what="database" path={data.database} />
              </dl>
            </div>
          </div>
        </Section>
      ) : null}

      {data ? (
        <Tools
          tools={data.tools}
          agents={agents.data}
          agentsError={agents.data === null ? agents.error : null}
        />
      ) : null}
    </>
  );
}

// ---------------------------------------------------------------------------
// Structure
// ---------------------------------------------------------------------------

/** A heading and what it governs. The heading's id names the list beneath it. */
function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section aria-labelledby={id}>
      <h2 id={id}>{title}</h2>
      {children}
    </section>
  );
}

/**
 * A list of rows, named by its heading.
 *
 * ARIA roles rather than `<ul>` and `<li>`, as on the dashboard: the
 * stylesheets have no reset for a bare list. What a screen reader hears is the
 * same.
 */
function List({ labelledBy, children }: { labelledBy: string; children: ReactNode }) {
  return (
    <div role="list" aria-labelledby={labelledBy}>
      {children}
    </div>
  );
}

/** One entry of a {@link List}. */
function Item({ children }: { children: ReactNode }) {
  return <div role="listitem">{children}</div>;
}

/**
 * A chip saying whether something is in place.
 *
 * The verdict shape, since a stored key or a verified chain is a fact found,
 * not a risk. The hidden noun says what the chip is about, as every chip in
 * `status.tsx` does.
 */
function Fact({ tone, noun, children }: { tone: string; noun: string; children: string }) {
  return (
    <span className={`verdict ${tone}`}>
      <span className="visually-hidden">{noun} </span>
      {children}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Model providers
// ---------------------------------------------------------------------------

function ProviderRow({
  provider,
  keychain,
  onChanged,
}: {
  provider: ProviderView;
  keychain: boolean;
  onChanged: () => void;
}) {
  const formId = useId();
  const [editing, setEditing] = useState(false);
  // Plain state, never a draft: a key held in a draft would outlive the
  // moment it was needed, which is exactly what a key field must not do.
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [removeError, setRemoveError] = useState<string | null>(null);
  useUnsavedGuard(
    editing && key.trim() !== "",
    `A ${provider.id} key has been typed and not stored.`,
  );

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

  // A removal that fails is said so here. Before, the call was voided, and a
  // key that was still in the keychain looked removed until the next reload.
  const remove = useCallback(async () => {
    setBusy(true);
    setRemoveError(null);
    try {
      await api.removeProviderKey(provider.id);
      onChanged();
    } catch (failure) {
      setRemoveError(describeError(failure));
    } finally {
      setBusy(false);
    }
  }, [provider.id, onChanged]);

  const removable = provider.configured && provider.source === "system keychain";

  return (
    <div className="row stacked">
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
        {provider.configured ? (
          <Fact tone="ok" noun="key">
            Set
          </Fact>
        ) : (
          <Fact tone="neutral" noun="key">
            Not set
          </Fact>
        )}
        {keychain ? (
          <button
            type="button"
            className="ghost"
            aria-expanded={editing}
            aria-controls={editing ? formId : undefined}
            onClick={() => {
              setEditing((open) => !open);
              setKey("");
              setError(null);
            }}
          >
            {editing ? "Cancel" : provider.configured ? "Replace" : "Add key"}
          </button>
        ) : null}
        {removable ? (
          <button
            type="button"
            className="ghost"
            disabled={busy}
            onClick={() => setConfirming(true)}
          >
            Remove
          </button>
        ) : null}
      </div>

      {removeError ? (
        <div className="reveal">
          <ErrorBanner message={`The ${provider.id} key was not removed: ${removeError}`} />
        </div>
      ) : null}

      {editing ? (
        <div className="reveal" id={formId}>
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
          <button
            type="button"
            className="primary"
            disabled={busy || key.trim() === ""}
            onClick={() => void save()}
          >
            {busy ? "Storing…" : "Store key"}
          </button>
        </div>
      ) : null}

      {confirming ? (
        <ConfirmDialog
          title={`Remove the ${provider.id} key?`}
          message={
            `The key is deleted from this machine's keychain. Agents on ${provider.id} cannot ` +
            "run until a key is stored again or set in the environment."
          }
          confirmLabel="Remove key"
          cancelLabel="Keep it"
          onAnswer={(confirmed) => {
            setConfirming(false);
            if (confirmed) void remove();
          }}
        />
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Integrations
// ---------------------------------------------------------------------------

/**
 * The accounts an integration's tools act as.
 *
 * Read on its own rather than with the settings, as the credentials are, so a
 * bind or a removal reloads this list alone, and what is drawn is what the
 * runtime reports afterwards rather than what this screen expected.
 */
function Integrations({ keychain }: { keychain: boolean | null }) {
  const listed = useAsync(() => api.listIntegrations(), []);
  const [done, setDone] = useState<string | null>(null);
  const data = listed.data;

  return (
    <Section id="settings-integrations" title="Integrations">
      {listed.error ? (
        <ErrorBanner
          message={
            data
              ? `Integrations could not be refreshed: ${listed.error}`
              : `Integrations could not be read: ${listed.error}`
          }
        />
      ) : null}
      <p className="visually-hidden" role="status">
        {done ?? ""}
      </p>
      {data === null ? (
        listed.error ? null : (
          <div className="panel">
            <SkeletonRows count={1} />
          </div>
        )
      ) : data.length === 0 ? (
        <div className="panel">
          <div className="empty">No integration is registered.</div>
        </div>
      ) : (
        data.map((integration) => (
          <Integration
            key={integration.id}
            integration={integration}
            keychain={keychain}
            onChanged={(message) => {
              setDone(message);
              listed.reload();
            }}
          />
        ))
      )}
    </Section>
  );
}

/**
 * One integration: what binding an account to it would let an agent use, the
 * accounts bound, and the form that binds another.
 *
 * The token field is plain state, never a draft, and is emptied the moment
 * the call returns, bound or refused. Nothing comes back from the call; the
 * list is read again.
 */
function Integration({
  integration,
  keychain,
  onChanged,
}: {
  integration: IntegrationView;
  keychain: boolean | null;
  onChanged: (message: string) => void;
}) {
  const formId = useId();
  const headingId = `${formId}-heading`;
  const [adding, setAdding] = useState(false);
  const [label, setLabel] = useState("");
  const [host, setHost] = useState("");
  const [privateNetwork, setPrivateNetwork] = useState(false);
  const [scopes, setScopes] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<AccountRow | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const now = useNow();
  useUnsavedGuard(
    adding && token !== "",
    `A ${integration.display_name} token has been typed and not stored.`,
  );

  const close = () => {
    setAdding(false);
    setToken("");
    setError(null);
  };

  const bind = async () => {
    setBusy(true);
    setError(null);
    const bound = label.trim();
    try {
      await api.bindIntegration(
        integration.id,
        bound,
        hostArgument(host),
        offersPrivate && privateNetwork,
        scopes.trim() === "" ? null : scopes.trim(),
        token,
      );
      setLabel("");
      setHost("");
      setPrivateNetwork(false);
      setScopes("");
      setAdding(false);
      onChanged(`Bound ${bound} to ${integration.display_name}.`);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      // Bound or refused, the token has done its work in this window.
      setToken("");
      setBusy(false);
    }
  };

  const unbind = async (row: AccountRow) => {
    setRemoveError(null);
    try {
      await api.unbindIntegration(row.id);
      onChanged(`Removed ${row.label} from ${integration.display_name}.`);
    } catch (failure) {
      setRemoveError(`${row.label} was not removed: ${describeError(failure)}`);
    }
  };

  const rows = accountRows(integration.accounts, now);
  const problem = labelProblem(label);
  // Held while the host is the service's own, and sent only when offered.
  const offersPrivate = offersPrivateNetwork(host, integration.default_host);
  // Unknown while settings load; the runtime refuses for itself if it cannot store.
  const canStore = keychain !== false;

  return (
    <section aria-labelledby={headingId}>
      <h3 id={headingId}>{integration.display_name}</h3>
      <p className="field-note">
        An account lets an agent with a matching{" "}
        <span className="mono">{integration.id}</span> rule use:
      </p>
      <div className="row-meta mono">
        {integration.tools.map((tool) => (
          <span key={tool}>{tool}</span>
        ))}
      </div>
      {removeError ? <ErrorBanner message={removeError} /> : null}
      <div className="panel reveal">
        {rows.length === 0 ? (
          <div className="empty">
            No account is bound. From a terminal:{" "}
            <span className="mono">agentos integration add {integration.id}</span>
          </div>
        ) : (
          <List labelledBy={headingId}>
            {rows.map((row) => (
              <Item key={row.key}>
                <AccountItem
                  row={row}
                  integration={integration.display_name}
                  onRemove={() => setRemoving(row)}
                />
              </Item>
            ))}
          </List>
        )}
      </div>
      {canStore ? (
        <div className="inline reveal">
          <button
            type="button"
            aria-expanded={adding}
            aria-controls={adding ? formId : undefined}
            onClick={() => (adding ? close() : setAdding(true))}
          >
            {adding ? "Cancel" : "Add account"}
          </button>
        </div>
      ) : (
        <p className="field-note">
          With no keychain on this machine, an account cannot be bound from here.
        </p>
      )}
      {adding ? (
        <div className="panel reveal">
          <div className="panel-body" id={formId}>
            <div className="field">
              <label htmlFor={`${formId}-label`}>
                Label · lower-case letters, digits or -; what a call names the account by
              </label>
              <input
                id={`${formId}-label`}
                value={label}
                autoComplete="off"
                spellCheck={false}
                aria-invalid={problem !== null}
                aria-describedby={problem ? `${formId}-label-problem` : undefined}
                onChange={(event) => setLabel(event.target.value)}
              />
              {problem ? (
                <p className="field-note" id={`${formId}-label-problem`}>
                  {problem}
                </p>
              ) : null}
            </div>
            <div className="field">
              <label htmlFor={`${formId}-host`}>API host · optional</label>
              <input
                id={`${formId}-host`}
                value={host}
                autoComplete="off"
                spellCheck={false}
                placeholder={integration.default_host}
                onChange={(event) => setHost(event.target.value)}
              />
              <p className="field-note">
                For GitHub Enterprise, the server&apos;s address ending in{" "}
                <span className="mono">/api/v3</span>.
              </p>
            </div>
            {offersPrivate ? (
              <div className="field">
                <label className={privateNetwork ? "check on" : "check"}>
                  <input
                    type="checkbox"
                    checked={privateNetwork}
                    onChange={(event) => setPrivateNetwork(event.target.checked)}
                  />
                  <span>
                    Allow a private network address
                    <span className="check-note">
                      Lets this account&apos;s requests reach addresses on your own network,
                      for a GitHub Enterprise server; loopback, link-local and cloud metadata
                      addresses are still refused.
                    </span>
                  </span>
                </label>
              </div>
            ) : null}
            <div className="field">
              <label htmlFor={`${formId}-scopes`}>
                Scopes · optional; your note of what the token can do
              </label>
              <input
                id={`${formId}-scopes`}
                value={scopes}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => setScopes(event.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor={`${formId}-token`}>Token</label>
              <input
                id={`${formId}-token`}
                type="password"
                value={token}
                autoComplete="off"
                onChange={(event) => setToken(event.target.value)}
              />
              <p className="field-note">
                The token is kept in the system keychain, never in the AgentOS database, and an
                agent acts with it only where a <span className="mono">{integration.id}</span>{" "}
                rule allows, whatever its scopes, or where a{" "}
                <span className="mono">network.credential</span> rule names it.
              </p>
            </div>
            {error ? <ErrorBanner message={error} /> : null}
            <button
              type="button"
              className="primary"
              disabled={busy || !bindReady(label, token)}
              onClick={() => void bind()}
            >
              {busy ? "Binding…" : "Bind account"}
            </button>
          </div>
        </div>
      ) : null}

      {removing ? (
        <ConfirmDialog
          title={`Remove the ${integration.display_name} account ${removing.label}?`}
          message={
            "The account is removed, then its token is deleted from this machine's keychain. " +
            "A run that names it, or relies on it as the only account, will fail until one is " +
            "bound again."
          }
          confirmLabel="Remove account"
          cancelLabel="Keep it"
          onAnswer={(confirmed) => {
            const row = removing;
            setRemoving(null);
            if (confirmed) void unbind(row);
          }}
        />
      ) : null}
    </section>
  );
}

/**
 * One bound account, with a connection test.
 *
 * An account whose token is gone is drawn as broken and says what to do, and
 * is not offered a test: the runtime would refuse it before sending anything.
 */
function AccountItem({
  row,
  integration,
  onRemove,
}: {
  row: AccountRow;
  integration: string;
  onRemove: () => void;
}) {
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<IntegrationTestView | null>(null);
  const [testError, setTestError] = useState<string | null>(null);

  const test = async () => {
    setTesting(true);
    setResult(null);
    setTestError(null);
    try {
      setResult(await api.testIntegration(row.id));
    } catch (failure) {
      setTestError(describeError(failure));
    } finally {
      setTesting(false);
    }
  };

  const line = result ? testLine(result) : null;

  return (
    <div className="row stacked">
      <div className="inline">
        <div className="row-main">
          <div className="row-title mono">{row.label}</div>
          <div className="row-meta">
            {row.meta.map((part) => (
              <span key={part}>{part}</span>
            ))}
          </div>
        </div>
        {row.present ? (
          <Fact tone="ok" noun="token">
            Token stored
          </Fact>
        ) : (
          <Fact tone="danger" noun="token">
            No token
          </Fact>
        )}
        {row.present ? (
          <button type="button" className="ghost" disabled={testing} onClick={() => void test()}>
            {testing ? "Testing…" : "Test"}
            <span className="visually-hidden"> {row.label}</span>
          </button>
        ) : null}
        <button type="button" className="ghost" onClick={onRemove}>
          Remove
          <span className="visually-hidden"> {row.label}</span>
        </button>
      </div>
      {row.present ? null : (
        <div className="banner error flush reveal">
          The keychain holds no token for this account, so every {integration} call made as it
          fails. Remove it and add it again with a token
          {row.origin ? (
            <>
              , or store one under Network credentials as{" "}
              <span className="mono">
                {row.origin} / {row.label}
              </span>
            </>
          ) : null}
          .
        </div>
      )}
      <div role="status" aria-live="polite" aria-busy={testing}>
        {line ? (
          <div className="inline reveal">
            <Fact tone={line.tone} noun="connection">
              {line.verdict}
            </Fact>
            <span className="muted">{line.detail}</span>
          </div>
        ) : null}
      </div>
      {testError ? (
        <div className="reveal">
          <ErrorBanner message={`The test could not run: ${testError}`} />
        </div>
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Network credentials
// ---------------------------------------------------------------------------

/**
 * Secrets a run may send to one origin, and nowhere else.
 *
 * The list shows an origin and a name and never a value, not even masked: the
 * runtime does not send one, and the rows are built so that a view which did
 * would still not draw it. The secret field is plain state, never a draft, and
 * is emptied the moment the call returns, stored or not.
 */
function NetworkCredentials({ keychain }: { keychain: boolean | null }) {
  const formId = useId();
  const stored = useAsync(() => api.listNetworkCredentials(), []);
  const [adding, setAdding] = useState(false);
  const [origin, setOrigin] = useState("");
  const [name, setName] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [removing, setRemoving] = useState<CredentialRow | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);
  useUnsavedGuard(adding && secret !== "", "A network credential has been typed and not stored.");

  const close = () => {
    setAdding(false);
    setSecret("");
    setError(null);
  };

  const store = async () => {
    setBusy(true);
    setError(null);
    setDone(null);
    try {
      const saved = await api.setNetworkCredential(origin.trim(), name.trim(), secret);
      setDone(`Stored ${saved.name} for ${saved.origin}.`);
      setOrigin("");
      setName("");
      setAdding(false);
      stored.reload();
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      // Stored or refused, the secret has done its work in this window.
      setSecret("");
      setBusy(false);
    }
  };

  const remove = async (row: CredentialRow) => {
    setRemoveError(null);
    setDone(null);
    try {
      await api.removeNetworkCredential(row.origin, row.name);
      setDone(`Removed ${row.name} for ${row.origin}.`);
      stored.reload();
    } catch (failure) {
      setRemoveError(`${row.label} was not removed: ${describeError(failure)}`);
    }
  };

  const rows = stored.data ? credentialRows(stored.data) : null;
  // Unknown while settings load; the runtime refuses for itself if it cannot store.
  const canStore = keychain !== false;

  return (
    <Section id="settings-credentials" title="Network credentials">
      <p className="muted">
        A credential is bound to its origin, so a run cannot spend it anywhere else. Spending it
        is a separate grant, <span className="mono">network.credential</span>, from reaching the
        origin.
      </p>
      {stored.error ? (
        <ErrorBanner
          message={
            rows
              ? `Credentials could not be refreshed: ${stored.error}`
              : `Credentials could not be read: ${stored.error}`
          }
        />
      ) : null}
      {removeError ? <ErrorBanner message={removeError} /> : null}
      <p className="visually-hidden" role="status">
        {done ?? ""}
      </p>
      <div className="panel">
        {rows === null ? (
          stored.error ? null : (
            <SkeletonRows count={1} />
          )
        ) : rows.length === 0 ? (
          <div className="empty">No credentials are stored.</div>
        ) : (
          <List labelledBy="settings-credentials">
            {rows.map((row) => (
              <Item key={row.key}>
                <Row>
                  <div className="row-main">
                    <div className="row-title mono" title={row.label}>
                      {row.label}
                    </div>
                    {row.account ? (
                      <div className="row-meta">
                        <span>the token of {row.account}</span>
                      </div>
                    ) : null}
                  </div>
                  <button type="button" className="ghost" onClick={() => setRemoving(row)}>
                    Remove
                    <span className="visually-hidden"> {row.label}</span>
                  </button>
                </Row>
              </Item>
            ))}
          </List>
        )}
      </div>
      {canStore ? (
        <div className="inline reveal">
          <button
            type="button"
            aria-expanded={adding}
            aria-controls={adding ? formId : undefined}
            onClick={() => (adding ? close() : setAdding(true))}
          >
            {adding ? "Cancel" : "Add credential"}
          </button>
          {done ? <span className="faint">{done}</span> : null}
        </div>
      ) : (
        <p className="field-note">
          With no keychain on this machine, a credential cannot be stored from here.
        </p>
      )}
      {adding ? (
        <div className="panel reveal">
          <div className="panel-body" id={formId}>
            <div className="field">
              <label htmlFor={`${formId}-origin`}>
                Origin · scheme, host and port, such as https://crm.example.com
              </label>
              <input
                id={`${formId}-origin`}
                value={origin}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => setOrigin(event.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor={`${formId}-name`}>
                Name · letters, digits, _ or -; what a policy and a call refer to it by
              </label>
              <input
                id={`${formId}-name`}
                value={name}
                autoComplete="off"
                spellCheck={false}
                onChange={(event) => setName(event.target.value)}
              />
            </div>
            <div className="field">
              <label htmlFor={`${formId}-secret`}>
                Token · sent as a bearer token, so without the scheme; kept in the operating
                system keychain and never shown again
              </label>
              <input
                id={`${formId}-secret`}
                type="password"
                value={secret}
                autoComplete="off"
                onChange={(event) => setSecret(event.target.value)}
              />
              <p className="field-note">
                A name already stored for the same origin is replaced, an integration
                account&apos;s token included.
              </p>
            </div>
            {error ? <ErrorBanner message={error} /> : null}
            <button
              type="button"
              className="primary"
              disabled={busy || !credentialReady(origin, name, secret)}
              onClick={() => void store()}
            >
              {busy ? "Storing…" : "Store credential"}
            </button>
          </div>
        </div>
      ) : null}


      {removing ? (
        <ConfirmDialog
          title="Remove this credential?"
          message={
            `${removing.label}\n\n` +
            "The secret is deleted from this machine's keychain. A run that needs it will have " +
            "nothing to send until it is stored again." +
            (removing.account
              ? ` It is the token of ${removing.account}, which will have no token either.`
              : "")
          }
          confirmLabel="Remove credential"
          cancelLabel="Keep it"
          onAnswer={(confirmed) => {
            const row = removing;
            setRemoving(null);
            if (confirmed) void remove(row);
          }}
        />
      ) : null}
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/**
 * The switch for unattended work on this machine, and what it would find.
 *
 * The pacing fields follow the scheduler until someone types in them, and
 * follow it again once what was typed has been applied.
 */
function Scheduler() {
  const now = useNow();
  const fieldId = useId();
  const status = useAsync(() => api.schedulerStatus(), []);
  useVisibleInterval(status.reload, SCHEDULER_MS);

  // The switch's own answer, shown until the next read of the status, which
  // it then asks for. Without it the panel would read the old state for the
  // moment between the two.
  const [answer, setAnswer] = useState<SchedulerView | null>(null);
  useEffect(() => setAnswer(null), [status.data]);
  const view = answer ?? status.data;

  const [tickText, setTickText] = useState<string | null>(null);
  const [maxText, setMaxText] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refusal, setRefusal] = useState<string | null>(null);

  const pacing = view
    ? readPacing(
        view,
        tickText ?? String(view.tick_seconds),
        maxText ?? String(view.max_concurrent_runs),
      )
    : null;
  useUnsavedGuard(
    pacing?.changed === true,
    "The scheduler's pacing has been changed and not applied.",
  );

  const apply = async (enabled: boolean) => {
    if (!view || !pacing) return;
    setBusy(true);
    setRefusal(null);
    try {
      // Turning it off never waits on the fields: a stop is always possible,
      // whatever has been typed beside it.
      const tick = (pacing.valid ? pacing.tick : null) ?? view.tick_seconds;
      const max = (pacing.valid ? pacing.max : null) ?? view.max_concurrent_runs;
      setAnswer(await api.setSchedulerRunning(enabled, tick, max));
      setTickText(null);
      setMaxText(null);
    } catch (failure) {
      setRefusal(describeError(failure));
    } finally {
      setBusy(false);
      status.reload();
    }
  };

  const notices = schedulerNotices(view, refusal);

  return (
    <Section id="settings-scheduler" title="Scheduler">
      {status.error ? (
        <ErrorBanner
          message={
            view
              ? `The scheduler's state could not be refreshed: ${status.error}`
              : `The scheduler's state could not be read: ${status.error}`
          }
        />
      ) : null}
      {notices.map((notice) => (
        <div key={notice} className="banner warn" role="status">
          {notice}
        </div>
      ))}
      <div className="panel">
        {view === null || pacing === null ? (
          status.error ? null : (
            <SkeletonRows count={2} />
          )
        ) : (
          <div className="panel-body">
            <div className="inline">
              {view.running ? (
                <Fact tone="live" noun="scheduler">
                  Running
                </Fact>
              ) : (
                <Fact tone="neutral" noun="scheduler">
                  Off
                </Fact>
              )}
              <span className="muted">{schedulerSummary(view, now)}</span>
            </div>

            <div className="inline reveal">
              <div className="field">
                <label htmlFor={`${fieldId}-tick`}>Tick, in seconds</label>
                <input
                  id={`${fieldId}-tick`}
                  type="number"
                  min={5}
                  step={1}
                  inputMode="numeric"
                  value={tickText ?? String(view.tick_seconds)}
                  onChange={(event) => setTickText(event.target.value)}
                />
              </div>
              <div className="field">
                <label htmlFor={`${fieldId}-max`}>Runs at once, at most</label>
                <input
                  id={`${fieldId}-max`}
                  type="number"
                  min={1}
                  step={1}
                  inputMode="numeric"
                  value={maxText ?? String(view.max_concurrent_runs)}
                  onChange={(event) => setMaxText(event.target.value)}
                />
              </div>
            </div>
            <div className="inline">
              {view.running ? (
                <button type="button" disabled={busy} onClick={() => void apply(false)}>
                  {busy ? "Working…" : "Turn off"}
                </button>
              ) : (
                <button
                  type="button"
                  className="primary"
                  disabled={busy || !pacing.valid}
                  onClick={() => void apply(true)}
                >
                  {busy ? "Working…" : "Turn on"}
                </button>
              )}
              {view.running && pacing.changed ? (
                <button
                  type="button"
                  disabled={busy || !pacing.valid}
                  onClick={() => void apply(true)}
                >
                  Apply pacing
                </button>
              ) : null}
              {!pacing.valid ? (
                <span className="faint">Both fields need a whole number.</span>
              ) : null}
            </div>
            <p className="field-note">
              A scheduled run has nobody watching it, so anything the policy would have escalated
              to a person is refused, with a note the agent can re-plan around.
            </p>
            <p className="field-note">
              Schedules fire only while this application is open, or while{" "}
              <span className="mono">agentos schedule run</span> runs in a terminal. Turning the
              scheduler on is remembered, and it starts again at the next launch.
            </p>

            <dl className="facts flush reveal">
              {schedulerFacts(view, now).map((fact) => (
                <FactPair key={fact.label} label={fact.label} attention={fact.attention}>
                  {fact.value}
                </FactPair>
              ))}
            </dl>
          </div>
        )}
      </div>
    </Section>
  );
}

function FactPair({
  label,
  attention,
  children,
}: {
  label: string;
  attention: boolean;
  children: string;
}) {
  return (
    <>
      <dt>{label}</dt>
      <dd>
        {attention ? (
          <Fact tone="warn" noun={label}>
            {children}
          </Fact>
        ) : (
          children
        )}
      </dd>
    </>
  );
}

// ---------------------------------------------------------------------------
// Audit log
// ---------------------------------------------------------------------------

function AuditLog() {
  const now = useNow();
  const [last, setLast] = useState<Verification | null>(() => lastVerification);
  const [verifying, setVerifying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const verify = useCallback(async () => {
    setVerifying(true);
    setError(null);
    try {
      const problems = await api.verifyAudit();
      const result = { at: new Date().toISOString(), problems };
      lastVerification = result;
      setLast(result);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      setVerifying(false);
    }
  }, []);

  return (
    <Section id="settings-audit" title="Audit log">
      {error ? <ErrorBanner message={`The chain could not be verified: ${error}`} /> : null}
      <div className="panel">
        <div className="panel-body">
          <div className="inline">
            <button type="button" disabled={verifying} onClick={() => void verify()}>
              {verifying ? "Verifying…" : "Verify the chain"}
            </button>
            {/* The sentence is withheld while a check runs, so the same result
                twice is still a change the region announces. */}
            <span role="status" aria-live="polite" aria-busy={verifying}>
              {last && !verifying ? (
                <>
                  {last.problems.length === 0 ? (
                    <Fact tone="ok" noun="audit chain">
                      Intact
                    </Fact>
                  ) : (
                    <Fact tone="danger" noun="audit chain">
                      {last.problems.length === 1
                        ? "1 problem"
                        : `${last.problems.length} problems`}
                    </Fact>
                  )}
                  <span className="visually-hidden">
                    {" "}
                    {verificationAnnouncement(last.problems)}
                  </span>
                </>
              ) : null}
            </span>
            <span className="faint">{verificationLine(last, now)}</span>
          </div>
          <p className="field-note">
            Recomputes every hash and checks each record still points at the one before it.
          </p>
          {last && last.problems.length > 0 ? (
            <div className="stack reveal">
              {last.problems.map((problem, index) => (
                <div key={`${index}:${problem}`} className="banner error flush">
                  {problem}
                </div>
              ))}
            </div>
          ) : null}
        </div>
      </div>
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/**
 * One stored path, with a Copy button: these are what an operator pastes into
 * a terminal or a ticket, and selecting a long path by hand is where a
 * character gets dropped.
 */
function StoragePath({ label, what, path }: { label: string; what: string; path: string }) {
  const { copy, state } = useCopy();
  return (
    <>
      <dt>{label}</dt>
      <dd>
        <div className="inline">
          <span className="mono">{path}</span>
          <button type="button" className="ghost" onClick={() => void copy(path)}>
            {state === "copied" ? "Copied" : state === "failed" ? "Could not copy" : "Copy"}
            <span className="visually-hidden"> the {what} path</span>
          </button>
          <span className="visually-hidden" role="status">
            {state === "copied"
              ? `The ${what} path was copied.`
              : state === "failed"
                ? `The ${what} path could not be copied.`
                : ""}
          </span>
        </div>
      </dd>
    </>
  );
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/**
 * Every tool, by domain, with who holds it.
 *
 * The Agents screen answers what one agent may do; this answers the reverse,
 * which is the question an operator brings here: who on this machine can run
 * `terminal.exec`?
 */
function Tools({
  tools,
  agents,
  agentsError,
}: {
  tools: readonly ToolView[];
  agents: readonly AgentSummary[] | null;
  agentsError: string | null;
}) {
  const filterId = useId();
  const [filter, setFilter] = useState("");
  const groups = catalogue(tools, agents, filter);
  const shown = countEntries(groups);
  const filtering = filter.trim() !== "";

  return (
    <Section id="settings-tools" title="Tools">
      <p className="muted">
        <span className="verdict warn">external</span> marks a tool whose output someone other
        than you can write. Reading it raises the approval bar for the rest of the run.
      </p>
      {agentsError ? (
        <ErrorBanner message={`Who holds each tool could not be read: ${agentsError}`} />
      ) : null}
      <div className="toolbar">
        <input
          id={filterId}
          type="search"
          aria-label="Filter tools"
          placeholder="Filter by tool, capability or agent"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>
      <p className="visually-hidden" role="status">
        {filtering ? `${shown} of ${tools.length} tools shown.` : ""}
      </p>
      {groups.length === 0 ? (
        <div className="panel">
          <div className="empty">
            {tools.length === 0 ? "No tools are registered." : "No tool matches the filter."}
          </div>
        </div>
      ) : (
        groups.map((group) => {
          const headingId = `${filterId}-${group.domain}`;
          return (
            <section key={group.domain} aria-labelledby={headingId}>
              <h3 id={headingId} className="mono">
                {group.domain}
              </h3>
              <div className="panel">
                <List labelledBy={headingId}>
                  {group.entries.map((entry) => (
                    <Item key={entry.tool.name}>
                      <ToolRow entry={entry} />
                    </Item>
                  ))}
                </List>
              </div>
            </section>
          );
        })
      )}
    </Section>
  );
}

function ToolRow({ entry }: { entry: CatalogueEntry }) {
  const { tool } = entry;
  const holders = grantLine(entry.holders);
  return (
    <Row>
      <div className="row-main">
        <div className="row-title mono">{tool.name}</div>
        <div className="row-meta">
          <span>{tool.description}</span>
        </div>
        {tool.capabilities.length > 0 ? (
          <div className="row-meta mono">
            <span className="visually-hidden">Capabilities:</span>
            {tool.capabilities.map((capability) => (
              <span key={capability}>{capability}</span>
            ))}
          </div>
        ) : null}
        <div className="row-meta">
          <span className={holders.nobody ? "faint" : undefined}>{holders.text}</span>
        </div>
      </div>
      <Risk level={tool.risk} />
      {tool.returns_untrusted_data ? (
        <span className="verdict warn">
          external
          <span className="visually-hidden">
            . Its output can be written by someone other than you, so reading it raises the
            approval bar for the rest of the run.
          </span>
        </span>
      ) : null}
    </Row>
  );
}
