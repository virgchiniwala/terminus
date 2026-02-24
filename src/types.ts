export type SurfaceKind = "Autopilots" | "Outcomes" | "Approvals" | "Activity";

export interface HomeSurface {
  title: SurfaceKind;
  subtitle: string;
  count: number;
  cta: string;
}

export interface HomeSnapshot {
  surfaces: HomeSurface[];
  runner: {
    mode: "app_open" | "background";
    statusLine: string;
    backlogCount?: number;
    watcherEnabled?: boolean;
    watcherLastTickMs?: number | null;
    missedRunsCount?: number;
    suppressedAutopilotsCount?: number;
    suppressedAutopilots?: Array<{
      autopilotId: string;
      name: string;
      suppressUntilMs: number;
    }>;
  };
}

export type IntentDraftKind = "one_off_run" | "draft_autopilot";

export interface IntentDraftPreview {
  reads: string[];
  writes: string[];
  approvalsRequired: string[];
  estimatedSpend: string;
  primaryCta: string;
}

export interface IntentDraftResponse {
  kind: IntentDraftKind;
  classificationReason: string;
  plan: AutopilotPlan;
  preview: IntentDraftPreview;
}

export interface EmailConnectionRecord {
  provider: "gmail" | "microsoft365";
  status: "connected" | "disconnected";
  accountEmail: string | null;
  scopes: string[];
  connectedAtMs: number | null;
  updatedAtMs: number;
  lastError: string | null;
  watcherBackoffUntilMs?: number | null;
  watcherConsecutiveFailures?: number;
  watcherLastError?: string | null;
  watcherUpdatedAtMs?: number | null;
}

export interface OAuthStartResponse {
  provider: "gmail" | "microsoft365";
  authUrl: string;
  state: string;
  expiresAtMs: number;
}

export interface RunnerControlRecord {
  backgroundEnabled: boolean;
  watcherEnabled: boolean;
  watcherPollSeconds: number;
  watcherMaxItems: number;
  gmailAutopilotId: string;
  microsoftAutopilotId: string;
  watcherLastTickMs: number | null;
  missedRunsCount: number;
}

export interface AutopilotSendPolicyRecord {
  autopilotId: string;
  allowSending: boolean;
  recipientAllowlist: string[];
  maxSendsPerDay: number;
  quietHoursStartLocal: number;
  quietHoursEndLocal: number;
  allowOutsideQuietHours: boolean;
  updatedAtMs: number;
}

export interface ClarificationRecord {
  id: string;
  runId: string;
  stepId: string;
  fieldKey: string;
  question: string;
  optionsJson?: string | null;
  answerJson?: string | null;
  status: "pending" | "answered" | "canceled" | string;
}

export type RunHealthStatus =
  | "healthy_running"
  | "waiting_for_approval"
  | "waiting_for_clarification"
  | "retrying_transient"
  | "retrying_stuck"
  | "policy_blocked"
  | "provider_misconfigured"
  | "source_unreachable"
  | "resource_throttled"
  | "completed"
  | "failed_unclassified";

export interface InterventionSuggestion {
  kind:
    | "approve_pending_action"
    | "answer_clarification"
    | "retry_now_if_due"
    | "pause_autopilot_15m"
    | "reduce_source_scope"
    | "switch_provider_supported_default"
    | "open_receipt"
    | "open_activity_log"
    | string;
  label: string;
  reason: string;
  disabled: boolean;
}

export interface RunDiagnosticRecord {
  id: string;
  runId: string;
  autopilotId: string;
  runState: string;
  healthStatus: RunHealthStatus | string;
  reasonCode: string;
  summary: string;
  suggestions: InterventionSuggestion[];
  createdAtMs: number;
}

export interface ApplyInterventionResult {
  ok: boolean;
  runId: string;
  message: string;
  updatedRunState?: string | null;
}

export type RecipeKind = "website_monitor" | "inbox_triage" | "daily_brief";

export type RiskTier = "low" | "medium" | "high";

export type PrimitiveId =
  | "read_web"
  | "read_forwarded_email"
  | "read_sources"
  | "aggregate_daily_summary"
  | "triage_email"
  | "read_vault_file"
  | "write_outcome_draft"
  | "write_email_draft"
  | "send_email"
  | "schedule_run"
  | "notify_user";

export type ProviderTier = "supported" | "experimental";

export interface ProviderMetadata {
  id: "openai" | "anthropic" | "gemini";
  tier: ProviderTier;
  defaultModel: string;
}

export interface PlanStep {
  id: string;
  label: string;
  primitive: PrimitiveId;
  requiresApproval: boolean;
  riskTier: RiskTier;
}

export interface AutopilotPlan {
  schemaVersion: "1.0";
  recipe: RecipeKind;
  intent: string;
  provider: ProviderMetadata;
  webSourceUrl?: string | null;
  webAllowedDomains?: string[];
  inboxSourceText?: string | null;
  dailySources?: string[];
  recipientHints?: string[];
  allowedPrimitives: PrimitiveId[];
  steps: PlanStep[];
}
