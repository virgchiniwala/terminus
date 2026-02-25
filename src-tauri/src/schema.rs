use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeKind {
    WebsiteMonitor,
    InboxTriage,
    DailyBrief,
    Custom,
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
    ReadSources,
    ReadForwardedEmail,
    CallApi,
    TriageEmail,
    AggregateDailySummary,
    ReadVaultFile,
    WriteOutcomeDraft,
    WriteEmailDraft,
    SendEmail,
    ScheduleRun,
    NotifyUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiCallRequest {
    pub url: String,
    pub method: String,
    pub header_key_ref: String,
    pub auth_header_name: String,
    pub auth_scheme: String,
    pub body_json: Option<String>,
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
    pub inbox_source_text: Option<String>,
    pub daily_sources: Vec<String>,
    pub api_call_request: Option<ApiCallRequest>,
    pub recipient_hints: Vec<String>,
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
        let inbox_source_text = if recipe == RecipeKind::InboxTriage {
            Some(intent.clone())
        } else {
            None
        };
        let daily_sources = if recipe == RecipeKind::DailyBrief {
            extract_urls(&intent)
                .into_iter()
                .take(5)
                .collect::<Vec<String>>()
        } else {
            Vec::new()
        };
        let mut web_allowed_domains = web_allowed_domains;
        if recipe == RecipeKind::DailyBrief {
            for source in &daily_sources {
                if let Some(host) = extract_host(source) {
                    if !web_allowed_domains
                        .iter()
                        .any(|h| h.eq_ignore_ascii_case(&host))
                    {
                        web_allowed_domains.push(host);
                    }
                }
            }
        }
        let wants_send = intent_mentions_send(&intent);
        let mut allowed_primitives = vec![
            PrimitiveId::ReadWeb,
            PrimitiveId::ReadSources,
            PrimitiveId::ReadForwardedEmail,
            PrimitiveId::TriageEmail,
            PrimitiveId::AggregateDailySummary,
            PrimitiveId::WriteOutcomeDraft,
            PrimitiveId::WriteEmailDraft,
            PrimitiveId::NotifyUser,
        ];
        if wants_send {
            allowed_primitives.push(PrimitiveId::SendEmail);
        }
        let recipient_hints = extract_emails(&intent);

        let steps = match recipe {
            RecipeKind::WebsiteMonitor => {
                let mut steps = vec![
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
                ];
                if wants_send {
                    steps.push(PlanStep {
                        id: "step_4".to_string(),
                        label: "Send approved email through connected account".to_string(),
                        primitive: PrimitiveId::SendEmail,
                        requires_approval: true,
                        risk_tier: RiskTier::High,
                    });
                }
                steps
            }
            RecipeKind::InboxTriage => {
                let mut steps = vec![
                    PlanStep {
                        id: "step_1".to_string(),
                        label: "Read forwarded email or pasted message".to_string(),
                        primitive: PrimitiveId::ReadForwardedEmail,
                        requires_approval: false,
                        risk_tier: RiskTier::Low,
                    },
                    PlanStep {
                        id: "step_2".to_string(),
                        label: "Apply inbox triage action".to_string(),
                        primitive: PrimitiveId::TriageEmail,
                        requires_approval: true,
                        risk_tier: RiskTier::Medium,
                    },
                    PlanStep {
                        id: "step_3".to_string(),
                        label: "Draft reply options and triage labels".to_string(),
                        primitive: PrimitiveId::WriteOutcomeDraft,
                        requires_approval: false,
                        risk_tier: RiskTier::Medium,
                    },
                    PlanStep {
                        id: "step_4".to_string(),
                        label: "Queue email draft for explicit approval".to_string(),
                        primitive: PrimitiveId::WriteEmailDraft,
                        requires_approval: true,
                        risk_tier: RiskTier::Medium,
                    },
                ];
                if wants_send {
                    steps.push(PlanStep {
                        id: "step_5".to_string(),
                        label: "Send approved reply through connected account".to_string(),
                        primitive: PrimitiveId::SendEmail,
                        requires_approval: true,
                        risk_tier: RiskTier::High,
                    });
                }
                steps
            }
            RecipeKind::DailyBrief => vec![
                PlanStep {
                    id: "step_1".to_string(),
                    label: "Read configured sources".to_string(),
                    primitive: PrimitiveId::ReadSources,
                    requires_approval: false,
                    risk_tier: RiskTier::Low,
                },
                PlanStep {
                    id: "step_2".to_string(),
                    label: "Aggregate a cohesive daily summary".to_string(),
                    primitive: PrimitiveId::AggregateDailySummary,
                    requires_approval: false,
                    risk_tier: RiskTier::Medium,
                },
                PlanStep {
                    id: "step_3".to_string(),
                    label: "Compose a single daily brief outcome card".to_string(),
                    primitive: PrimitiveId::WriteOutcomeDraft,
                    requires_approval: true,
                    risk_tier: RiskTier::Medium,
                },
            ],
            RecipeKind::Custom => Vec::new(),
        };

        Self {
            schema_version: "1.0".to_string(),
            recipe,
            intent,
            provider,
            web_source_url,
            web_allowed_domains,
            inbox_source_text,
            daily_sources,
            api_call_request: None,
            recipient_hints,
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

fn extract_urls(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter_map(|token| {
            let normalized = token
                .trim_matches(|c: char| ",.;:!?()[]{}<>\"'".contains(c))
                .to_string();
            if normalized.starts_with("http://") || normalized.starts_with("https://") {
                Some(normalized)
            } else {
                None
            }
        })
        .collect()
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

fn intent_mentions_send(input: &str) -> bool {
    let normalized = input.to_ascii_lowercase();
    normalized.contains("send")
        || normalized.contains("ship this")
        || normalized.contains("reply automatically")
}

fn extract_emails(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter_map(|token| {
            let normalized = token
                .trim_matches(|c: char| ",.;:!?()[]{}<>\"'".contains(c))
                .to_ascii_lowercase();
            if normalized.contains('@')
                && normalized
                    .split('@')
                    .nth(1)
                    .map(|domain| domain.contains('.'))
                    .unwrap_or(false)
            {
                Some(normalized)
            } else {
                None
            }
        })
        .collect::<Vec<String>>()
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
        let custom = AutopilotPlan::from_intent(
            RecipeKind::Custom,
            "Parse invoices and prepare weekly categories".to_string(),
            ProviderId::OpenAi,
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
        assert_eq!(custom.steps.len(), 0);
        assert_eq!(website.steps.len(), 3);
        assert_eq!(triage.steps.len(), 4);
        assert_eq!(brief.steps.len(), 3);
    }
}
