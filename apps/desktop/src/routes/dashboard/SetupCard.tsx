import type { CSSProperties } from "react";

import { Icon } from "../../components/common";
import type { Navigate } from "../route";
import { type ReadinessItem, setupProgress } from "./readinessModel";
import { Item, List } from "./Section";

/**
 * What stands between this installation and a first run, as a progress card.
 *
 * Drawn only while something is missing, under the figures and above all the
 * work, since none of that work can happen until it is done. A bar in the accent
 * gradient says how far along setup is; beneath it, each missing step is a
 * row that opens the screen where it is put right, and the steps already done
 * follow, ticked, so the bar's figure can be read off the list.
 */
export function SetupCard({
  missing,
  navigate,
}: {
  missing: readonly ReadinessItem[];
  navigate: Navigate;
}) {
  const progress = setupProgress(missing);
  return (
    <div className="dash-setup">
      <div className="dash-setup-head">
        <span className="dash-setup-mark" aria-hidden="true">
          <Icon name="sparkles" size={18} />
        </span>
        <div className="dash-setup-titles">
          <h2 id="dashboard-setup">Not set up yet</h2>
          <p className="dash-setup-sub">
            {progress.done} of {progress.total} ready. Finish these before giving an agent work.
          </p>
        </div>
        <span className="dash-setup-percent" aria-hidden="true">
          {progress.percent}%
        </span>
      </div>
      <div
        className="dash-setup-bar"
        role="progressbar"
        aria-label="Setup"
        aria-valuemin={0}
        aria-valuemax={progress.total}
        aria-valuenow={progress.done}
        aria-valuetext={`${progress.done} of ${progress.total} ready`}
      >
        <span
          className="dash-setup-bar-fill"
          style={{ "--w": `${progress.percent}%` } as CSSProperties}
        />
      </div>
      <List labelledBy="dashboard-setup" className="dash-setup-steps">
        {progress.steps.map((step) =>
          step.missing ? (
            <Item key={step.id}>
              <MissingStep item={step.missing} navigate={navigate} />
            </Item>
          ) : (
            <Item key={step.id} className="dash-setup-done">
              <Icon name="check" size={14} />
              {step.done}
            </Item>
          ),
        )}
      </List>
    </div>
  );
}

function MissingStep({ item, navigate }: { item: ReadinessItem; navigate: Navigate }) {
  return (
    <button
      type="button"
      className="dash-setup-step lift"
      data-spotlight
      onClick={() => navigate(item.target)}
    >
      <span className="dash-setup-step-mark" aria-hidden="true">
        <Icon name="alert" size={14} />
      </span>
      <span className="dash-setup-step-main">
        <span className="dash-setup-step-problem">{item.problem}</span>
        <span className="dash-setup-step-remedy">{item.remedy}</span>
      </span>
      <span className="dash-setup-step-go">
        {item.target.name === "agents" ? "Agents" : "Settings"}
        <Icon name="chevron-right" size={14} />
      </span>
    </button>
  );
}
