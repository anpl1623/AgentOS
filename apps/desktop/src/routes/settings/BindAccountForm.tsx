import { useState } from "react";

import type { IntegrationView } from "../../bindings/IntegrationView";
import { ErrorBanner } from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { useUnsavedGuard } from "../../sdk/drafts";
import { bindReady, hostArgument, labelProblem, offersPrivateNetwork } from "./integrationsModel";

/** The bind form's state, owned by the integration so it outlives the form. */
export interface BindAccount {
  adding: boolean;
  open: () => void;
  /** Close the form. The token goes; the label, host and note stay for next time. */
  close: () => void;
  label: string;
  setLabel: (value: string) => void;
  host: string;
  setHost: (value: string) => void;
  privateNetwork: boolean;
  setPrivateNetwork: (value: boolean) => void;
  scopes: string;
  setScopes: (value: string) => void;
  token: string;
  setToken: (value: string) => void;
  /** The private-network permission is on offer for the host typed. */
  offersPrivate: boolean;
  busy: boolean;
  error: string | null;
  bind: () => Promise<void>;
}

/**
 * The state behind binding an account to `integration`.
 *
 * Held by the integration rather than the form, so closing the form and
 * opening it again keeps what was typed apart from the token. The token field
 * is plain state, never a draft, and is emptied the moment the call returns,
 * bound or refused. Nothing comes back from the call; the list is read again.
 */
export function useBindAccount(
  integration: IntegrationView,
  onBound: (message: string) => void,
): BindAccount {
  const [adding, setAdding] = useState(false);
  const [label, setLabel] = useState("");
  const [host, setHost] = useState("");
  const [privateNetwork, setPrivateNetwork] = useState(false);
  const [scopes, setScopes] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useUnsavedGuard(
    adding && token !== "",
    `A ${integration.display_name} token has been typed and not stored.`,
  );

  // Held while the host is the service's own, and sent only when offered.
  const offersPrivate = offersPrivateNetwork(host, integration.default_host);

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
      onBound(`Bound ${bound} to ${integration.display_name}.`);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      // Bound or refused, the token has done its work in this window.
      setToken("");
      setBusy(false);
    }
  };

  return {
    adding,
    open: () => setAdding(true),
    close,
    label,
    setLabel,
    host,
    setHost,
    privateNetwork,
    setPrivateNetwork,
    scopes,
    setScopes,
    token,
    setToken,
    offersPrivate,
    busy,
    error,
    bind,
  };
}

/**
 * The fields that bind an account: its label, an optional host for a server
 * of the operator's own, the private-network permission when that host is
 * not the service's, a note of the token's scopes, and the token.
 */
export function BindAccountForm({
  form,
  integration,
  formId,
}: {
  form: BindAccount;
  integration: IntegrationView;
  formId: string;
}) {
  const problem = labelProblem(form.label);

  return (
    <div className="settings-form settings-enter" id={formId}>
      <div className="settings-form-grid">
        <div className="field">
          <label htmlFor={`${formId}-label`}>
            Label · lower-case letters, digits or -; what a call names the account by
          </label>
          <input
            id={`${formId}-label`}
            value={form.label}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={problem !== null}
            aria-describedby={problem ? `${formId}-label-problem` : undefined}
            onChange={(event) => form.setLabel(event.target.value)}
          />
          {problem ? (
            <p className="field-note settings-invalid" id={`${formId}-label-problem`}>
              {problem}
            </p>
          ) : null}
        </div>
        <div className="field">
          <label htmlFor={`${formId}-host`}>API host · optional</label>
          <input
            id={`${formId}-host`}
            value={form.host}
            autoComplete="off"
            spellCheck={false}
            placeholder={integration.default_host}
            onChange={(event) => form.setHost(event.target.value)}
          />
          <p className="field-note">
            For GitHub Enterprise, the server&apos;s address ending in{" "}
            <span className="mono">/api/v3</span>.
          </p>
        </div>
      </div>
      {form.offersPrivate ? (
        <div className="field settings-enter">
          <label className={form.privateNetwork ? "check on" : "check"}>
            <input
              type="checkbox"
              checked={form.privateNetwork}
              onChange={(event) => form.setPrivateNetwork(event.target.checked)}
            />
            <span>
              Allow a private network address
              <span className="check-note">
                Lets this account&apos;s requests reach addresses on your own network, for a
                GitHub Enterprise server; loopback, link-local and cloud metadata addresses are
                still refused.
              </span>
            </span>
          </label>
        </div>
      ) : null}
      <div className="settings-form-grid">
        <div className="field">
          <label htmlFor={`${formId}-scopes`}>
            Scopes · optional; your note of what the token can do
          </label>
          <input
            id={`${formId}-scopes`}
            value={form.scopes}
            autoComplete="off"
            spellCheck={false}
            onChange={(event) => form.setScopes(event.target.value)}
          />
        </div>
        <div className="field">
          <label htmlFor={`${formId}-token`}>Token</label>
          <input
            id={`${formId}-token`}
            type="password"
            value={form.token}
            autoComplete="off"
            onChange={(event) => form.setToken(event.target.value)}
          />
        </div>
      </div>
      <p className="field-note settings-form-note">
        The token is kept in the system keychain, never in the AgentOS database, and an agent
        acts with it only where a <span className="mono">{integration.id}</span> rule allows,
        whatever its scopes, or where a <span className="mono">network.credential</span> rule
        names it.
      </p>
      {form.error ? <ErrorBanner message={form.error} /> : null}
      <div className="settings-form-actions">
        <button
          type="button"
          className="primary"
          disabled={form.busy || !bindReady(form.label, form.token)}
          onClick={() => void form.bind()}
        >
          {form.busy ? "Binding…" : "Bind account"}
        </button>
        <button type="button" className="ghost" onClick={form.close}>
          Cancel
        </button>
      </div>
    </div>
  );
}
