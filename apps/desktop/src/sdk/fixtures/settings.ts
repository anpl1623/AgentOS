/** Settings: the paths, the tool catalogue, and the providers' keys. */

import type { SettingsView } from "../../bindings/SettingsView";

import { Refusal } from "./refusal";

export const settings: SettingsView = {
  data_dir: "/Users/you/.agentos",
  workspace: "/Users/you/.agentos/workspace",
  database: "/Users/you/.agentos/agentos.db",
  keychain_available: true,
  keychain_reason: null,
  providers: [
    {
      id: "anthropic",
      configured: true,
      hint: "sk-a…9f2",
      source: "system keychain",
      note: "or set ANTHROPIC_API_KEY in the environment",
    },
    {
      id: "openai",
      configured: false,
      hint: null,
      source: null,
      note: "or set OPENAI_API_KEY in the environment",
    },
    { id: "ollama", configured: false, hint: null, source: null, note: "local; usually needs no key" },
    { id: "mock", configured: false, hint: null, source: null, note: "built in; no key needed" },
  ],
  browser_path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  browser_hint: null,
  tools: (
    [
      ["browser.navigate", "browser", "medium", true, "Open a URL in the agent's browser.",
        ["browser.navigate"]],
      ["browser.extract", "browser", "low", true, "Read the visible text of the page.",
        ["browser.read"]],
      ["browser.type", "browser", "high", false, "Type into a field on the page.",
        ["browser.interact"]],
      ["filesystem.read", "filesystem", "low", true, "Read a UTF-8 text file.",
        ["filesystem.read"]],
      ["filesystem.list", "filesystem", "low", true,
        "List the entries of a directory, one per line, marking directories with a trailing slash.",
        ["filesystem.list"]],
      ["filesystem.search", "filesystem", "low", true,
        "Search beneath a directory for entries whose name matches a glob, or for files containing some text.",
        ["filesystem.list", "filesystem.read"]],
      ["filesystem.write", "filesystem", "medium", false, "Write text to a file.",
        ["filesystem.write"]],
      ["filesystem.delete", "filesystem", "high", false, "Delete a file or directory.",
        ["filesystem.delete"]],
      ["terminal.exec", "terminal", "high", true, "Run a program directly with an argument vector.",
        ["terminal.exec"]],
      ["network.request", "network", "medium", true,
        "Make one HTTP request and return the status line, the response headers and the body.",
        ["network.fetch", "network.send", "network.credential"]],
      ["github.repos.get", "github", "medium", true, "Read a repository's description.",
        ["github.repos.read"]],
      ["github.issues.list", "github", "medium", true, "List a repository's issues.",
        ["github.issues.read"]],
      ["github.issues.get", "github", "medium", true, "Read one issue and its body.",
        ["github.issues.read"]],
      ["github.issues.create", "github", "high", true, "Open an issue.",
        ["github.issues.write"]],
      ["github.issues.comment", "github", "high", true, "Comment on an issue.",
        ["github.issues.write"]],
      ["github.issues.update", "github", "high", true,
        "Change an issue's state, title, body or labels.", ["github.issues.write"]],
      ["github.pulls.list", "github", "medium", true, "List a repository's pull requests.",
        ["github.pulls.read"]],
      ["github.pulls.get", "github", "medium", true, "Read one pull request, optionally its diff.",
        ["github.pulls.read"]],
      ["github.pulls.create", "github", "high", true, "Open a pull request.",
        ["github.pulls.write"]],
      ["github.pulls.comment", "github", "high", true, "Comment on a pull request.",
        ["github.pulls.write"]],
      ["github.pulls.merge", "github", "critical", true, "Merge a pull request.",
        ["github.pulls.merge"]],
      ["github.checks.list", "github", "medium", true, "List the checks run against a commit.",
        ["github.checks.read"]],
    ] as const
  ).map(([name, domain, risk, untrusted, description, capabilities]) => ({
    name,
    domain,
    risk,
    returns_untrusted_data: untrusted,
    description,
    capabilities: [...capabilities],
  })),
};

const PROVIDERS = ["anthropic", "openai", "ollama", "mock"];

/**
 * The providers' key state, which removing a key changes. See `credentials`
 * in integrations.ts.
 */
export let providers = settings.providers;

/** The runtime's refusal of a provider it cannot build. */
export function knownProvider(provider: string): void {
  if (!PROVIDERS.includes(provider)) {
    throw new Refusal(`unknown provider \`${provider}\`; expected one of ${PROVIDERS.join(", ")}`);
  }
}

/**
 * Removing a provider's key. The keychain's copy goes; the provider then reads
 * as the runtime would read one with no key anywhere, the environment included.
 */
export function removeProviderKey(args: Record<string, unknown>): null {
  const provider = String(args.provider);
  knownProvider(provider);
  providers = providers.map((each) =>
    each.id === provider ? { ...each, configured: false, hint: null, source: null } : each,
  );
  return null;
}
