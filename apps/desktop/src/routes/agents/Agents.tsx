import type { Navigate, Route } from "../route";
import { AgentDetail } from "./AgentDetail";
import { AgentList } from "./AgentList";

/**
 * The agents screen: every agent as a grid of cards, or one agent in detail
 * when the route names it.
 *
 * Composition only. The list and the detail page are their own modules, each
 * section of the detail page is too, and every decision they draw is a pure
 * function in one of the `*Model.ts` files beside them.
 */
export function Agents({
  route,
  navigate,
}: {
  route: Route & { name: "agents" };
  navigate: Navigate;
}) {
  if (route.agent) {
    return <AgentDetail name={route.agent} navigate={navigate} />;
  }
  return <AgentList navigate={navigate} />;
}
