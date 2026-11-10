/**
 * Builders for the agents screen's model tests.
 *
 * Shared by the test files beside each model, so that an agent, a tool or a
 * policy means the same thing in every one of them. Nothing on screen imports
 * this module.
 */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { MemoryView } from "../../bindings/MemoryView";
import type { PolicyView } from "../../bindings/PolicyView";
import type { ToolGrantView } from "../../bindings/ToolGrantView";
import type { ToolView } from "../../bindings/ToolView";

export function agent(
  name: string,
  provider: string,
  model: string,
  tools: string[] = [],
): AgentSummary {
  return {
    id: `id-${name}`,
    name,
    provider,
    model,
    status: "enabled",
    tools,
    max_steps: 24,
    created_at: "2026-10-01T00:00:00Z",
  };
}

export function tool(
  name: string,
  risk: string,
  capabilities: string[] = [name],
  untrusted = false,
): ToolView {
  return {
    name,
    domain: name.split(".")[0] ?? name,
    description: `does ${name}`,
    risk,
    returns_untrusted_data: untrusted,
    capabilities,
  };
}

export function grant(
  name: string,
  reaches: Record<string, string>,
  registered = true,
): ToolGrantView {
  const capabilities = Object.entries(reaches).map(([capability, reach]) => ({
    capability,
    reach,
  }));
  return { tool: name, registered, reach: "denied", capabilities };
}

export function policy(extra: Partial<PolicyView> = {}): PolicyView {
  return {
    document: "default: deny\n",
    version: 3,
    default_effect: "deny",
    max_risk: null,
    taint_enabled: true,
    taint_threshold: "high",
    rules: [],
    ...extra,
  };
}

export function memory(id: string, extra: Partial<MemoryView> = {}): MemoryView {
  return {
    id,
    agent_id: "id-sales",
    kind: "fact",
    content: "The CRM is at port 8420.",
    source: "user",
    source_untrusted: false,
    reaches_the_prompt: true,
    confidence: 1,
    task_id: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...extra,
  };
}

export const CATALOGUE = [
  tool("filesystem.read", "low"),
  tool("filesystem.write", "medium"),
  tool("terminal.exec", "high", ["terminal.exec"], true),
  tool("filesystem.search", "low", ["filesystem.list", "filesystem.read"]),
];
