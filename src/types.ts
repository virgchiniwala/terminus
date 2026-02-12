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
  };
}

export type RiskTier = "low" | "medium" | "high";

export type PrimitiveId =
  | "read.web"
  | "read.forwarded_email"
  | "read.vault_file"
  | "write.outcome_draft"
  | "write.email_draft"
  | "send.email"
  | "schedule.run"
  | "notify.user";

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
  recipe: "website_monitor" | "inbox_triage" | "daily_brief";
  intent: string;
  provider: ProviderMetadata;
  allowedPrimitives: PrimitiveId[];
  steps: PlanStep[];
}
