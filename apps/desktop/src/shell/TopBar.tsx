import { Icon } from "../components/icons";
import { tip } from "../components/Tooltip";
import type { Route, RouteName } from "../routes/route";
import { usingFixtures } from "../sdk/transport";
import { ApprovalsCount } from "./ApprovalsBadge";
import { Brand } from "./Brand";
import { SIDEBAR_ID } from "./layout";
import { labelFor, screenRoute } from "./nav";

/**
 * The narrow layout's top bar, in glass over the screen as it scrolls: the
 * menu that opens the sidebar as a drawer, the brand, the current screen's
 * name, and the two things that must stay one press away at any width — the
 * palette, and the approvals waiting on the operator.
 *
 * It is drawn only by the narrow layout's media query; at other widths the
 * sidebar carries all of this. `inert` while the drawer is open, with the rest
 * of the page behind it.
 */
export function TopBar({
  current,
  drawerOpen,
  onMenu,
  onOpenPalette,
  onNavigate,
  apple,
}: {
  current: RouteName;
  drawerOpen: boolean;
  onMenu: () => void;
  onOpenPalette: () => void;
  onNavigate: (route: Route) => void;
  apple: boolean;
}) {
  const onApprovals = current === "approvals";
  return (
    <header className="topbar" inert={drawerOpen}>
      <button
        type="button"
        className="icon-button"
        aria-expanded={drawerOpen}
        aria-controls={SIDEBAR_ID}
        onClick={onMenu}
        {...tip("Open navigation")}
      >
        <Icon name="menu" />
      </button>
      <Brand withName={false} />
      {/* Keyed on the screen so its name rises in with the screen it names. */}
      <span key={current} className="topbar-title screen-enter">
        {labelFor(current)}
      </span>
      {usingFixtures() ? (
        <span className="topbar-fixture" role="img" {...tip("Fixture data — not connected to a runtime")}>
          <Icon name="alert" size={16} />
        </span>
      ) : null}
      <span className="spacer" />
      <button
        type="button"
        className="icon-button"
        aria-keyshortcuts={apple ? "Meta+K" : "Control+K"}
        onClick={onOpenPalette}
        {...tip("Search or jump to…")}
      >
        <Icon name="search" />
      </button>
      <button
        type="button"
        className="topbar-approvals"
        aria-current={onApprovals ? "page" : undefined}
        onClick={() => onNavigate(screenRoute("approvals"))}
        data-tip="Approvals"
      >
        <Icon name="approvals" />
        <span className="visually-hidden">Approvals</span>
        <ApprovalsCount />
      </button>
    </header>
  );
}
