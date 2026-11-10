import type { DependencyChoice } from "../tasksModel";

/**
 * The tasks a new task can wait for, as a set of checkable cards.
 *
 * Cards rather than a multiple select: a list box hides that several may be
 * chosen behind a modifier key, and on one row Chrome draws it as a drop-down
 * reading "0 selected". A checkbox says what it is.
 */
export function WaitForPicker({
  id,
  choices,
  selected,
  onChange,
}: {
  id: string;
  choices: readonly DependencyChoice[];
  selected: readonly string[];
  onChange: (ids: string[]) => void;
}) {
  const toggle = (choice: string, on: boolean) =>
    onChange(on ? [...selected, choice] : selected.filter((each) => each !== choice));
  return (
    <fieldset id={id} className="wait-picker" aria-describedby={`${id}-note`}>
      <legend>Wait for</legend>
      <div className="checks">
        {choices.map((choice) => {
          const on = selected.includes(choice.id);
          return (
            <label key={choice.id} className={on ? "check on" : "check"}>
              <input
                type="checkbox"
                checked={on}
                onChange={(event) => toggle(choice.id, event.target.checked)}
              />
              <span className="check-name">{choice.label}</span>
            </label>
          );
        })}
      </div>
      <p id={`${id}-note`} className="field-note">
        Optional. A task that waits for others is queued, and starts once they all succeed.
      </p>
    </fieldset>
  );
}
