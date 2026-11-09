import { useId, useRef, useState } from "react";

import type { IntegrationView } from "../../bindings/IntegrationView";
import { EmptyState, ErrorBanner, Icon, SkeletonRows } from "../../components/common";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { useNow } from "../../components/hooks";
import { api, describeError } from "../../sdk/client";
import type { Async } from "../../sdk/useAsync";
import { AccountItem } from "./AccountItem";
import { BindAccountForm, useBindAccount } from "./BindAccountForm";
import { useReturnFocus } from "./hooks";
import { type AccountRow, accountRows } from "./integrationsModel";
import { Item, List, Section } from "./parts";
import { counted } from "./settingsModel";

/**
 * The accounts an integration's tools act as.
 *
 * Read on its own rather than with the settings, as the credentials are, so a
 * bind or a removal reloads this list alone, and what is drawn is what the
 * runtime reports afterwards rather than what this screen expected. The read
 * itself is the screen's, because the overview counts the same accounts.
 */
export function Integrations({
  listed,
  keychain,
}: {
  listed: Async<IntegrationView[]>;
  keychain: boolean | null;
}) {
  const [done, setDone] = useState<string | null>(null);
  const data = listed.data;

  return (
    <Section
      id="settings-integrations"
      intro="The accounts an integration's tools act as. An agent uses one only where its policy has a matching rule."
    >
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
          <EmptyState icon="link" title="No integration is registered">
            This build of AgentOS has no integration an account could be bound to.
          </EmptyState>
        </div>
      ) : (
        <div className="settings-stack">
          {data.map((integration) => (
            <Integration
              key={integration.id}
              integration={integration}
              keychain={keychain}
              onChanged={(message) => {
                setDone(message);
                listed.reload();
              }}
            />
          ))}
        </div>
      )}
    </Section>
  );
}

/**
 * One integration: what binding an account to it would let an agent use, the
 * accounts bound, and the form that binds another.
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
  const [removing, setRemoving] = useState<AccountRow | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const now = useNow();
  const form = useBindAccount(integration, onChanged);
  const toggle = useRef<HTMLButtonElement>(null);
  useReturnFocus(form.adding, toggle);

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
  // Unknown while settings load; the runtime refuses for itself if it cannot store.
  const canStore = keychain !== false;

  return (
    <section className="panel settings-integration" aria-labelledby={headingId}>
      <div className="settings-integration-head">
        <span className="settings-integration-mark" aria-hidden="true">
          <Icon name="link" size={18} />
        </span>
        <div className="settings-integration-title">
          <h3 id={headingId}>{integration.display_name}</h3>
          <p className="settings-integration-sub">
            {counted(rows.length, "account", "accounts")} ·{" "}
            <span className="mono">{integration.default_host}</span>
          </p>
        </div>
        {canStore ? (
          <button
            ref={toggle}
            type="button"
            className="settings-button"
            aria-expanded={form.adding}
            aria-controls={form.adding ? formId : undefined}
            onClick={() => (form.adding ? form.close() : form.open())}
          >
            <Icon name={form.adding ? "x" : "plus"} size={14} />
            {form.adding ? "Cancel" : "Add account"}
          </button>
        ) : null}
      </div>
      <div className="settings-integration-tools">
        <p className="field-note">
          An account lets an agent with a matching{" "}
          <span className="mono">{integration.id}</span> rule use:
        </p>
        <div className="settings-tags">
          {integration.tools.map((tool) => (
            <span key={tool} className="tag">
              {tool}
            </span>
          ))}
        </div>
      </div>
      {removeError ? (
        <div className="settings-integration-note">
          <ErrorBanner message={removeError} />
        </div>
      ) : null}
      {rows.length === 0 ? (
        <div className="empty">
          No account is bound. From a terminal:{" "}
          <span className="mono">agentos integration add {integration.id}</span>
        </div>
      ) : (
        <List labelledBy={headingId} className="stagger">
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
      {canStore ? null : (
        <p className="field-note settings-integration-note">
          With no keychain on this machine, an account cannot be bound from here.
        </p>
      )}
      {form.adding ? (
        <BindAccountForm form={form} integration={integration} formId={formId} />
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
