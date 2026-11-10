import { describe, expect, it } from "vitest";

import {
  beginPolicyDraft,
  checkedText,
  effectParts,
  lineCount,
  policyDirty,
  policySuperseded,
  ruleTone,
  splitRule,
} from "./policyModel";
import { policy } from "./testData";

describe("splitRule", () => {
  it("splits on the first separator", () => {
    expect(splitRule("browser.navigate => allow on [origin:http://127.0.0.1:8420]")).toEqual({
      capability: "browser.navigate",
      effect: "allow on [origin:http://127.0.0.1:8420]",
    });
    expect(splitRule("a.b => ask on [x => y]")).toEqual({
      capability: "a.b",
      effect: "ask on [x => y]",
    });
  });

  it("keeps a line without one whole rather than dropping it", () => {
    expect(splitRule("something the engine said")).toEqual({
      capability: "something the engine said",
      effect: "",
    });
  });
});

describe("ruleTone", () => {
  it("reads the effect's first word", () => {
    expect(ruleTone("allow on [origin:http://127.0.0.1:8420]")).toBe("ok");
    expect(ruleTone("ask on [path:/tmp]")).toBe("warn");
    expect(ruleTone("  Deny everywhere")).toBe("blocked");
  });

  it("is neutral for wording it has not seen, or none", () => {
    expect(ruleTone("escalate on [x]")).toBe("neutral");
    expect(ruleTone("")).toBe("neutral");
  });
});

describe("effectParts", () => {
  it("splits the verb from the resources it names", () => {
    expect(effectParts("allow on [origin:http://127.0.0.1:8420]")).toEqual({
      verb: "allow",
      rest: "on [origin:http://127.0.0.1:8420]",
    });
  });

  it("keeps a one-word or empty effect whole", () => {
    expect(effectParts("deny")).toEqual({ verb: "deny", rest: "" });
    expect(effectParts("  ")).toEqual({ verb: "", rest: "" });
  });
});

describe("policy drafts", () => {
  it("is clean until the text changes, and clean again when changed back", () => {
    const draft = beginPolicyDraft(policy());
    expect(policyDirty(draft)).toBe(false);
    expect(policyDirty({ ...draft, document: "default: allow\n" })).toBe(true);
    expect(policyDirty({ ...draft, document: "default: deny\n" })).toBe(false);
    expect(policyDirty(null)).toBe(false);
  });

  it("notices a save made elsewhere after editing began", () => {
    const draft = beginPolicyDraft(policy({ version: 3 }));
    expect(policySuperseded(draft, policy({ version: 3 }))).toBe(false);
    expect(policySuperseded(draft, policy({ version: 4 }))).toBe(true);
    expect(policySuperseded(null, policy())).toBe(false);
    expect(policySuperseded(draft, null)).toBe(false);
  });
});

describe("checkedText", () => {
  it("says the default and how many rules the engine read", () => {
    expect(checkedText(policy({ rules: ["a => allow", "b => allow"] }))).toBe(
      "Valid · default deny · 2 rules",
    );
    expect(checkedText(policy({ default_effect: "ask", rules: ["a => allow"] }))).toBe(
      "Valid · default ask · 1 rule",
    );
    expect(checkedText(policy())).toBe("Valid · default deny · 0 rules");
  });
});

describe("lineCount", () => {
  it("numbers every line the editor draws, including an empty last one", () => {
    expect(lineCount("")).toBe(1);
    expect(lineCount("default: deny")).toBe(1);
    expect(lineCount("default: deny\n")).toBe(2);
    expect(lineCount("a\n\nb")).toBe(3);
  });
});
