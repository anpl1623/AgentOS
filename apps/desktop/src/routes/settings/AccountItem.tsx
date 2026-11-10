import { useState } from "react";

import type { IntegrationTestView } from "../../bindings/IntegrationTestView";
import { ErrorBanner, Icon } from "../../components/common";
import { api, describeError } from "../../sdk/client";
import { type AccountRow, testLine } from "./integrationsModel";
import { Fact, RowMark } from "./parts";

/**
 * One bound account, with a connection test.
 *
 * An account whose token is gone is drawn as broken and says what to do, and
 * is not offered a test: the runtime would refuse it before sending anything.
 */
export function AccountItem({
  row,
  integration,
  onRemove,
}: {
  row: AccountRow;
  integration: string;
  onRemove: () => void;
}) {
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<IntegrationTestView | null>(null);
  const [testError, setTestError] = useState<string | null>(null);

  const test = async () => {
    setTesting(true);
    setResult(null);
    setTestError(null);
    try {
      setResult(await api.testIntegration(row.id));
    } catch (failure) {
      setTestError(describeError(failure));
    } finally {
      setTesting(false);
    }
  };

  const line = result ? testLine(result) : null;

  return (
    <div className={row.present ? "row stacked settings-item" : "row stacked settings-item broken"}>
      <div className="settings-item-head">
        <RowMark icon="user" tone={row.present ? "ok" : "danger"} />
        <div className="row-main">
          <div className="row-title mono">{row.label}</div>
          <div className="row-meta">
            {row.meta.map((part) => (
              <span key={part}>{part}</span>
            ))}
          </div>
        </div>
        <div className="settings-item-actions">
          {row.present ? (
            <Fact tone="ok" noun="token">
              Token stored
            </Fact>
          ) : (
            <Fact tone="danger" noun="token">
              No token
            </Fact>
          )}
          {row.present ? (
            <button
              type="button"
              className="ghost settings-button"
              disabled={testing}
              onClick={() => void test()}
            >
              <Icon name="zap" size={14} />
              {testing ? "Testing…" : "Test"}
              <span className="visually-hidden"> {row.label}</span>
            </button>
          ) : null}
          <button type="button" className="ghost settings-button" onClick={onRemove}>
            <Icon name="trash" size={14} />
            Remove
            <span className="visually-hidden"> {row.label}</span>
          </button>
        </div>
      </div>
      {row.present ? null : (
        <div className="settings-item-body">
          <div className="banner error flush">
            The keychain holds no token for this account, so every {integration} call made as it
            fails. Remove it and add it again with a token
            {row.origin ? (
              <>
                , or store one under Network credentials as{" "}
                <span className="mono">
                  {row.origin} / {row.label}
                </span>
              </>
            ) : null}
            .
          </div>
        </div>
      )}
      <div role="status" aria-live="polite" aria-busy={testing}>
        {line ? (
          <div className="settings-item-body settings-test settings-enter">
            <Fact tone={line.tone} noun="connection">
              {line.verdict}
            </Fact>
            <span className="muted">{line.detail}</span>
          </div>
        ) : null}
      </div>
      {testError ? (
        <div className="settings-item-body settings-enter">
          <ErrorBanner message={`The test could not run: ${testError}`} />
        </div>
      ) : null}
    </div>
  );
}
