import type { Dispatch, SetStateAction } from "react";
import { ConnectionHealthSummary } from "./ConnectionHealthSummary";
import type {
  AutopilotSendPolicyRecord,
  EmailConnectionRecord,
  OAuthStartResponse,
  RunnerControlRecord,
  TransportStatusRecord,
} from "../types";

type GuideScopeType = "autopilot" | "run" | "approval" | "outcome";

type Props = {
  oauthProvider: "gmail" | "microsoft365";
  setOauthProvider: Dispatch<SetStateAction<"gmail" | "microsoft365">>;
  oauthClientId: string;
  setOauthClientId: Dispatch<SetStateAction<string>>;
  oauthRedirectUri: string;
  setOauthRedirectUri: Dispatch<SetStateAction<string>>;
  saveOauthSetup: () => void;
  transportStatus: TransportStatusRecord | null;
  relaySubscriberTokenInput: string;
  setRelaySubscriberTokenInput: Dispatch<SetStateAction<string>>;
  saveRelaySubscriberToken: () => void;
  removeRelaySubscriberToken: () => void;
  watcherAutopilotId: string;
  setWatcherAutopilotId: Dispatch<SetStateAction<string>>;
  watcherMaxItems: number;
  setWatcherMaxItems: Dispatch<SetStateAction<number>>;
  runnerControl: RunnerControlRecord | null;
  saveRunnerControl: (next: RunnerControlRecord) => void;
  sendPolicyAutopilotId: string;
  setSendPolicyAutopilotId: Dispatch<SetStateAction<string>>;
  loadSendPolicy: () => void;
  sendPolicy: AutopilotSendPolicyRecord | null;
  sendPolicyAllowlistInput: string;
  setSendPolicyAllowlistInput: Dispatch<SetStateAction<string>>;
  saveSendPolicy: (next: AutopilotSendPolicyRecord) => void;
  connectionsMessage: string | null;
  guideScopeType: GuideScopeType;
  setGuideScopeType: Dispatch<SetStateAction<GuideScopeType>>;
  guideScopeId: string;
  setGuideScopeId: Dispatch<SetStateAction<string>>;
  guideInstruction: string;
  setGuideInstruction: Dispatch<SetStateAction<string>>;
  submitGuide: () => void;
  guideMessage: string | null;
  connections: EmailConnectionRecord[];
  startOauth: (provider: "gmail" | "microsoft365") => void;
  runWatcherTick: (provider: "gmail" | "microsoft365") => void;
  disconnectProvider: (provider: "gmail" | "microsoft365") => void;
  oauthSession: OAuthStartResponse | null;
  oauthCode: string;
  setOauthCode: Dispatch<SetStateAction<string>>;
  completeOauth: () => void;
  setOauthSession: Dispatch<SetStateAction<OAuthStartResponse | null>>;
};

