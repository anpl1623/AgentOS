/** The hues a live dot can take; each reads its colour from the status tokens. */
export type LiveTone = "ok" | "accent" | "warn" | "danger" | "info";

/**
 * A dot that says something is live: a run executing, a stream connected.
 *
 * The ring around it pulses while `pulse` is on, and only then, so a dot that
 * moves always means something is happening now. The pulse is a stylesheet
 * animation and stops under reduced motion; the dot stays.
 *
 * With a `label` it is an image with that name; without one it is decoration
 * beside text that already says the same thing.
 */
export function LiveDot({
  tone = "ok",
  pulse = true,
  label,
}: {
  tone?: LiveTone | undefined;
  pulse?: boolean | undefined;
  label?: string | undefined;
}) {
  return (
    <span
      className={pulse ? `live-dot tone-${tone} pulsing` : `live-dot tone-${tone}`}
      {...(label === undefined ? { "aria-hidden": true } : { role: "img", "aria-label": label })}
    />
  );
}
