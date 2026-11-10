/**
 * The product's mark and name, at the head of the sidebar and in the top bar.
 *
 * The shield is the application icon's, so the chrome and the dock agree. It
 * sits on a tile in the accent gradient; the name beside it is hidden when the
 * sidebar is a rail, by the sidebar's own container query.
 */
export function Brand({ withName = true }: { withName?: boolean }) {
  return (
    <span className="brand">
      <span className="brand-tile" aria-hidden="true">
        <Mark />
      </span>
      {withName ? <span className="brand-name">AgentOS</span> : null}
    </span>
  );
}

/** The shield from the application icon, so the chrome and the dock agree. */
function Mark() {
  return (
    <svg className="brand-mark" viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M4.5 3.5h15v9.2c0 4.6-3.4 7.8-7.5 7.8s-7.5-3.2-7.5-7.8V3.5z"
        className="brand-mark-outer"
        fill="none"
        strokeWidth="2.4"
      />
      <circle
        className="brand-mark-inner"
        cx="12"
        cy="10.6"
        r="2.9"
        fill="none"
        strokeWidth="1.7"
      />
      <circle className="brand-mark-dot" cx="12" cy="10.6" r="0.9" />
    </svg>
  );
}
