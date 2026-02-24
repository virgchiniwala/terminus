use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GuidanceMode {
    Applied,
    ProposedRule,
    NeedsApproval,
}

pub(crate) fn normalize_guidance_instruction(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Add one short instruction to guide this item.".to_string());
    }
    if trimmed.chars().count() > 280 {
        return Err("Keep guidance under 280 characters for now.".to_string());
    }
    Ok(trimmed.to_string())
}

pub(crate) fn classify_guidance(instruction: &str) -> (GuidanceMode, String, Option<String>) {
    let lowered = instruction.to_ascii_lowercase();
    let risky = [
        "enable sending",
        "disable approval",
        "allow all",
        "add recipient",
        "remove quiet hours",
        "run shell",
        "execute code",
    ]
    .iter()
    .any(|term| lowered.contains(term));
    if risky {
        return (
            GuidanceMode::NeedsApproval,
            "That change affects protected capabilities. Terminus saved your request and will require explicit approval.".to_string(),
            None,
        );
    }
    if lowered.contains("always") || lowered.contains("from now on") || lowered.starts_with("when ")
    {
        return (
            GuidanceMode::ProposedRule,
            "Saved as a proposed rule for your review.".to_string(),
            Some(instruction.to_string()),
        );
    }
    (
        GuidanceMode::Applied,
        "Applied to this scoped item only.".to_string(),
        None,
    )
}

pub(crate) fn sanitize_log_message(input: &str) -> String {
    let out = input
        .replace("Authorization", "[REDACTED_HEADER]")
        .replace("Bearer ", "[REDACTED_BEARER] ")
        .replace("api_key", "[REDACTED_FIELD]");
    redact_prefixed_secret_like(&out)
}

pub(crate) fn compute_missed_cycles(
    last_tick_ms: Option<i64>,
    now_ms_value: i64,
    poll_ms: i64,
) -> i64 {
    if poll_ms <= 0 {
        return 0;
    }
    let Some(last_tick) = last_tick_ms else {
        return 0;
    };
    if now_ms_value <= last_tick {
        return 0;
    }
    let elapsed = now_ms_value - last_tick;
    if elapsed <= poll_ms {
        return 0;
    }
    (elapsed / poll_ms) - 1
}

fn redact_prefixed_secret_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if i + 3 <= chars.len()
            && chars[i] == 's'
            && chars[i + 1] == 'k'
            && chars[i + 2] == '-'
            && (i == 0 || !chars[i - 1].is_ascii_alphanumeric())
        {
            let mut j = i + 3;
            let mut token_len = 0usize;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                token_len += 1;
                j += 1;
            }
            if token_len >= 12 {
                out.push_str("[REDACTED_KEY]");
                i = j;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        classify_guidance, compute_missed_cycles, normalize_guidance_instruction, GuidanceMode,
    };

    #[test]
    fn compute_missed_cycles_returns_zero_when_within_interval() {
        assert_eq!(compute_missed_cycles(Some(1_000), 1_900, 1_000), 0);
        assert_eq!(compute_missed_cycles(Some(1_000), 2_000, 1_000), 0);
    }

    #[test]
    fn compute_missed_cycles_returns_expected_overage() {
        assert_eq!(compute_missed_cycles(Some(1_000), 3_500, 1_000), 1);
        assert_eq!(compute_missed_cycles(Some(1_000), 6_100, 1_000), 4);
    }

    #[test]
    fn guidance_classification_blocks_capability_escalation() {
        let (mode, _, _) = classify_guidance("Enable sending for all recipients.");
        assert!(matches!(mode, GuidanceMode::NeedsApproval));
    }

    #[test]
    fn guidance_classification_proposes_rule_for_recurring_phrases() {
        let (mode, _, rule) = classify_guidance("From now on, always keep replies short.");
        assert!(matches!(mode, GuidanceMode::ProposedRule));
        assert!(rule.is_some());
    }

    #[test]
    fn guidance_instruction_is_bounded() {
        let long = "x".repeat(281);
        assert!(normalize_guidance_instruction(&long).is_err());
        assert!(normalize_guidance_instruction("  keep this concise ").is_ok());
    }
}
