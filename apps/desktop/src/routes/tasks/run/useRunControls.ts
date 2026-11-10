import { useEffect, useState } from "react";

import { api, describeError } from "../../../sdk/client";
import type { Navigate } from "../../route";
import type { Coalescer } from "./refreshModel";
import type { FailureAnchor, TraceMode } from "./runModel";
import type { SpanTarget } from "./timelineModel";

/** The element id of a row on this screen, for scrolling to it. */
export function rowId(target: SpanTarget): string {
  switch (target.kind) {
    case "step":
      return `run-step-${target.ordinal}`;
    case "execution":
      return `run-execution-${target.id}`;
    case "approval":
      return `run-approval-${target.id}`;
  }
}

/** Stopping and retrying the run, and what the screen says while it does. */
export interface RunActions {
  /** The operator asked the run to stop; it is `cancelling` until the runtime says it ended. */
  cancelRequested: boolean;
  retrying: boolean;
  error: string | null;
  cancel: () => Promise<void>;
  retry: () => Promise<void>;
}

/**
 * The Stop and Retry actions.
 *
 * Stop is a safety control, so it acts on the first press: no confirmation
 * stands between an operator and an agent that is doing the wrong thing. What
 * it does not do is claim more than the runtime has said — the run is shown
 * as `cancelling` until a read reports it ended.
 */
export function useRunActions(
  runId: string,
  taskId: string | null,
  navigate: Navigate,
  refresh: Coalescer,
): RunActions {
  const [cancelRequested, setCancelRequested] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Each action ignores a press while its own request runs. The buttons are
  // aria-disabled rather than disabled then, so they keep focus, and a key
  // press still reaches them.
  const cancel = async () => {
    if (cancelRequested) return;
    setCancelRequested(true);
    setError(null);
    try {
      const stopping = await api.cancelRun(runId);
      // Nothing in this process was running it; the next read says what is.
      if (!stopping) setCancelRequested(false);
    } catch (failure) {
      setCancelRequested(false);
      setError(`The run could not be stopped: ${describeError(failure)}`);
    }
    refresh.schedule();
  };

  const retry = async () => {
    if (taskId === null || retrying) return;
    setRetrying(true);
    setError(null);
    try {
      const started = await api.retryTask(taskId);
      navigate({ name: "tasks", runId: started.run_id });
    } catch (failure) {
      setError(`The task could not be retried: ${describeError(failure)}`);
      setRetrying(false);
    }
  };

  return { cancelRequested, retrying, error, cancel, retry };
}

/** How the trace is being read: its mode, which calls are open, and finding a row. */
export interface TraceReading {
  mode: TraceMode;
  setMode: (mode: TraceMode) => void;
  expanded: ReadonlySet<string>;
  toggle: (id: string) => void;
  /** Open every call in `ids`, or close them all. */
  setAll: (ids: readonly string[], open: boolean) => void;
  /** Bring a row into view, switching mode or opening a call if that is what shows it. */
  reveal: (target: SpanTarget | FailureAnchor) => void;
}

export function useTraceReading(): TraceReading {
  const [mode, setMode] = useState<TraceMode>("timeline");
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [scrollTo, setScrollTo] = useState<{ id: string; nonce: number } | null>(null);

  // Scroll after the render that put the row on screen: switching mode or
  // expanding a call happens in the same update as the request.
  useEffect(() => {
    if (scrollTo === null) return;
    const row = document.getElementById(scrollTo.id);
    row?.scrollIntoView({ block: "center" });
    row?.focus({ preventScroll: true });
  }, [scrollTo]);

  const reveal = (target: SpanTarget | FailureAnchor) => {
    if (target.kind === "execution") {
      setExpanded((open) => new Set(open).add(target.id));
      if (mode === "steps") setMode("timeline");
    } else if (target.kind === "step" && mode === "calls") {
      setMode("timeline");
    }
    setScrollTo((previous) => ({ id: rowId(target), nonce: (previous?.nonce ?? 0) + 1 }));
  };

  const toggle = (id: string) =>
    setExpanded((open) => {
      const next = new Set(open);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const setAll = (ids: readonly string[], open: boolean) =>
    setExpanded((current) => {
      const next = new Set(current);
      for (const id of ids) {
        if (open) next.add(id);
        else next.delete(id);
      }
      return next;
    });

  return { mode, setMode, expanded, toggle, setAll, reveal };
}
