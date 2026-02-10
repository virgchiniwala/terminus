# LLM Provider Registry Design

**Goal:** Multi-provider LLM support with extensible architecture. Users can add providers without code changes.

---

## Design Principles

1. **Provider-agnostic interface** — All providers implement common adapter
2. **Drop-in configuration** — New providers via config file + UI, no rebuild
3. **Safe defaults** — Cost ceilings, model selection, error handling
4. **User-friendly setup** — API key entry, validation, clear error messages
5. **Cost transparency** — Show pricing per provider/model before every run

---

## Architecture

### Common Adapter Interface

Every provider implements this Rust trait:

```rust
pub trait LLMProvider {
    // Identity
    fn provider_id(&self) -> &str;  // e.g., "openai", "anthropic"
    fn display_name(&self) -> &str; // e.g., "OpenAI", "Anthropic"
    
    // Configuration
    fn validate_credentials(&self, config: &ProviderConfig) -> Result<(), ProviderError>;
    fn list_models(&self) -> Vec<ModelInfo>;
    fn default_model(&self) -> &str;
    
    // Execution
    async fn generate(
        &self,
        request: &GenerationRequest,
        callbacks: &dyn GenerationCallbacks,
    ) -> Result<GenerationResponse, ProviderError>;
    
    // Cost estimation
    fn estimate_cost(&self, model: &str, tokens: TokenCount) -> f64;
    fn get_pricing(&self, model: &str) -> ModelPricing;
}
```

### Data Structures

```rust
pub struct ProviderConfig {
    pub provider_id: String,
    pub credentials: HashMap<String, String>, // e.g., {"api_key": "..."}
    pub endpoint: Option<String>,             // Custom endpoint (optional)
    pub models: Vec<String>,                  // Available models
    pub enabled: bool,
}

pub struct ModelInfo {
    pub id: String,              // e.g., "gpt-4o", "claude-sonnet-4-5"
    pub display_name: String,    // e.g., "GPT-4o", "Claude 3.5 Sonnet"
    pub context_window: usize,   // tokens
    pub max_output: usize,       // tokens
    pub supports_streaming: bool,
    pub cost_per_input_token: f64,
    pub cost_per_output_token: f64,
}

pub struct GenerationRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f64>,
    pub stream: bool,
}

pub struct GenerationResponse {
    pub content: String,
    pub usage: TokenUsage,
    pub model: String,
    pub finish_reason: FinishReason,
}
```

---

## Provider Registry

### Storage (SQLite)

```sql
CREATE TABLE providers (
    id TEXT PRIMARY KEY,              -- "openai", "anthropic", etc.
    display_name TEXT NOT NULL,
    enabled BOOLEAN DEFAULT TRUE,
    credentials_json TEXT,            -- Encrypted API keys
    endpoint TEXT,
    last_validated_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE models (
    id TEXT PRIMARY KEY,              -- "openai/gpt-4o"
    provider_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    context_window INTEGER NOT NULL,
    max_output INTEGER NOT NULL,
    cost_input REAL NOT NULL,
    cost_output REAL NOT NULL,
    supports_streaming BOOLEAN DEFAULT TRUE,
    FOREIGN KEY (provider_id) REFERENCES providers(id)
);
```

### Registry Manager

```rust
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn LLMProvider>>,
    db: Database,
}

impl ProviderRegistry {
    // Lifecycle
    pub fn new(db: Database) -> Self;
    pub async fn load_providers(&mut self) -> Result<(), RegistryError>;
    
    // Provider management
    pub fn register_provider(&mut self, provider: Box<dyn LLMProvider>) -> Result<(), RegistryError>;
    pub fn get_provider(&self, id: &str) -> Option<&dyn LLMProvider>;
    pub fn list_providers(&self) -> Vec<ProviderInfo>;
    pub fn list_enabled_providers(&self) -> Vec<ProviderInfo>;
    
    // Configuration
    pub async fn save_provider_config(&self, config: &ProviderConfig) -> Result<(), RegistryError>;
    pub async fn validate_provider(&self, id: &str) -> Result<(), ProviderError>;
    pub async fn enable_provider(&self, id: &str) -> Result<(), RegistryError>;
    pub async fn disable_provider(&self, id: &str) -> Result<(), RegistryError>;
    
    // Model selection
    pub fn list_models(&self, provider_id: Option<&str>) -> Vec<ModelInfo>;
    pub fn get_model_info(&self, model_id: &str) -> Option<ModelInfo>;
    pub fn recommend_model(&self, budget: f64, task_type: TaskType) -> Option<String>;
}
```

---

## Built-In Providers (MVP)

### 1. OpenAI Provider

**Models:**
- `gpt-4o` (default) — Latest, balanced
- `gpt-4o-mini` — Cheaper, faster
- `o1` — Reasoning-focused
- `o3-mini` — Cost-efficient reasoning

**Credentials:**
- `api_key` (required)
- `organization_id` (optional)