export function ConnectionPanel(props: Props) {
  const {
    oauthProvider,
    setOauthProvider,
    oauthClientId,
    setOauthClientId,
    oauthRedirectUri,
    setOauthRedirectUri,
    saveOauthSetup,
    transportStatus,
    relaySubscriberTokenInput,
    setRelaySubscriberTokenInput,
    saveRelaySubscriberToken,
    removeRelaySubscriberToken,
    watcherAutopilotId,
    setWatcherAutopilotId,
    watcherMaxItems,
    setWatcherMaxItems,
    runnerControl,
    saveRunnerControl,
    sendPolicyAutopilotId,
    setSendPolicyAutopilotId,
    loadSendPolicy,
    sendPolicy,
    sendPolicyAllowlistInput,
    setSendPolicyAllowlistInput,
    saveSendPolicy,
    connectionsMessage,
    guideScopeType,
    setGuideScopeType,
    guideScopeId,
    setGuideScopeId,
    guideInstruction,
    setGuideInstruction,
    submitGuide,
    guideMessage,
    connections,
    startOauth,
    runWatcherTick,
    disconnectProvider,
    oauthSession,
    oauthCode,
    setOauthCode,
    completeOauth,
    setOauthSession,
  } = props;

  return (
    <section className="connection-panel" aria-label="Email connections">
      <div className="connection-panel-header">
        <h2>Email Connections</h2>
        <p>Connect Gmail or Microsoft 365 once so inbox automations can run while your Mac is awake.</p>
      </div>
      <div className="watcher-controls">
        <label>
          <span>Execution mode</span>
          <input
            value={
              transportStatus?.mode === "hosted_relay"
                ? "Hosted (Relay)"
                : transportStatus?.mode === "byok_local"
                  ? "BYOK (Local)"
                  : "Mock (Dev/Test)"
            }
            readOnly
          />
        </label>
        <label>
          <span>Hosted plan token</span>
          <input
            type="password"
            value={relaySubscriberTokenInput}
            onChange={(event) => setRelaySubscriberTokenInput(event.target.value)}
            placeholder={transportStatus?.relayConfigured ? "Token saved in Keychain" : "Paste subscriber token"}
          />
        </label>
        <label>
          <span>&nbsp;</span>
          <div className="transport-token-actions">
            <button type="button" onClick={saveRelaySubscriberToken}>Save Token</button>
            <button type="button" onClick={removeRelaySubscriberToken}>Remove</button>
          </div>
        </label>
      </div>
      {transportStatus && (
        <p className="transport-status-note">
          Relay endpoint: {transportStatus.relayUrl} {transportStatus.relayConfigured ? "• token saved" : "• no token saved"}
        </p>
      )}
      <div className="connection-setup-grid">
        <label>
          Provider
          <select
            value={oauthProvider}
            onChange={(event) => setOauthProvider(event.target.value as "gmail" | "microsoft365")}
          >
            <option value="gmail">Gmail</option>
            <option value="microsoft365">Microsoft 365</option>
          </select>
        </label>
        <label>
          OAuth Client ID
          <input
            value={oauthClientId}
            onChange={(event) => setOauthClientId(event.target.value)}
            placeholder="Paste OAuth client id"
          />
        </label>
        <label>
          Redirect URI
          <input
            value={oauthRedirectUri}
            onChange={(event) => setOauthRedirectUri(event.target.value)}
            placeholder="https://your-app/callback"
          />
        </label>
        <button type="button" className="intent-primary" onClick={saveOauthSetup}>
          Save Setup
        </button>
      </div>
      <div className="watcher-controls">
        <label>
          Inbox Autopilot ID
          <input
            value={watcherAutopilotId}
            onChange={(event) => setWatcherAutopilotId(event.target.value)}
            placeholder="auto_inbox_watch"
          />
        </label>
        <label>
          Max emails per tick
          <input
            type="number"
            min={1}
            max={25}
            value={watcherMaxItems}
            onChange={(event) => setWatcherMaxItems(Number(event.target.value) || 10)}
          />
        </label>
      </div>
      {runnerControl && (
        <div className="watcher-controls">
          <label>
            <span>Background runner</span>
            <select
              value={runnerControl.backgroundEnabled ? "on" : "off"}
              onChange={(event) =>
                saveRunnerControl({
                  ...runnerControl,
                  backgroundEnabled: event.target.value === "on",
                })
              }
            >
              <option value="off">Off</option>
              <option value="on">On</option>
            </select>
          </label>
          <label>
            <span>Inbox watcher</span>
            <select
              value={runnerControl.watcherEnabled ? "on" : "off"}
              onChange={(event) =>
                saveRunnerControl({
                  ...runnerControl,
                  watcherEnabled: event.target.value === "on",
                })
              }
            >
              <option value="on">Active</option>
              <option value="off">Paused</option>
            </select>
          </label>
          <label>
            <span>Watcher interval (seconds)</span>
            <input
              type="number"
              min={15}
              max={900}
              value={runnerControl.watcherPollSeconds}
              onChange={(event) =>
                saveRunnerControl({
                  ...runnerControl,
                  watcherPollSeconds: Number(event.target.value) || 60,
                })
              }
            />
          </label>
          <label>
            <span>Watcher max emails</span>
            <input
              type="number"
              min={1}
              max={25}
              value={runnerControl.watcherMaxItems}
              onChange={(event) =>
                saveRunnerControl({
                  ...runnerControl,
                  watcherMaxItems: Number(event.target.value) || 10,
                })
              }
            />
          </label>
        </div>
      )}
      <div className="watcher-controls">
        <label>
          <span>Send policy Autopilot ID</span>
          <input
            value={sendPolicyAutopilotId}
            onChange={(event) => setSendPolicyAutopilotId(event.target.value)}
            placeholder="auto_inbox_watch_gmail"
          />
        </label>
        <label>
          <span>&nbsp;</span>
          <button type="button" onClick={loadSendPolicy}>
            Load Send Policy
          </button>
        </label>
      </div>
      {sendPolicy && (
        <div className="watcher-controls">
          <label>
            <span>Sending</span>
            <select
              value={sendPolicy.allowSending ? "on" : "off"}
              onChange={(event) =>
                saveSendPolicy({ ...sendPolicy, allowSending: event.target.value === "on" })
              }
            >
              <option value="off">Compose only</option>
              <option value="on">Allow sending</option>
            </select>
          </label>
          <label>
            <span>Recipient allowlist (comma separated)</span>
            <input
              value={sendPolicyAllowlistInput}
              onChange={(event) => setSendPolicyAllowlistInput(event.target.value)}
              onBlur={() =>
                saveSendPolicy({
                  ...sendPolicy,
                  recipientAllowlist: sendPolicyAllowlistInput
                    .split(",")
                    .map((x) => x.trim())
                    .filter((x) => x.length > 0),
                })
              }
              placeholder="person@example.com, @company.com"
            />
          </label>
          <label>
            <span>Max sends per day</span>
            <input
              type="number"
              min={1}
              max={200}
              value={sendPolicy.maxSendsPerDay}
              onChange={(event) =>
                saveSendPolicy({
                  ...sendPolicy,
                  maxSendsPerDay: Number(event.target.value) || sendPolicy.maxSendsPerDay,
                })
              }
            />
          </label>
          <label>
            <span>Allow outside quiet hours</span>
            <select
              value={sendPolicy.allowOutsideQuietHours ? "yes" : "no"}
              onChange={(event) =>
                saveSendPolicy({
                  ...sendPolicy,
                  allowOutsideQuietHours: event.target.value === "yes",
                })
              }
            >
              <option value="no">No</option>
              <option value="yes">Yes</option>
            </select>
          </label>
        </div>
      )}
      {connectionsMessage && <p className="connection-message">{connectionsMessage}</p>}
      <div className="watcher-controls">
        <label>
          <span>Guide scope</span>
          <select
            value={guideScopeType}
            onChange={(event) =>
              setGuideScopeType(event.target.value as "autopilot" | "run" | "approval" | "outcome")
            }
          >
            <option value="autopilot">Autopilot</option>
            <option value="run">Run</option>
            <option value="approval">Approval</option>
            <option value="outcome">Outcome</option>
          </select>
        </label>
        <label>
          <span>Scope ID</span>
          <input
            value={guideScopeId}
            onChange={(event) => setGuideScopeId(event.target.value)}
            placeholder="autopilot_123 / run_123 / ..."
          />
        </label>
        <label>
          <span>Guide instruction</span>
          <input
            value={guideInstruction}
            onChange={(event) => setGuideInstruction(event.target.value)}
            placeholder="One thing to change for this item"
          />
        </label>
        <label>
          <span>&nbsp;</span>
          <button type="button" onClick={submitGuide}>
            Apply Guide
          </button>
        </label>
      </div>
      {guideMessage && <p className="connection-message">{guideMessage}</p>}

      <div className="connection-cards">
        {connections.map((record) => (
          <article key={record.provider} className="connection-card">
            <h3>{record.provider === "gmail" ? "Gmail" : "Microsoft 365"}</h3>
            <p>Status: {record.status === "connected" ? "Connected" : "Disconnected"}</p>
            {record.accountEmail && <p>Account: {record.accountEmail}</p>}
            <ConnectionHealthSummary record={record} />
            <div className="connection-actions">
              <button type="button" onClick={() => startOauth(record.provider)}>
                {record.status === "connected" ? "Reconnect" : "Connect"}
              </button>
              <button
                type="button"
                onClick={() => runWatcherTick(record.provider)}
                disabled={record.status !== "connected"}
              >
                Poll Inbox Now
              </button>
              {record.status === "connected" && (
                <button type="button" onClick={() => disconnectProvider(record.provider)}>
                  Disconnect
                </button>
              )}
            </div>
          </article>
        ))}
      </div>

      {oauthSession && (
        <div className="oauth-flow">
          <p>
            Open this link to authorize {oauthSession.provider === "gmail" ? "Gmail" : "Microsoft 365"}:
          </p>
          <a href={oauthSession.authUrl} target="_blank" rel="noreferrer">
            {oauthSession.authUrl}
          </a>
          <label>
            Authorization code
            <input
              value={oauthCode}
              onChange={(event) => setOauthCode(event.target.value)}
              placeholder="Paste code from callback"
            />
          </label>
          <div className="connection-actions">
            <button type="button" className="intent-primary" onClick={completeOauth}>
              Complete Connection
            </button>
            <button type="button" onClick={() => setOauthSession(null)}>
              Cancel
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
