import { EmptyState, type IconName } from "../../components/common";
import type { EmptyAction } from "./feedSignals";

/** The mark over an empty feed, and the words on the one action it offers. */
const EMPTY_ACTIONS: {
  readonly [K in EmptyAction]: { icon: IconName; label: string };
} = {
  retry: { icon: "alert", label: "Read the log again" },
  clear: { icon: "filter", label: "Clear filters" },
  "every-run": { icon: "tasks", label: "Show every run" },
  "every-event": { icon: "shield", label: "Show all activity" },
  tasks: { icon: "activity", label: "Go to Tasks" },
};

/**
 * The feed with nothing in it: why, and the step that fills it.
 *
 * The words and the step are decided together in the model (`emptyMessage`,
 * `emptyAction`), so the button never offers something the sentence did not
 * give as the reason.
 */
export function EmptyFeed({
  message,
  action,
  onAction,
}: {
  message: string;
  action: EmptyAction;
  onAction: { readonly [K in EmptyAction]: () => void };
}) {
  const { icon, label } = EMPTY_ACTIONS[action];
  return (
    <EmptyState
      icon={icon}
      title={message}
      action={
        <button type="button" onClick={onAction[action]}>
          {label}
        </button>
      }
    />
  );
}
