import { useState } from "react";

import type { NetworkCredentialView } from "../../bindings/NetworkCredentialView";
import { ErrorBanner } from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { useUnsavedGuard } from "../../sdk/drafts";
import { credentialReady } from "./credentialsModel";

/** The add form's state, owned by the section so it outlives the form. */
export interface AddCredential {
  adding: boolean;
  open: () => void;
  /** Close the form. The secret goes; the origin and name stay for next time. */
  close: () => void;
  origin: string;
  setOrigin: (value: string) => void;
  name: string;
  setName: (value: string) => void;
  secret: string;
  setSecret: (value: string) => void;
  busy: boolean;
  error: string | null;
  store: () => Promise<void>;
}

/**
 * The state behind storing a credential.
 *
 * The secret field is plain state, never a draft, and is emptied the moment
 * the call returns, stored or not. `onStart` is told as a store begins, so
 * the section can clear the sentence about whatever it last did.
 */
export function useAddCredential(
  onStored: (saved: NetworkCredentialView) => void,
  onStart: () => void,
): AddCredential {
  const [adding, setAdding] = useState(false);
  const [origin, setOrigin] = useState("");
  const [name, setName] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useUnsavedGuard(adding && secret !== "", "A network credential has been typed and not stored.");

  const close = () => {
    setAdding(false);
    setSecret("");
    setError(null);
  };

  const store = async () => {
    setBusy(true);
    setError(null);
    onStart();
    try {
      const saved = await api.setNetworkCredential(origin.trim(), name.trim(), secret);
      setOrigin("");
      setName("");
      setAdding(false);
      onStored(saved);
    } catch (failure) {
      setError(describeError(failure));
    } finally {
      // Stored or refused, the secret has done its work in this window.
      setSecret("");
      setBusy(false);
    }
  };

  return {
    adding,
    open: () => setAdding(true),
    close,
    origin,
    setOrigin,
    name,
    setName,
    secret,
    setSecret,
    busy,
    error,
    store,
  };
}

/**
 * The fields that store a credential: the origin it is bound to, its name, and
 * the secret. `focusFirst` puts focus in the first field as the form opens,
 * for when the button that opened it does not stay on screen to hold it.
 */
export function CredentialForm({
  form,
  formId,
  focusFirst,
}: {
  form: AddCredential;
  formId: string;
  focusFirst: boolean;
}) {
  return (
    <div className="panel settings-enter">
      <div className="panel-body" id={formId}>
        <div className="settings-form-grid">
          <div className="field">
            <label htmlFor={`${formId}-origin`}>
              Origin · scheme, host and port, such as https://crm.example.com
            </label>
            <input
              id={`${formId}-origin`}
              autoFocus={focusFirst}
              value={form.origin}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => form.setOrigin(event.target.value)}
            />
          </div>
          <div className="field">
            <label htmlFor={`${formId}-name`}>
              Name · letters, digits, _ or -; what a policy and a call refer to it by
            </label>
            <input
              id={`${formId}-name`}
              value={form.name}
              autoComplete="off"
              spellCheck={false}
              onChange={(event) => form.setName(event.target.value)}
            />
          </div>
        </div>
        <div className="field">
          <label htmlFor={`${formId}-secret`}>
            Token · sent as a bearer token, so without the scheme; kept in the operating system
            keychain and never shown again
          </label>
          <input
            id={`${formId}-secret`}
            type="password"
            value={form.secret}
            autoComplete="off"
            onChange={(event) => form.setSecret(event.target.value)}
          />
          <p className="field-note">
            A name already stored for the same origin is replaced, an integration account&apos;s
            token included.
          </p>
        </div>
        {form.error ? <ErrorBanner message={form.error} /> : null}
        <div className="settings-form-actions">
          <button
            type="button"
            className="primary"
            disabled={form.busy || !credentialReady(form.origin, form.name, form.secret)}
            onClick={() => void form.store()}
          >
            {form.busy ? "Storing…" : "Store credential"}
          </button>
          <button type="button" className="ghost" onClick={form.close}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
