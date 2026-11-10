/**
 * Network credentials and the integration accounts bound to them. The two
 * lists change together, so they live together.
 */

import type { IntegrationTestView } from "../../bindings/IntegrationTestView";
import type { IntegrationView } from "../../bindings/IntegrationView";
import type { NetworkCredentialView } from "../../bindings/NetworkCredentialView";

import { minutesAgo } from "./clock";
import { Refusal } from "./refusal";
import { settings } from "./settings";

/**
 * Two credentials bound to the CRM's origin, and a GitHub account's token. A
 * fixture of a credential is an origin and a name, as the runtime's answer is:
 * there is no value here to show, so no screen built against this data can
 * learn to show one.
 *
 * Like the accounts below and the provider keys, the list can be removed from,
 * so the screens' empty and after-removal states can be reached. A removal
 * replaces the list rather than editing it: a screen that reads it again must
 * be handed a new array, as the runtime's answer is, or it would not redraw.
 */
export let credentials: NetworkCredentialView[] = [
  { origin: "http://127.0.0.1:8420", name: "default", account: null },
  { origin: "http://127.0.0.1:8420", name: "reports", account: null },
  // The token of the GitHub account `work`, which is a credential like any
  // other, and is listed as that account's.
  { origin: "https://api.github.com", name: "work", account: "GitHub account work" },
];

/**
 * The runtime's reduction of a URL to `scheme://host[:port]`, close enough to
 * build the credential form against: it refuses what `normalise_origin`
 * refuses most often, in the same words, and lower-cases and drops a default
 * port as it does. It is not the parser, which refuses more.
 */
