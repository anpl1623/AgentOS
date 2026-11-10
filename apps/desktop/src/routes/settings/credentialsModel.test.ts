import { describe, expect, it } from "vitest";

import { credentialReady, credentialRows } from "./credentialsModel";

describe("credentialRows", () => {
  it("orders by origin then name, labelled origin / name", () => {
    const rows = credentialRows([
      { origin: "https://b.example", name: "token", account: null },
      { origin: "https://a.example", name: "zeta", account: null },
      { origin: "https://a.example", name: "alpha", account: null },
    ]);
    expect(rows.map((row) => row.label)).toEqual([
      "https://a.example / alpha",
      "https://a.example / zeta",
      "https://b.example / token",
    ]);
  });

  it("never carries a value, even when the view it is given does", () => {
    // A future view that grew a value, a hint or a mask must still not reach
    // the screen through these rows.
    const leaky = [
      {
        origin: "https://crm.example",
        name: "api",
        account: null,
        value: "s3cr3t",
        hint: "s3…t",
        masked: "••••",
      },
    ];
    const [row] = credentialRows(leaky);
    expect(Object.keys(row!).sort()).toEqual(["account", "key", "label", "name", "origin"]);
    expect(JSON.stringify(row)).not.toContain("s3");
    expect(JSON.stringify(row)).not.toContain("••••");
  });

  it("keys a row unambiguously whatever its parts contain", () => {
    const [first, second] = credentialRows([
      { origin: "https://a / b", name: "c", account: null },
      { origin: "https://a", name: "b / c", account: null },
    ]);
    expect(first!.label).toBe(second!.label);
    expect(first!.key).not.toBe(second!.key);
  });

  it("names the account a credential is the token of, and no other", () => {
    const [deploy, work] = credentialRows([
      { origin: "https://api.github.com", name: "work", account: "GitHub account work" },
      { origin: "https://api.github.com", name: "deploy", account: null },
    ]);
    expect(deploy!.account).toBeNull();
    expect(work!.account).toBe("GitHub account work");
  });
});

describe("credentialReady", () => {
  it("needs an origin, a name and a secret", () => {
    expect(credentialReady("https://crm.example", "api", "s")).toBe(true);
    expect(credentialReady(" ", "api", "s")).toBe(false);
    expect(credentialReady("https://crm.example", "", "s")).toBe(false);
    expect(credentialReady("https://crm.example", "api", "")).toBe(false);
  });

  it("judges the secret exactly as typed, without trimming it", () => {
    expect(credentialReady("https://crm.example", "api", " ")).toBe(true);
  });
});
