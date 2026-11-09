/**
 * Builders the settings models' tests share. Never imported by the screen,
 * so none of it reaches the application bundle.
 */

import type { AgentSummary } from "../../bindings/AgentSummary";
import type { IntegrationAccountView } from "../../bindings/IntegrationAccountView";
import type { SchedulerView } from "../../bindings/SchedulerView";
import type { ToolView } from "../../bindings/ToolView";

export const NOW = Date.parse("2026-10-06T12:00:00Z");
export const minutesFrom = (n: number) => new Date(NOW + n * 60_000).toISOString();

export function tool(name: string, risk: string, extra: Partial<ToolView> = {}): ToolView {
  return {
    name,
    domain: name.split(".")[0] ?? name,
    description: `The ${name} tool.`,
    risk,
    returns_untrusted_data: false,
    capabilities: [name],
    ...extra,
  };
}

export function agent(name: string, tools: string[], status = "enabled"): AgentSummary {
  return {
    id: `id-${name}`,
    name,
    provider: "mock",
    model: "mock",
    status,
    tools,
    max_steps: 10,
    created_at: minutesFrom(-1000),
  };
}

export function scheduler(extra: Partial<SchedulerView> = {}): SchedulerView {
  return {
    running: false,
    tick_seconds: 30,
    max_concurrent_runs: 1,
    started_at: null,
    error: null,
    active_schedules: 2,
    next_fire_at: minutesFrom(30),
    overdue: 0,
    runnable_tasks: 1,
    unreachable_tasks: 0,
    ...extra,
  };
}

export function account(
  label: string,
  extra: Partial<IntegrationAccountView> = {},
): IntegrationAccountView {
  return {
    id: `acct-${label}`,
    label,
    host: "https://api.github.com",
    origin: "https://api.github.com",
    private_network: false,
    scopes: null,
    credential_present: true,
    created_at: minutesFrom(-600),
    last_used_at: null,
    ...extra,
  };
}
