import { useId, useRef, useState } from "react";

import { EmptyState, ErrorBanner, Icon, SkeletonRows } from "../../components/common";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { api, describeError } from "../../sdk/client";
import { useAsync } from "../../sdk/useAsync";
import { CredentialForm, useAddCredential } from "./CredentialForm";
import { type CredentialRow, credentialRows } from "./credentialsModel";
import { useReturnFocus } from "./hooks";
import { Item, List, RowMark, Section } from "./parts";

/**
 * Secrets a run may send to one origin, and nowhere else.
 *
 * The list shows an origin and a name and never a value, not even masked: the
 * runtime does not send one, and the rows are built so that a view which did
 * would still not draw it.
 */
export function NetworkCredentials({ keychain }: { keychain: boolean | null }) {
  const formId = useId();
  const stored = useAsync(() => api.listNetworkCredentials(), []);
  const [done, setDone] = useState<string | null>(null);
  const [removing, setRemoving] = useState<CredentialRow | null>(null);
  const [removeError, setRemoveError] = useState<string | null>(null);
  const form = useAddCredential(
    (saved) => {
      setDone(`Stored ${saved.name} for ${saved.origin}.`);
      stored.reload();
    },
    () => setDone(null),
  );
  const toggleRef = useRef<HTMLButtonElement>(null);
  useReturnFocus(form.adding, toggleRef);
  // Set when the form was opened from the empty list, whose button goes as
  // the form arrives; focus then starts in the form rather than falling to
  // the top of the window.
  const [focusFirst, setFocusFirst] = useState(false);

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

  // The last outcome is cleared as the form opens, so a sentence about the
  // previous credential is not left standing beside the next one.
  const openForm = (fromEmpty: boolean) => {
    setDone(null);
    setFocusFirst(fromEmpty);
    form.open();
  };

  const rows = stored.data ? credentialRows(stored.data) : null;
  // Unknown while settings load; the runtime refuses for itself if it cannot store.
  const canStore = keychain !== false;
  const toggle = canStore ? (
    <button
      ref={toggleRef}
      type="button"
      className="settings-button"
      aria-expanded={form.adding}
      aria-controls={form.adding ? formId : undefined}
      onClick={() => (form.adding ? form.close() : openForm(false))}
    >
      <Icon name={form.adding ? "x" : "plus"} size={14} />
      {form.adding ? "Cancel" : "Add credential"}
    </button>
  ) : null;

  return (
    <Section
      id="settings-credentials"
      actions={toggle}
      intro={
        <>
          A credential is bound to its origin, so a run cannot spend it anywhere else. Spending
          it is a separate grant, <span className="mono">network.credential</span>, from reaching
          the origin.
        </>
      }
    >
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
      {form.adding ? (
        <CredentialForm form={form} formId={formId} focusFirst={focusFirst} />
      ) : null}
      <div className="panel">
        {rows === null ? (
          stored.error ? null : (
            <SkeletonRows count={1} />
          )
        ) : rows.length === 0 ? (
          <EmptyState
            icon="key"
            title="No credentials are stored"
            action={
              canStore && !form.adding ? (
                <button
                  type="button"
                  className="settings-button"
                  onClick={() => openForm(true)}
                >
                  <Icon name="plus" size={14} />
                  Add credential
                </button>
              ) : undefined
            }
          >
            A run that calls an API with a token needs one here, bound to that API&apos;s origin.
          </EmptyState>
        ) : (
          <List labelledBy="settings-credentials" className="stagger">
            {rows.map((row) => (
              <Item key={row.key}>
                <div className="row settings-credential">
                  <RowMark icon="lock" />
                  <div className="row-main">
                    <div className="row-title mono" title={row.label}>
                      <span className="settings-origin">{row.origin}</span>
                      <span className="settings-origin-sep"> / </span>
                      {row.name}
                    </div>
                    {row.account ? (
                      <div className="row-meta">
                        <span className="settings-account">
                          <Icon name="link" size={12} />
                          the token of {row.account}
                        </span>
                      </div>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    className="ghost settings-button"
                    onClick={() => setRemoving(row)}
                  >
                    <Icon name="trash" size={14} />
                    Remove
                    <span className="visually-hidden"> {row.label}</span>
                  </button>
                </div>
              </Item>
            ))}
          </List>
        )}
      </div>
      {done ? <p className="field-note settings-enter">{done}</p> : null}
      {canStore ? null : (
        <p className="field-note">
          With no keychain on this machine, a credential cannot be stored from here.
        </p>
      )}

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
