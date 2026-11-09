import { describe, expect, it } from "vitest";

import {
  createFormDirty,
  createInput,
  createProgress,
  groupCatalogue,
  nameProblem,
  newCreateForm,
  toggleTool,
} from "./createModel";
import { CATALOGUE, agent } from "./testData";

describe("create form", () => {
  it("opens clean", () => {
    expect(createFormDirty(newCreateForm())).toBe(false);
    expect(createFormDirty(null)).toBe(false);
  });

  it("is dirty after any field changes", () => {
    const form = newCreateForm();
    expect(createFormDirty({ ...form, name: "sales" })).toBe(true);
    expect(createFormDirty({ ...form, vision: "yes" })).toBe(true);
    expect(createFormDirty({ ...form, tools: toggleTool(form.tools, "terminal.exec") })).toBe(true);
  });

  it("compares tools as a set", () => {
    const form = newCreateForm();
    const again = toggleTool(toggleTool(form.tools, "filesystem.read"), "filesystem.read");
    expect(again).not.toEqual(form.tools);
    expect(createFormDirty({ ...form, tools: again })).toBe(false);
  });

  it("sends a base URL only for a provider that takes one", () => {
    const form = { ...newCreateForm(), name: " sales ", baseUrl: " http://localhost:11434/v1 " };
    expect(createInput({ ...form, provider: "ollama" }).base_url).toBe("http://localhost:11434/v1");
    expect(createInput({ ...form, provider: "anthropic" }).base_url).toBeNull();
    expect(createInput({ ...form, provider: "openai", baseUrl: "  " }).base_url).toBeNull();
    expect(createInput(form).name).toBe("sales");
  });

  it("maps vision's three states", () => {
    const form = newCreateForm();
    expect(createInput(form).vision).toBeNull();
    expect(createInput({ ...form, vision: "yes" }).vision).toBe(true);
    expect(createInput({ ...form, vision: "no" }).vision).toBe(false);
  });
});

describe("nameProblem", () => {
  const agents = [agent("sales", "anthropic", "claude-opus-5")];

  it("catches a name already taken, as the runtime would store it", () => {
    expect(nameProblem("sales", agents)).toBe("There is already an agent named “sales”.");
    expect(nameProblem("  sales ", agents)).toBe("There is already an agent named “sales”.");
  });

  it("keeps case, as the runtime does", () => {
    expect(nameProblem("Sales", agents)).toBeNull();
  });

  it("says nothing of an empty name, or before the list has loaded", () => {
    expect(nameProblem("   ", agents)).toBeNull();
    expect(nameProblem("sales", null)).toBeNull();
  });
});

describe("createProgress", () => {
  const agents = [agent("sales", "anthropic", "claude-opus-5")];

  it("waits for a name before the form is ready", () => {
    const progress = createProgress(newCreateForm(), agents);
    expect(progress).toEqual({ identity: false, model: true, tools: true, ready: false });
  });

  it("is ready with a fresh name and a model", () => {
    const form = { ...newCreateForm(), name: "ops" };
    expect(createProgress(form, agents).ready).toBe(true);
  });

  it("is not ready with a name already taken, or no model", () => {
    expect(createProgress({ ...newCreateForm(), name: "sales" }, agents).ready).toBe(false);
    expect(createProgress({ ...newCreateForm(), name: "ops", model: " " }, agents)).toMatchObject({
      model: false,
      ready: false,
    });
  });

  it("does not need tools, but marks the step only once one is granted", () => {
    const form = { ...newCreateForm(), name: "ops", tools: [] };
    expect(createProgress(form, agents)).toMatchObject({ tools: false, ready: true });
  });
});

describe("groupCatalogue", () => {
  it("groups by domain in the catalogue's order", () => {
    expect(
      groupCatalogue(CATALOGUE).map((group) => [group.domain, group.tools.map((t) => t.name)]),
    ).toEqual([
      ["filesystem", ["filesystem.read", "filesystem.write", "filesystem.search"]],
      ["terminal", ["terminal.exec"]],
    ]);
  });

  it("is empty for an empty catalogue", () => {
    expect(groupCatalogue([])).toEqual([]);
  });
});
