/**
 * Network credentials: secrets a run may send to one origin and nowhere else,
 * shown as an origin and a name, never a value.
 */

import type { NetworkCredentialView } from "../../bindings/NetworkCredentialView";

/** A stored credential as the runtime lists it: where it may go, and its name. */
export type StoredCredential = NetworkCredentialView;

/** One row of the credentials list. */
export interface CredentialRow {
  /** Stable across reloads, and unambiguous whatever the two parts contain. */
  key: string;
  origin: string;
  name: string;
  /** `{origin} / {name}`, which is everything the row ever shows of the secret. */
  label: string;
  /**
   * The integration account whose token this is, such as `GitHub account
   * work`, or `null`. Removing or replacing the credential removes or
   * replaces that account's token, and the screen says so first.
   */
  account: string | null;
}

/**
 * The credentials as rows, by origin and then name.
 *
 * Built field by field from the origin and the name and nothing else. Should
 * a later view carry more, a value or a hint of one, it still never reaches
 * the screen: a masked secret tells whoever is looking over a shoulder its
 * length and its first characters, which is more than this list has any need
 * to show.
 */
export function credentialRows(stored: readonly StoredCredential[]): CredentialRow[] {
  return stored
    .map(({ origin, name, account }) => ({
      key: JSON.stringify([origin, name]),
      origin,
      name,
      label: `${origin} / ${name}`,
      account: account ?? null,
    }))
    .sort((a, b) => a.origin.localeCompare(b.origin) || a.name.localeCompare(b.name));
}

/**
 * Whether the add form can be sent.
 *
 * Origin and name are trimmed for the check. The secret is not: it is sent
 * exactly as typed, and trimming it here would judge a value other than the
 * one that will be stored.
 */
export function credentialReady(origin: string, name: string, secret: string): boolean {
  return origin.trim() !== "" && name.trim() !== "" && secret !== "";
}