**Endpoint:**
- Default: `https://api.openai.com/v1`
- Custom: User can override for proxies

**Cost Estimation:**
- Input: $2.50 / 1M tokens (gpt-4o)
- Output: $10.00 / 1M tokens (gpt-4o)
- Updated from pricing API when possible

### 2. Anthropic Provider

**Models:**
- `claude-sonnet-4-5` (default) — Latest Sonnet
- `claude-opus-4-5` — Highest capability
- `claude-haiku-4` — Fast, cost-efficient

**Credentials:**
- `api_key` (required)

**Endpoint:**
- Default: `https://api.anthropic.com/v1`

**Cost Estimation:**
- Sonnet 4.5 input: $3.00 / 1M tokens
- Sonnet 4.5 output: $15.00 / 1M tokens
- Pricing updated from docs

---

## Extensibility (Phase 2+)

### Adding a New Provider

**Step 1:** Create provider implementation

```rust
// src-tauri/src/providers/gemini.rs
pub struct GeminiProvider {
    config: ProviderConfig,
    client: reqwest::Client,
}

impl LLMProvider for GeminiProvider {
    fn provider_id(&self) -> &str { "gemini" }
    fn display_name(&self) -> &str { "Google Gemini" }
    
    fn validate_credentials(&self, config: &ProviderConfig) -> Result<(), ProviderError> {
        // Check API key format, test request
    }
    
    fn list_models(&self) -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "gemini-2.5-flash".to_string(),
                display_name: "Gemini 2.5 Flash".to_string(),
                context_window: 1_000_000,
                max_output: 8192,
                supports_streaming: true,
                cost_per_input_token: 0.000_000_075,
                cost_per_output_token: 0.000_000_300,
            },
        ]
    }
    
    async fn generate(
        &self,
        request: &GenerationRequest,
        callbacks: &dyn GenerationCallbacks,
    ) -> Result<GenerationResponse, ProviderError> {
        // Call Gemini API, stream results via callbacks
    }
}
```

**Step 2:** Register provider in main

```rust
// src-tauri/src/main.rs
let mut registry = ProviderRegistry::new(db);
registry.register_provider(Box::new(OpenAIProvider::new()));
registry.register_provider(Box::new(AnthropicProvider::new()));
registry.register_provider(Box::new(GeminiProvider::new())); // New!
```

**Step 3:** Add UI config (React)

```tsx
// src/providers/ProviderSetup.tsx
<ProviderCard
  id="gemini"
  name="Google Gemini"
  icon={<GeminiIcon />}
  setup={<GeminiSetupForm />}
/>
```

**That's it!** No rebuild required if provider registry loaded dynamically.

---

## User Flows

### First-Time Setup

1. **Onboarding Screen:** "Connect your LLM provider"
2. **Provider List:** Show OpenAI + Anthropic cards
3. **Select Provider:** User taps OpenAI
4. **Setup Screen:**
   - "Enter your OpenAI API key"
   - Input field (masked)
   - "Where to find your key" link → docs
   - "Test Connection" button
5. **Validation:**
   - Call `/v1/models` to verify key
   - Show success ✓ or error message
6. **Model Selection:**
   - Show available models
   - Highlight default (gpt-4o)
   - Show pricing (input/output per 1M tokens)
   - "Use this model" button
7. **Done!** Proceed to starter automations

### Adding a Second Provider

1. **Settings → Providers**
2. **"+ Add Provider"**
3. **Provider List:** Show all supported providers
4. **Select Anthropic**
5. **Setup Flow** (same as above)
6. **Model Preference:**
   - "Which model for new automations?"
   - Default: User's first provider
   - Can override per-automation

### Switching Providers

1. **Automation Detail View**
2. **Model Settings Section:**
   - "Current: OpenAI gpt-4o"
   - "Change Model" button
3. **Model Picker:**
   - Group by provider
   - Show cost estimate diff
   - "This will cost ~$0.05 more per run"
4. **Confirm:** "Switch to Claude Sonnet 4.5?"
5. **Done!** Next run uses new model

---

## Cost Management

### Per-Provider Budgets

```rust
pub struct ProviderBudget {
    pub provider_id: String,
    pub daily_limit: f64,          // USD
    pub monthly_limit: f64,        // USD
    pub per_run_soft_cap: usize,   // tokens
    pub per_run_hard_cap: usize,   // tokens
    pub alert_threshold: f64,      // 0.8 = alert at 80%
}
```

**UI:**
- Settings → Providers → [Provider] → Budget
- Sliders for daily/monthly limits
- Toggle per-run caps
- "Alert me when I hit 80% of budget"

### Cost Tracking

```sql
CREATE TABLE provider_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    run_id TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cost REAL NOT NULL,
    timestamp INTEGER NOT NULL,
    FOREIGN KEY (provider_id) REFERENCES providers(id)
);
```

**Analytics:**
- Cost by provider (this month)
- Cost by model
- Projected monthly spend
- "You're on track for $X this month"

---

## Error Handling

### Provider Errors

