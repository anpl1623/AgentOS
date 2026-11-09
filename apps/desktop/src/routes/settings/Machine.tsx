import type { SettingsView } from "../../bindings/SettingsView";
import { CopyButton, type IconName } from "../../components/common";
import { RowMark, Section } from "./parts";

/**
 * The browser agents drive, or what to do when none was found.
 *
 * The path gets a Copy button for the same reason the storage paths do: it is
 * what an operator pastes into a terminal to check which browser that is.
 */
export function Browser({ settings }: { settings: SettingsView }) {
  return (
    <Section
      id="settings-browser"
      intro="The browser an agent's browser tools drive, when its policy lets it browse."
    >
      {settings.browser_path ? (
        <dl className="panel settings-paths">
          <PathRow
            icon="globe"
            label="Executable"
            what="browser"
            path={settings.browser_path}
          />
        </dl>
      ) : (
        <div className="banner warn prose" role="status">
          {settings.browser_hint}
        </div>
      )}
    </Section>
  );
}

/** Where AgentOS keeps its state, one path to a row. */
export function Storage({ settings }: { settings: SettingsView }) {
  return (
    <Section
      id="settings-storage"
      intro="Everything AgentOS knows is kept under these paths, on this machine and nowhere else."
    >
      <dl className="panel settings-paths">
        <PathRow
          icon="folder"
          label="Data directory"
          what="data directory"
          path={settings.data_dir}
        />
        <PathRow icon="agents" label="Workspaces" what="workspaces" path={settings.workspace} />
        <PathRow icon="activity" label="Database" what="database" path={settings.database} />
      </dl>
    </Section>
  );
}

/**
 * One path, with a Copy button: these are what an operator pastes into a
 * terminal or a ticket, and selecting a long path by hand is where a
 * character gets dropped. A label and its value, so a term and its
 * definition; the stylesheet puts them side by side where there is room.
 */
function PathRow({
  icon,
  label,
  what,
  path,
}: {
  icon: IconName;
  label: string;
  what: string;
  path: string;
}) {
  return (
    <div className="settings-path">
      <dt>
        <RowMark icon={icon} />
        <span>{label}</span>
      </dt>
      <dd>
        <span className="mono settings-path-value">{path}</span>
        <CopyButton text={path} label={`Copy the ${what} path`} />
      </dd>
    </div>
  );
}
