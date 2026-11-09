import { useCallback, useId, useRef, useState } from "react";

import type { ProviderView } from "../../bindings/ProviderView";
import type { SettingsView } from "../../bindings/SettingsView";
import { ErrorBanner, Icon } from "../../components/common";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { api, describeError } from "../../sdk/client";
import { useUnsavedGuard } from "../../sdk/drafts";
import { useReturnFocus } from "./hooks";
import { Fact, Item, List, RowMark, Section } from "./parts";

/**
 * The model providers and whether each has a key. A key is stored in the
 * operating system's keychain from here, replaced, or removed; it is never
 * shown again, only the redacted hint the runtime gives.
 */
export function Providers({
  settings,
  onChanged,
}: {
  settings: SettingsView;
  onChanged: () => void;
}) {
  return (
    <Section
      id="settings-providers"
      intro="The keys agents reach a model with. Stored in this machine's keychain, never in the database or a log."
    >
      {!settings.keychain_available ? (
        <div className="banner warn" role="status">
          This machine has no usable keychain
          {settings.keychain_reason ? ` (${settings.keychain_reason})` : ""}, so credentials must
          come from the environment. An agent cannot read them back — child processes get an
          allowlist that excludes them.
        </div>
      ) : null}
      <div className="panel">
        <List labelledBy="settings-providers" className="stagger">
          {settings.providers.map((provider) => (
            <Item key={provider.id}>
              <ProviderRow
                provider={provider}
                keychain={settings.keychain_available}
                onChanged={onChanged}
              />
            </Item>
          ))}
        </List>
      </div>
    </Section>
  );
}

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
  const toggle = useRef<HTMLButtonElement>(null);
  useReturnFocus(editing, toggle);

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
    <div className="row stacked settings-item">
      <div className="settings-item-head">
        <RowMark icon="key" tone={provider.configured ? "ok" : "neutral"} />
        <div className="row-main">
          <div className="row-title">{provider.id}</div>
          <div className="row-meta">
            {provider.configured ? (
              <span>
                <span className="mono">{provider.hint}</span> · via {provider.source}
              </span>
            ) : (
              <span>{provider.note}</span>
            )}
          </div>
        </div>
        <div className="settings-item-actions">
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
              ref={toggle}
              type="button"
              className="ghost settings-button"
              aria-expanded={editing}
              aria-controls={editing ? formId : undefined}
              onClick={() => {
                setEditing((open) => !open);
                setKey("");
                setError(null);
              }}
            >
              <Icon name={editing ? "x" : provider.configured ? "edit" : "plus"} size={14} />
              {editing ? "Cancel" : provider.configured ? "Replace" : "Add key"}
            </button>
          ) : null}
          {removable ? (
            <button
              type="button"
              className="ghost settings-button"
              disabled={busy}
              onClick={() => setConfirming(true)}
            >
              <Icon name="trash" size={14} />
              Remove
            </button>
          ) : null}
        </div>
      </div>

      {removeError ? (
        <div className="settings-item-body settings-enter">
          <ErrorBanner message={`The ${provider.id} key was not removed: ${removeError}`} />
        </div>
      ) : null}

      {editing ? (
        <div className="settings-item-body settings-enter" id={formId}>
          <label htmlFor={`key-${provider.id}`}>
            API key · stored in the operating system keychain, never in the database or a log
          </label>
          <div className="settings-field-row">
            <input
              id={`key-${provider.id}`}
              type="password"
              value={key}
              autoComplete="off"
              onChange={(event) => setKey(event.target.value)}
              placeholder="Paste the key"
            />
            <button
              type="button"
              className="primary"
              disabled={busy || key.trim() === ""}
              onClick={() => void save()}
            >
              {busy ? "Storing…" : "Store key"}
            </button>
          </div>
          {error ? (
            <div className="reveal">
              <ErrorBanner message={error} />
            </div>
          ) : null}
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
