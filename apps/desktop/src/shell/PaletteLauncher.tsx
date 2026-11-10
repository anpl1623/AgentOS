import { Icon } from "../components/icons";
import { Kbd } from "../components/Kbd";

/**
 * The visible way into the command palette, for the person who does not know
 * the shortcut yet: a search field's shape that opens the palette, with the
 * shortcut beside it so the next time can be quicker.
 *
 * The shortcut is announced through `aria-keyshortcuts` rather than read out
 * of the key cap. In the rail it is an icon button, its words a tooltip.
 */
export function PaletteLauncher({
  onOpen,
  apple,
}: {
  onOpen: () => void;
  /** Whether to write the shortcut as `⌘K` rather than `Ctrl K`. */
  apple: boolean;
}) {
  const keys = apple ? "⌘K" : "Ctrl K";
  return (
    <button
      type="button"
      className="palette-launcher"
      onClick={onOpen}
      aria-keyshortcuts={apple ? "Meta+K" : "Control+K"}
      data-tip={`Search or jump to  ${keys}`}
      data-tip-side="right"
    >
      <Icon name="search" size={16} className="palette-launcher-icon" />
      <span className="palette-launcher-label">Search or jump to…</span>
      <Kbd hidden>{keys}</Kbd>
    </button>
  );
}
