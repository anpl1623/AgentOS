import { avatarVariant, monogram } from "./agentsModel";

/**
 * An agent's face: its initials on a tile in the accent gradient.
 *
 * Decoration beside the name, which says the same thing, so it is hidden from
 * assistive technology. Which gradient an agent wears comes from its name, so
 * the same agent looks the same everywhere. A disabled agent's tile drops the
 * gradient for a quiet raised fill, so a list of cards shows at a glance which
 * agents can be given work, alongside the chip that says so in words.
 */
export function AgentAvatar({
  name,
  enabled,
  size = "md",
}: {
  name: string;
  enabled: boolean;
  size?: "md" | "sm" | undefined;
}) {
  const classes = ["agent-avatar", `v${avatarVariant(name)}`, size, enabled ? null : "off"]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={classes} aria-hidden="true">
      {monogram(name)}
    </span>
  );
}
