import { StatTile } from "../../components/common";
import type { OverviewTile } from "./overviewModel";

/**
 * The four facts at the top of the screen — provider keys, accounts, the
 * scheduler and the audit chain — each a button to the section that explains
 * it. A tile is loud only when what it counts needs a person.
 */
export function Overview({
  tiles,
  onJump,
}: {
  tiles: readonly OverviewTile[];
  onJump: (section: string) => void;
}) {
  return (
    <section className="grid stats settings-overview" aria-label="At a glance">
      {tiles.map((tile) => (
        <StatTile
          key={tile.section}
          label={tile.label}
          icon={tile.icon}
          value={tile.value}
          hint={tile.hint === "" ? undefined : tile.hint}
          tone={tile.tone}
          onClick={() => onJump(tile.section)}
        />
      ))}
    </section>
  );
}