```rust
pub enum ProviderError {
    InvalidCredentials { message: String },
    RateLimitExceeded { retry_after: Option<Duration> },
    ModelNotAvailable { model: String },
    QuotaExceeded { limit_type: String },
    NetworkError { source: reqwest::Error },
    Timeout { duration: Duration },
    UnexpectedResponse { status: u16, body: String },
}
```

**User-Facing Messages:**

| Error | Message | Action |
|-------|---------|--------|
| InvalidCredentials | "Your OpenAI API key is invalid. Please check and try again." | → Re-enter key |
| RateLimitExceeded | "OpenAI rate limit hit. Retrying in 30 seconds..." | Auto-retry |
| ModelNotAvailable | "GPT-4o is unavailable right now. Try GPT-4o-mini instead?" | → Model picker |
| QuotaExceeded | "You've hit your OpenAI usage cap. Upgrade your plan or use Anthropic." | → Provider settings |
| NetworkError | "Can't reach OpenAI. Check your internet connection." | Retry button |

### Fallback Strategy

```rust
pub struct FallbackConfig {
    pub enabled: bool,
    pub fallback_order: Vec<(String, String)>, // (provider_id, model)
    pub retry_count: usize,
}
```

**Example:**
1. Try OpenAI gpt-4o
2. If rate limited → wait + retry (3x)
3. If still failing → try Anthropic claude-sonnet-4-5
4. If still failing → alert user

**User Control:**
- Settings → Providers → Fallback
- Toggle "Auto-fallback to other providers"
- Drag to reorder fallback priority
- "Always ask before switching providers" checkbox

---

## Security

### Credential Storage

- **Encrypt API keys** with macOS Keychain (or platform equivalent)
- Never log API keys
- Never send keys to analytics/telemetry
- Mask keys in UI (show last 4 chars: `sk-...vX2Q`)

### Audit Log

```sql
CREATE TABLE provider_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider_id TEXT NOT NULL,
    action TEXT NOT NULL,         -- "credentials_added", "provider_enabled", "model_changed"
    metadata TEXT,                 -- JSON with details
    timestamp INTEGER NOT NULL
);
```

**Events:**
- Credentials added/updated/removed
- Provider enabled/disabled
- Model changed
- Budget limits changed
- Fallback triggered

---

## Testing

### Provider Tests

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_openai_provider_validation() {
        let provider = OpenAIProvider::new();
        let config = ProviderConfig {
            provider_id: "openai".to_string(),
            credentials: hashmap! {
                "api_key" => "sk-test123".to_string(),
            },
            ..Default::default()
        };
        
        let result = provider.validate_credentials(&config).await;
        assert!(result.is_err()); // Invalid key should fail
    }
    
    #[tokio::test]
    async fn test_cost_estimation() {
        let provider = OpenAIProvider::new();
        let cost = provider.estimate_cost("gpt-4o", TokenCount {
            input: 1000,
            output: 500,
        });
        
        // gpt-4o: $2.50/1M input, $10.00/1M output
        // Expected: (1000 * 0.0000025) + (500 * 0.00001) = 0.0025 + 0.005 = 0.0075
        assert_eq!(cost, 0.0075);
    }
}
```

### Integration Tests

- Test credential validation (mock API)
- Test model listing
- Test generation request/response
- Test streaming
- Test error handling (rate limits, network errors)
- Test fallback logic
- Test budget enforcement

---

## Implementation Checklist

### Phase 1: Core (MVP)
- [ ] Define `LLMProvider` trait
- [ ] Create `ProviderRegistry`
- [ ] Implement OpenAI provider
- [ ] Implement Anthropic provider
- [ ] Build credential storage (Keychain)
- [ ] Build provider setup UI
- [ ] Build model picker UI
- [ ] Implement cost estimation
- [ ] Add error handling
- [ ] Write unit tests

### Phase 2: Polish
- [ ] Add fallback strategy
- [ ] Build budget management UI
- [ ] Add cost tracking/analytics
- [ ] Implement rate limit handling
- [ ] Add provider audit log
- [ ] Write integration tests

### Phase 3: Extensibility
- [ ] Add Gemini provider
- [ ] Add local model support (Ollama/LM Studio)
- [ ] Add Azure OpenAI provider
- [ ] Build dynamic provider loading (no rebuild)
- [ ] Add provider marketplace/discovery

---

## Success Metrics

**Usability:**
- [ ] Users can add a provider in <2 minutes
- [ ] 100% of users understand cost before first run
- [ ] 0% of users report surprise bills
- [ ] 95%+ provider validation success rate

**Reliability:**
- [ ] <1% provider error rate (excluding user key issues)
- [ ] <5s average provider validation time
- [ ] Fallback works 100% of the time when configured

**Extensibility:**
- [ ] Adding a new provider takes <1 hour (code + tests)
- [ ] Provider implementations are <500 lines each
- [ ] No core changes needed to add providers

---

**Last updated:** 2026-02-10  
**Owner:** Jarvis
