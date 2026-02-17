use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeKind {
    WebsiteMonitor,
    InboxTriage,
    DailyBrief,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    OpenAi,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTier {
    Supported,
    Experimental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskTier {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveId {
    ReadWeb,
    ReadForwardedEmail,
    ReadVaultFile,
    WriteOutcomeDraft,
    WriteEmailDraft,
    SendEmail,
    ScheduleRun,
    NotifyUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub id: ProviderId,
    pub tier: ProviderTier,
    pub default_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub label: String,
    pub primitive: PrimitiveId,
    pub requires_approval: bool,
    pub risk_tier: RiskTier,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutopilotPlan {
    pub schema_version: String,
    pub recipe: RecipeKind,
    pub intent: String,
    pub provider: ProviderMetadata,
    pub web_source_url: Option<String>,
    pub web_allowed_domains: Vec<String>,
    pub allowed_primitives: Vec<PrimitiveId>,
    pub steps: Vec<PlanStep>,
}

impl ProviderMetadata {
    pub fn from_provider_id(id: ProviderId) -> Self {
        match id {
            ProviderId::OpenAi => Self {
                id,
                tier: ProviderTier::Supported,
                default_model: "gpt-4o-mini".to_string(),
            },
            ProviderId::Anthropic => Self {
                id,
                tier: ProviderTier::Supported,
                default_model: "claude-3-5-sonnet-latest".to_string(),
            },
            ProviderId::Gemini => Self {
                id,
                tier: ProviderTier::Experimental,
                default_model: "gemini-2.5-flash".to_string(),
            },
        }
    }
}

impl AutopilotPlan {
    pub fn from_intent(recipe: RecipeKind, intent: String, provider_id: ProviderId) -> Self {
        let provider = ProviderMetadata::from_provider_id(provider_id);
        let web_source_url = extract_first_url(&intent);
        let web_allowed_domains = web_source_url
            .as_deref()
            .and_then(extract_host)
            .map(|host| vec![host])
            .unwrap_or_default();
        let allowed_primitives = vec![
            PrimitiveId::ReadWeb,
            PrimitiveId::ReadForwardedEmail,
            PrimitiveId::WriteOutcomeDraft,
            PrimitiveId::WriteEmailDraft,
            PrimitiveId::NotifyUser,
        ];

        let steps = match recipe {
            RecipeKind::WebsiteMonitor => vec![
                PlanStep {
                    id: "step_1".to_string(),
                    label: "Read website content from allowlisted domain".to_string(),
                    primitive: PrimitiveId::ReadWeb,
                    requires_approval: false,
                    risk_tier: RiskTier::Low,
                },
                PlanStep {
                    id: "step_2".to_string(),
                    label: "Create summary outcome draft".to_string(),
                    primitive: PrimitiveId::WriteOutcomeDraft,
                    requires_approval: true,
                    risk_tier: RiskTier::Medium,
                },
                PlanStep {
                    id: "step_3".to_string(),
                    label: "Create email draft for approval queue".to_string(),
                    primitive: PrimitiveId::WriteEmailDraft,
                    requires_approval: true,
                    risk_tier: RiskTier::Medium,
                },
            ],
            RecipeKind::InboxTriage => vec![
                PlanStep {
                    id: "step_1".to_string(),
                    label: "Read forwarded email or pasted message".to_string(),
                    primitive: PrimitiveId::ReadForwardedEmail,
                    requires_approval: false,
                    risk_tier: RiskTier::Low,
                },
                PlanStep {
                    id: "step_2".to_string(),
                    label: "Draft reply options and triage labels".to_string(),
                    primitive: PrimitiveId::WriteOutcomeDraft,
                    requires_approval: true,
                    risk_tier: RiskTier::Medium,
                },
                PlanStep {
                    id: "step_3".to_string(),
                    label: "Queue email draft for explicit approval".to_string(),
                    primitive: PrimitiveId::WriteEmailDraft,
                    requires_approval: true,
                    risk_tier: RiskTier::Medium,
                },
            ],
            RecipeKind::DailyBrief => vec![
                PlanStep {
                    id: "step_1".to_string(),
                    label: "Read configured sources".to_string(),
                    primitive: PrimitiveId::ReadWeb,
                    requires_approval: false,
                    risk_tier: RiskTier::Low,
                },
                PlanStep {
                    id: "step_2".to_string(),
                    label: "Compose a single daily brief outcome card".to_string(),
                    primitive: PrimitiveId::WriteOutcomeDraft,
                    requires_approval: true,
                    risk_tier: RiskTier::Medium,
                },
                PlanStep {
                    id: "step_3".to_string(),
                    label: "Notify user that the brief is ready".to_string(),
                    primitive: PrimitiveId::NotifyUser,
                    requires_approval: false,
                    risk_tier: RiskTier::Low,
                },
            ],
        };

        Self {
            schema_version: "1.0".to_string(),
            recipe,
            intent,
            provider,
            web_source_url,
            web_allowed_domains,
            allowed_primitives,
            steps,
        }
    }
}

fn extract_first_url(input: &str) -> Option<String> {
    input.split_whitespace().find_map(|token| {
        let normalized = token
            .trim_matches(|c: char| ",.;:!?()[]{}<>\"'".contains(c))
            .to_string();
        if normalized.starts_with("http://") || normalized.starts_with("https://") {
            Some(normalized)
        } else {
            None
        }
    })
}

fn extract_host(url: &str) -> Option<String> {
    let (_, rest) = url.split_once("://")?;
    let host_port = rest.split('/').next()?.trim();
    let host = host_port.split('@').next_back()?.split(':').next()?.trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::{AutopilotPlan, ProviderId, ProviderTier, RecipeKind};

    #[test]
    fn builds_shared_plan_schema_for_all_three_recipes() {
        let website = AutopilotPlan::from_intent(
            RecipeKind::WebsiteMonitor,
            "Monitor company blog and draft updates".to_string(),
            ProviderId::OpenAi,
        );
        let triage = AutopilotPlan::from_intent(
            RecipeKind::InboxTriage,
            "Triage forwarded customer email and draft response".to_string(),
            ProviderId::Anthropic,
        );
        let brief = AutopilotPlan::from_intent(
            RecipeKind::DailyBrief,
            "Prepare a concise daily brief from saved sources".to_string(),
            ProviderId::Gemini,
        );

        assert_eq!(website.schema_version, triage.schema_version);
        assert_eq!(triage.schema_version, brief.schema_version);
        assert_eq!(website.allowed_primitives, triage.allowed_primitives);
        assert_eq!(triage.allowed_primitives, brief.allowed_primitives);
        assert!(!website
            .allowed_primitives
            .contains(&super::PrimitiveId::ScheduleRun));
        assert!(!website
            .allowed_primitives
            .contains(&super::PrimitiveId::ReadVaultFile));
        assert_eq!(brief.provider.tier, ProviderTier::Experimental);
        assert_eq!(website.steps.len(), 3);
        assert_eq!(triage.steps.len(), 3);
        assert_eq!(brief.steps.len(), 3);
    }
}
