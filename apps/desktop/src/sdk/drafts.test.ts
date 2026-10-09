import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clearDraft,
  confirmLeave,
  latchInitial,
  readDraft,
  registerGuard,
  writeDraft,
} from "./drafts";

const unregister: (() => void)[] = [];

function guard(message: string | null): void {
  unregister.push(registerGuard(() => message));
}

afterEach(() => {
  for (const each of unregister.splice(0)) each();
});

describe("confirmLeave", () => {
  it("allows leaving without asking when no guard has anything to lose", async () => {
    guard(null);
    const ask = vi.fn(() => false);
    await expect(confirmLeave(ask)).resolves.toBe(true);
    expect(ask).not.toHaveBeenCalled();
  });

  it("asks once, with every guard's message, and honours the answer", async () => {
    guard("The policy has unsaved changes.");
    guard("The objective has not been started.");
    const ask = vi.fn((_message: string) => Promise.resolve(false));
    await expect(confirmLeave(ask)).resolves.toBe(false);
    expect(ask).toHaveBeenCalledTimes(1);
    expect(ask.mock.calls[0]?.[0]).toContain("unsaved changes");
    expect(ask.mock.calls[0]?.[0]).toContain("not been started");
  });

  it("leaves when the person agrees", async () => {
    guard("Unsaved.");
    await expect(confirmLeave(() => true)).resolves.toBe(true);
  });

  it("stays when there is no way to ask", async () => {
    // No window in this environment, so no default confirmer exists.
    guard("Unsaved.");
    await expect(confirmLeave(null)).resolves.toBe(false);
  });

  it("stops consulting a guard once it is unregistered", async () => {
    guard("Unsaved.");
    for (const each of unregister.splice(0)) each();
    await expect(confirmLeave(() => false)).resolves.toBe(true);
  });
});

describe("drafts", () => {
  it("outlive the component that wrote them until cleared", () => {
    writeDraft("objective", "Follow up with Acme");
    expect(readDraft<string>("objective")).toBe("Follow up with Acme");
    clearDraft("objective");
    expect(readDraft("objective")).toBeUndefined();
  });
});

describe("the starting value of a draft", () => {
  it("is kept while the key stays the same, whatever is passed on later renders", () => {
    const first = latchInitial(null, "policy:a", { document: "a" });
    const again = latchInitial(first, "policy:a", { document: "a, re-rendered" });
    expect(again).toBe(first);
    expect(again.initial.document).toBe("a");
  });

  it("is taken afresh when the key changes, so one agent's policy is not another's draft", () => {
    // A detail screen that stays mounted across a route change: agent A's
    // document must not become the starting value of agent B's draft.
    const a = latchInitial(null, "policy:a", "document of a");
    const b = latchInitial(a, "policy:b", "document of b");
    expect(b).toEqual({ key: "policy:b", initial: "document of b" });
  });
});
