import { describe, expect, it } from "vitest";

import {
  accountRows,
  bindReady,
  boundAccounts,
  brokenAccounts,
  hostArgument,
  labelProblem,
  offersPrivateNetwork,
  testLine,
} from "./integrationsModel";
import { NOW, account, minutesFrom } from "./testData";

const GITHUB = {
  id: "github",
  display_name: "GitHub",
  default_host: "https://api.github.com",
  tools: [],
};

describe("account labels", () => {
  it("takes what the runtime takes and explains what it refuses", () => {
    expect(labelProblem("")).toBeNull();
    expect(labelProblem("work")).toBeNull();
    expect(labelProblem(" ghe-01 ")).toBeNull();
    expect(labelProblem("x".repeat(32))).toBeNull();
    // It ends a keychain key and is named in policy, so no dots, capitals or more.
    for (const bad of ["Work", "work.alt", "work_alt", "x".repeat(33), "wörk"]) {
      expect(labelProblem(bad)).toMatch(/1 to 32 lower-case/);
    }
  });
});

describe("bindReady", () => {
  it("needs a valid label and a token", () => {
    expect(bindReady("work", "ghp_x")).toBe(true);
    expect(bindReady("", "ghp_x")).toBe(false);
    expect(bindReady("Work", "ghp_x")).toBe(false);
    expect(bindReady("work", "")).toBe(false);
  });

  it("judges the token trimmed, as the command that stores it does", () => {
    expect(bindReady("work", "   ")).toBe(false);
    expect(bindReady("work", " ghp_x\n")).toBe(true);
  });
});

describe("hostArgument", () => {
  it("sends nothing for an empty host, so the integration's default applies", () => {
    expect(hostArgument("  ")).toBeNull();
    expect(hostArgument(" https://ghe.example/api/v3 ")).toBe("https://ghe.example/api/v3");
  });
});

describe("offersPrivateNetwork", () => {
  const github = "https://api.github.com";

  it("is offered for a server of the operator's own", () => {
    expect(offersPrivateNetwork("https://ghe.example/api/v3", github)).toBe(true);
  });

  it("is not offered for no host, or for the service's own however it is typed", () => {
    expect(offersPrivateNetwork("", github)).toBe(false);
    expect(offersPrivateNetwork("   ", github)).toBe(false);
    expect(offersPrivateNetwork(" HTTPS://API.github.com/ ", github)).toBe(false);
  });
});

describe("accountRows", () => {
  it("orders by label and says where each account goes and when it was used", () => {
    const rows = accountRows(
      [
        account("work", { last_used_at: minutesFrom(-120) }),
        account("ghe", {
          host: "https://ghe.example/api/v3",
          private_network: true,
          scopes: " repo, read:org ",
        }),
      ],
      NOW,
    );
    expect(rows.map((row) => row.label)).toEqual(["ghe", "work"]);
    expect(rows[0]!.meta).toEqual([
      "https://ghe.example/api/v3",
      "private network allowed",
      "noted: repo, read:org",
      "never used",
    ]);
    expect(rows[1]!.meta).toEqual(["https://api.github.com", "last used 2h ago"]);
  });

  it("keeps an account whose token is gone, marked as such", () => {
    const [row] = accountRows([account("work", { credential_present: false })], NOW);
    expect(row!.present).toBe(false);
  });

  it("never carries a token, even when the view it is given does", () => {
    const leaky = [{ ...account("work"), token: "ghp_s3cr3t", hint: "ghp_…3t" }];
    const [row] = accountRows(leaky, NOW);
    expect(Object.keys(row!).sort()).toEqual(["id", "key", "label", "meta", "origin", "present"]);
    expect(JSON.stringify(row)).not.toContain("s3cr3t");
    expect(JSON.stringify(row)).not.toContain("ghp_");
  });
});

describe("testLine", () => {
  it("draws every failure louder than success", () => {
    expect(testLine({ outcome: "reachable", detail: "authenticated as octocat" })).toEqual({
      tone: "ok",
      verdict: "Reachable",
      detail: "Authenticated as octocat.",
    });
    expect(testLine({ outcome: "unauthorised", detail: "401" }).tone).toBe("danger");
    expect(testLine({ outcome: "wrong_host", detail: "not a GitHub API" }).tone).toBe("warn");
    expect(testLine({ outcome: "unreachable", detail: "timed out" }).tone).toBe("danger");
  });

  it("names an outcome it does not know rather than guessing", () => {
    expect(testLine({ outcome: "rate_limited", detail: "" })).toEqual({
      tone: "neutral",
      verdict: "rate_limited",
      detail: "",
    });
  });
});

describe("boundAccounts", () => {
  it("counts across integrations", () => {
    expect(boundAccounts([])).toBe(0);
    expect(boundAccounts([{ ...GITHUB, accounts: [] }])).toBe(0);
    expect(boundAccounts([{ ...GITHUB, accounts: [account("a"), account("b")] }])).toBe(2);
  });
});

describe("brokenAccounts", () => {
  it("counts only the accounts whose token is gone, across integrations", () => {
    expect(brokenAccounts([])).toBe(0);
    const gitlab = { ...GITHUB, id: "gitlab", display_name: "GitLab" };
    expect(
      brokenAccounts([
        { ...GITHUB, accounts: [account("a"), account("b", { credential_present: false })] },
        { ...gitlab, accounts: [account("c", { credential_present: false })] },
      ]),
    ).toBe(2);
  });
});
