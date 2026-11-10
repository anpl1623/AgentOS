import type { ToolView } from "../../bindings/ToolView";
import { ErrorBanner, Icon, Loading, Risk } from "../../components/common";
import { domainIcon } from "./agentsModel";
import { groupCatalogue } from "./createModel";

/**
 * The tools a new agent is offered, grouped by domain, each with its baseline
 * risk beside it so the cost of a tick is visible before it is made.
 */
export function ToolPicker({
  catalogue,
  catalogueError,
  selected,
  onToggle,
}: {
  catalogue: readonly ToolView[] | null;
  catalogueError: string | null;
  selected: readonly string[];
  onToggle: (tool: string) => void;
}) {
  const groups = catalogue ? groupCatalogue(catalogue) : [];
  return (
    <fieldset className="tool-picker">
      <legend className="visually-hidden">Tools to offer</legend>
      <p className="field-note tool-picker-note">
        Granting a tool only offers it to the model; the policy still decides each call.
      </p>
      {catalogueError ? (
        <ErrorBanner message={`The tool catalogue could not be read: ${catalogueError}`} />
      ) : null}
      {catalogue === null && catalogueError === null ? <Loading what="tools" /> : null}
      {groups.map((group) => {
        const chosen = group.tools.filter((tool) => selected.includes(tool.name)).length;
        return (
          <div className="tool-group" key={group.domain}>
            <div className="tool-group-head">
              <Icon name={domainIcon(group.domain)} size={15} />
              <span className="tool-group-name">{group.domain}</span>
              <span className="tool-group-count">
                {chosen} of {group.tools.length}
              </span>
            </div>
            <div className="checks">
              {group.tools.map((tool) => {
                const on = selected.includes(tool.name);
                return (
                  <label key={tool.name} className={on ? "check on" : "check"}>
                    <input type="checkbox" checked={on} onChange={() => onToggle(tool.name)} />
                    <span className="check-text">
                      <span className="check-name">{tool.name}</span>
                      {tool.returns_untrusted_data ? (
                        <span className="check-note">reads external data</span>
                      ) : null}
                    </span>
                    <Risk level={tool.risk} />
                  </label>
                );
              })}
            </div>
          </div>
        );
      })}
    </fieldset>
  );
}
