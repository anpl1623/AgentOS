import { Component, type ErrorInfo, type ReactNode } from "react";

/**
 * What is shown in place of a tree that threw while rendering.
 *
 * Without a boundary, one render error unmounts the whole window and leaves it
 * blank, with no way back short of quitting. Two are mounted: one around the
 * whole app, and one around the screen inside the shell, so a broken screen
 * keeps the sidebar and the operator can go somewhere that works.
 */
export class ErrorBoundary extends Component<
  {
    children: ReactNode;
    /** What failed, for the heading: the whole interface or one screen. */
    scope: "window" | "screen";
    /**
     * Clears a caught error when it changes. The shell passes the address, so
     * moving to another screen tries again without remounting a screen that
     * did not fail.
     */
    resetKey?: string | undefined;
  },
  { error: Error | null }
> {
  override state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: unknown): { error: Error } {
    return { error: error instanceof Error ? error : new Error(String(error)) };
  }

  override componentDidCatch(error: unknown, info: ErrorInfo): void {
    console.error(`The ${this.props.scope} failed to render`, error, info.componentStack);
  }

  override componentDidUpdate(previous: { resetKey?: string | undefined }): void {
    if (this.state.error !== null && previous.resetKey !== this.props.resetKey) {
      this.setState({ error: null });
    }
  }

  override render(): ReactNode {
    const { error } = this.state;
    if (error === null) return this.props.children;
    return <Crash error={error} scope={this.props.scope} />;
  }
}

function Crash({ error, scope }: { error: Error; scope: "window" | "screen" }) {
  return (
    <div className="crash" role="alert">
      <h1>{scope === "screen" ? "This screen failed to draw" : "The interface failed"}</h1>
      <p>
        {scope === "screen"
          ? "Nothing the runtime is doing has stopped. Choose another screen, or reload the window."
          : "Nothing the runtime is doing has stopped; only the window failed. Reloading it starts the interface again."}
      </p>
      <pre className="crash-detail">{error.message || error.name}</pre>
      <div className="dialog-actions">
        <button type="button" className="primary" onClick={() => window.location.reload()}>
          Reload the window
        </button>
      </div>
    </div>
  );
}