function fixtureOrigin(url: string): string {
  const [scheme, rest] = url.split(/:\/\/(.*)/s);
  const lowered = scheme?.toLowerCase();
  if (rest === undefined || (lowered !== "http" && lowered !== "https")) {
    throw new Refusal(`\`${url}\` is not an http or https URL`);
  }
  const authority = rest.split(/[/\\?#]/)[0] ?? "";
  if (authority === "") throw new Refusal(`\`${url}\` has no host`);
  if (authority.includes("@")) {
    throw new Refusal(`\`${url}\` carries credentials; a URL with userinfo is refused`);
  }
  const [host, port] = authority.toLowerCase().split(/:(?=\d*$)/);
  if (port !== undefined && !(Number(port) >= 1 && Number(port) <= 65_535)) {
    throw new Refusal(`\`${url}\` has a port that is not a number from 1 to 65535`);
  }
  const fallback = lowered === "https" ? "443" : "80";
  return port === undefined || port === fallback
    ? `${lowered}://${host}`
    : `${lowered}://${host}:${port}`;
}

/**
 * Storing a credential, as the runtime answers it: the origin as it will be
 * matched and the name, never the secret, with the runtime's refusals in its
 * words. The fixture keeps nothing, so the list does not grow; a secret held
 * in a development page is still a secret held somewhere it should not be.
 */
export function setNetworkCredential(args: Record<string, unknown>): NetworkCredentialView {
  const { origin, name } = credentialAddress(args);
  const secret = String(args.secret ?? "").trim();
  if (secret === "") throw new Refusal("no secret was provided");
  if (/[\u0000-\u001f\u007f-\u009f]/.test(secret)) {
    throw new Refusal("a credential cannot contain line breaks or other control characters");
  }
  const account =
    credentials.find((stored) => stored.origin === origin && stored.name === name)?.account ??
    null;
  return { origin, name, account };
}

/** A credential's origin and name, read and refused as the runtime reads them. */
function credentialAddress(args: Record<string, unknown>): { origin: string; name: string } {
  const origin = fixtureOrigin(String(args.origin ?? "").trim());
  const name = String(args.name ?? "").trim();
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(name)) {
    throw new Refusal(
      `\`${name}\` cannot name a credential: use 1 to 64 letters, digits, \`_\` or \`-\``,
    );
  }
  return { origin, name };
}

/**
 * Removing a credential, as the runtime answers it: nothing, with the address
 * refused as storing refuses it, and a credential that is not there removed
 * all the same. An account whose token it was stays bound with no token, which
 * is how the runtime reaches the state the integrations list makes loud.
 */
export function removeNetworkCredential(args: Record<string, unknown>): null {
  const { origin, name } = credentialAddress(args);
  credentials = credentials.filter((stored) => stored.origin !== origin || stored.name !== name);
  integrations = integrations.map((integration) => ({
    ...integration,
    accounts: integration.accounts.map((account) =>
      account.origin === origin && account.label === name
        ? { ...account, credential_present: false }
        : account,
    ),
  }));
  return null;
}

/**
 * GitHub with two accounts. The second is bound to an Enterprise server on a
 * private network and its token has gone from the keychain, which is the state
 * the list exists to make loud; a fixture where every row is healthy would
 * never show it.
 */
export let integrations: IntegrationView[] = [
  {
    id: "github",
    display_name: "GitHub",
    default_host: "https://api.github.com",
    tools: settings.tools.filter((tool) => tool.domain === "github").map((tool) => tool.name),
    accounts: [
      {
        id: "acct-work",
        label: "work",
        host: "https://api.github.com",
        origin: "https://api.github.com",
        private_network: false,
        scopes: "repo, read:org",
        credential_present: true,
        created_at: minutesAgo(60 * 24 * 9),
        last_used_at: minutesAgo(40),
      },
      {
        id: "acct-ghe",
        label: "ghe",
        host: "https://ghe.internal.example/api/v3",
        origin: "https://ghe.internal.example",
        private_network: true,
        scopes: null,
        credential_present: false,
        created_at: minutesAgo(60 * 24 * 30),
        last_used_at: null,
      },
    ],
  },
];

/**
 * Binding an account, as the command answers it: nothing, or a refusal in the
 * runtime's words. Like the credential fixture it keeps nothing, so the list
 * does not change; the screen reads the list again, as it does for real.
 */
export function bindIntegration(args: Record<string, unknown>): null {
  const integration = integrations.find((candidate) => candidate.id === args.integration);
  if (!integration) {
    throw new Refusal(`there is no integration called \`${String(args.integration)}\``);
  }
  const label = String(args.label ?? "").trim();
  if (!/^[a-z0-9-]{1,32}$/.test(label)) {
    throw new Refusal(
      `\`${label}\` cannot label an account: use 1 to 32 lower-case letters, digits or \`-\``,
    );
  }
  if (integration.accounts.some((account) => account.label === label)) {
    throw new Refusal(
      `a ${integration.display_name} account labelled \`${label}\` is already bound`,
    );
  }
  if (args.host !== null && args.host !== undefined) fixtureOrigin(String(args.host));
  if (String(args.token ?? "").trim() === "") throw new Refusal("no token was provided");
  return null;
}

/**
 * Unbinding an account, as the runtime does it: the account goes, and its
 * token with it unless another bound account at the same origin and label
 * still uses it. The integration stays listed with whatever is left bound,
 * nothing included.
 */
export function unbindIntegration(args: Record<string, unknown>): null {
  const account = integrations
    .flatMap((integration) => integration.accounts)
    .find((candidate) => candidate.id === args.accountId);
  if (!account) throw new Refusal("there is no integration account with that identity");
  integrations = integrations.map((integration) => ({
    ...integration,
    accounts: integration.accounts.filter((candidate) => candidate.id !== account.id),
  }));
  const shared = integrations.some((integration) =>
    integration.accounts.some(
      (other) => other.origin === account.origin && other.label === account.label,
    ),
  );
  if (!shared) {
    credentials = credentials.filter(
      (stored) => stored.origin !== account.origin || stored.name !== account.label,
    );
  }
  return null;
}

/** A connection test: an account with no token is refused before anything is sent. */
export function testIntegration(args: Record<string, unknown>): IntegrationTestView {
  const account = integrations
    .flatMap((integration) => integration.accounts)
    .find((candidate) => candidate.id === args.accountId);
  if (!account) throw new Refusal("there is no integration account with that identity");
  if (!account.credential_present) {
    throw new Refusal(
      `no token is stored for \`${account.label}\` at ${account.origin}; ` +
        "remove the account and bind it again",
    );
  }
  return { outcome: "reachable", detail: "authenticated as octocat" };
}
