/**
 * Integrations: the accounts an integration's tools act as, the form that
 * binds another, and how a connection test reads.
 */

import type { IntegrationAccountView } from "../../bindings/IntegrationAccountView";
import type { IntegrationTestView } from "../../bindings/IntegrationTestView";
import type { IntegrationView } from "../../bindings/IntegrationView";
import { asSentence, relative } from "./settingsModel";

/**
 * What an account label may be: it ends the key its token is stored under,
 * and a policy and a call name the account by it, so the alphabet is small.
 * The runtime holds the same rule and refuses for itself; this copy only
 * decides whether the form can be sent, and says why not as it is typed.
 */
const ACCOUNT_LABEL = /^[a-z0-9-]{1,32}$/;

/** Why a typed label cannot be used, or `null` when it can or nothing is typed yet. */
export function labelProblem(label: string): string | null {
  const trimmed = label.trim();
  if (trimmed === "" || ACCOUNT_LABEL.test(trimmed)) return null;
  return "A label is 1 to 32 lower-case letters, digits or hyphens.";
}

/**
 * Whether the bind form can be sent.
 *
 * The token is judged trimmed, as the command judges it: unlike a network
 * credential's secret, a token is trimmed before it is stored, so one of only
 * spaces would be refused and the button should not offer to send it.
 */
export function bindReady(label: string, token: string): boolean {
  const trimmed = label.trim();
  return trimmed !== "" && ACCOUNT_LABEL.test(trimmed) && token.trim() !== "";
}

/** The host as the command takes it: `null` for the integration's default. */
export function hostArgument(host: string): string | null {
  const trimmed = host.trim();
  return trimmed === "" ? null : trimmed;
}

/**
 * Whether the form offers the private-network permission for `host`.
 *
 * Only for a host other than the integration's own: that one is on the public
 * internet, so the permission could only widen where its token may go, and the
 * runtime refuses it there. The comparison is rough, trimmed and lower-cased
 * with a trailing `/` dropped; the runtime's is exact, and refuses for itself.
 */
export function offersPrivateNetwork(host: string, defaultHost: string): boolean {
  const typed = hostArgument(host);
  const plain = (url: string) => url.trim().toLowerCase().replace(/\/+$/, "");
  return typed !== null && plain(typed) !== plain(defaultHost);
}

/** One row of an integration's account list. */
export interface AccountRow {
  key: string;
  id: string;
  label: string;
  /**
   * The origin its token is stored for, which is where the Network
   * credentials section lists it, or `null` for a host that reads as none.
   */
  origin: string | null;
  /** The token is in the keychain. A row without one is drawn as broken. */
  present: boolean;
  /** Host, private network, the operator's note and last use, in that order. */
  meta: string[];
}

/**
 * The accounts as rows, by label.
 *
 * Built field by field, as the credential rows are, so a view that one day
 * carried a hint of the token would still not draw it. The private-network
 * permission is named whenever it is on: it is the one thing on the row that
 * widens where a request may go.
 */
export function accountRows(
  accounts: readonly IntegrationAccountView[],
  now: number,
): AccountRow[] {
  return accounts
    .map((account) => ({
      key: account.id,
      id: account.id,
      label: account.label,
      origin: account.origin,
      present: account.credential_present,
      meta: [
        account.host,
        ...(account.private_network ? ["private network allowed"] : []),
        ...(account.scopes !== null && account.scopes.trim() !== ""
          ? [`noted: ${account.scopes.trim()}`]
          : []),
        account.last_used_at === null
          ? "never used"
          : `last used ${relative(account.last_used_at, now)}`,
      ],
    }))
    .sort((a, b) => a.label.localeCompare(b.label) || a.id.localeCompare(b.id));
}

/** A finished connection test, as the row shows it. */
export interface TestLine {
  tone: "ok" | "warn" | "danger" | "neutral";
  verdict: string;
  detail: string;
}

/**
 * How a connection test reads.
 *
 * Unauthorised and unreachable are failures of the account, a wrong host one
 * of what was typed when it was bound; all three mean a run acting as it will
 * fail, so none is drawn as quietly as success. An outcome this screen does not
 * know is shown as the runtime named it rather than guessed at.
 */
export function testLine(result: IntegrationTestView): TestLine {
  const detail = asSentence(result.detail);
  switch (result.outcome) {
    case "reachable":
      return { tone: "ok", verdict: "Reachable", detail };
    case "unauthorised":
      return { tone: "danger", verdict: "Unauthorised", detail };
    case "wrong_host":
      return { tone: "warn", verdict: "Wrong host", detail };
    case "unreachable":
      return { tone: "danger", verdict: "Unreachable", detail };
    default:
      return { tone: "neutral", verdict: result.outcome, detail };
  }
}

/** How many accounts are bound across every integration. */
export function boundAccounts(integrations: readonly IntegrationView[]): number {
  return integrations.reduce((total, integration) => total + integration.accounts.length, 0);
}

/**
 * How many bound accounts have no token in the keychain: every call made as
 * one of them fails, which is why the overview counts them where it can be
 * seen before scrolling.
 */
export function brokenAccounts(integrations: readonly IntegrationView[]): number {
  return integrations.reduce(
    (total, integration) =>
      total + integration.accounts.filter((account) => !account.credential_present).length,
    0,
  );
}
