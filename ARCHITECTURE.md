# Architecture Overview

**Last Updated:** 2026-02-13  
**Version:** MVP (v0.1.0)

---

## System Design

Terminus is a **Personal AI OS** built as a native desktop app (Tauri) with a Rust backend and React frontend. It enables anyone to automate tasks through natural language intent, with strong safety guarantees and minimal token costs.

### Core Principles

1. **Safety First** - All actions require explicit approval; no silent side-effects
2. **Billing-Safe** - Integer cent accounting, enforced spend caps
3. **Transparent** - Always show what will happen before it happens
4. **Calm UX** - Object-first UI (not chat-based), minimal noise
5. **Local-First** - All execution and secrets stay on device

---

## Architecture Layers

```
┌─────────────────────────────────────────┐
│          Frontend (React)               │  
│  - Intent Input                         │
│  - Plan Preview                         │
│  - Approval Gates                       │
│  - Activity Feed                        │
└─────────────────────────────────────────┘
              ↕ Tauri IPC
┌─────────────────────────────────────────┐
│        Backend (Rust)                   │
│  ┌────────────────────────────────┐    │
│  │   Runner (State Machine)        │    │
│  │  - Tick-based execution         │    │
│  │  - Retry with backoff           │    │
│  │  - Approval orchestration       │    │
│  └────────────────────────────────┘    │
│  ┌────────────────────────────────┐    │
│  │   Provider Layer                │    │
│  │  - OpenAI / Anthropic / Gemini  │    │
│  │  - Retryable error detection    │    │
│  │  - Spend tracking               │    │
│  └────────────────────────────────┘    │
│  ┌────────────────────────────────┐    │
│  │   Transport Layer               │    │
│  │  - Mock (tests)                 │    │
│  │  - LocalHTTP (real API)         │    │
│  │  - (Future: Relay)              │    │
│  └────────────────────────────────┘    │
│  ┌────────────────────────────────┐    │
│  │   Persistence (SQLite)          │    │
│  │  - Runs                         │    │
│  │  - Activities                   │    │
│  │  - Approvals                    │    │
│  │  - Outcomes                     │    │
│  │  - Spend Ledger                 │    │
│  └────────────────────────────────┘    │
└─────────────────────────────────────────┘
              ↕
┌─────────────────────────────────────────┐
│       macOS Keychain (Secrets)          │
└─────────────────────────────────────────┘
```

---

## Core Components

### 1. Runner (State Machine)

**File:** `src-tauri/src/runner.rs`

The runner is the heart of Terminus. It orchestrates execution through a **tick-based state machine** with persisted state in SQLite.

#### State Machine

```
Draft → Ready → Running → Succeeded
                  ↓
              Retrying ⟲ (bounded)
                  ↓
              Failed / Blocked

              NeedsApproval → (approve) → Ready
                           → (reject)  → Canceled
```

#### Key Methods

- `start_run()` - Create a new run from an AutopilotPlan
- `run_tick()` - Advance the state machine by one step
- `resume_due_runs()` - Resume retries that are due
- `approve()` / `reject()` - Handle approval decisions
- `get_run()` - Fetch current run state

#### Execution Model

**Tick-based (not recursive):**
- Each `run_tick()` call executes **exactly one step**
- State is persisted **before** returning
- Caller decides when to tick again (enables bounded work)
- No stack overflow, easy to pause/resume

**Idempotency:**
- Every run requires a unique `idempotency_key`
- Duplicate keys return the existing run
- Prevents accidental double-execution

**Retry Logic:**
- Exponential backoff: 100ms * 2^retry_count
- Max retries configurable per run
- Only retryable errors trigger retries (non-retryable → Failed immediately)
- `next_retry_at_ms` stored for deterministic resume

---

### 2. Provider Layer

**File:** `src-tauri/src/providers/`

Abstracts LLM providers (OpenAI, Anthropic, Gemini) behind a common interface.

#### Provider Tiers

- **Supported** - OpenAI, Anthropic (CI-tested, production-ready)
- **Experimental** - Gemini (available, not CI-blocking)
- **Future** - Hosted relay transport (not implemented)

#### Error Classification

Providers classify errors as **retryable** or **non-retryable**:

**Retryable:**
- Rate limits (429)
- Temporary server errors (500, 503)
- Network timeouts

**Non-Retryable:**
- Invalid API key (401)
- Malformed request (400)
- Content policy violation
- Model not found

#### Spend Tracking

Every provider call logs spend to the spend ledger:
- **Estimated** - Before execution (based on plan)
- **Actual** - After execution (real API cost)
- Stored as **integer cents** (no float money math)

---

### 3. Transport Layer

**File:** `src-tauri/src/transport/`

Handles actual API communication.

#### Transports

1. **MockTransport** (default)
   - Deterministic test responses
   - Simulates errors, caps, retries
   - No real API calls

2. **LocalHttpTransport** (env-flagged: `TERMINUS_TRANSPORT=local_http`)
   - Real OpenAI/Anthropic API calls
   - Keys from macOS Keychain
   - Production execution

3. **Relay Transport** (future)
   - Hosted backend for shared API keys
   - Not implemented yet

---

### 4. Persistence (SQLite)

**File:** `src-tauri/src/db.rs`

All runtime state lives in SQLite for durability.

#### Schema

**runs** - Execution state
```sql
CREATE TABLE runs (
  id TEXT PRIMARY KEY,
  autopilot_id TEXT NOT NULL,
  idempotency_key TEXT UNIQUE NOT NULL,
  state TEXT NOT NULL, -- RunState enum
  current_step_index INTEGER,
  retry_count INTEGER,
  max_retries INTEGER,
  next_retry_at_ms INTEGER,
  soft_cap_approved INTEGER,
  usd_cents_estimate INTEGER,
  usd_cents_actual INTEGER,
  failure_reason TEXT,
  plan TEXT NOT NULL, -- JSON AutopilotPlan
  provider_kind TEXT,
  provider_tier TEXT,
  created_at INTEGER,
  updated_at INTEGER
);
```

**activities** - Audit log
```sql
CREATE TABLE activities (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  activity_type TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT,
  user_message TEXT,
  created_at INTEGER,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

**approvals** - Pending decisions
```sql
CREATE TABLE approvals (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  step_id TEXT NOT NULL,
  status TEXT NOT NULL, -- pending/approved/rejected
  preview TEXT,
  created_at INTEGER,
  updated_at INTEGER,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

**outcomes** - Side-effect results
```sql
CREATE TABLE outcomes (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  step_id TEXT,
  kind TEXT NOT NULL, -- outcome_draft/receipt/etc
  content TEXT NOT NULL,
  created_at INTEGER,
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

**spend_ledger** - Billing records
```sql
CREATE TABLE spend_ledger (
  id TEXT PRIMARY KEY,
  run_id TEXT NOT NULL,
  step_id TEXT,
  entry_kind TEXT NOT NULL, -- estimate/actual
  amount_usd_cents INTEGER NOT NULL,
  provider_kind TEXT,
  created_at INTEGER,
  UNIQUE(run_id, step_id, entry_kind), -- Idempotency
  FOREIGN KEY (run_id) REFERENCES runs(id)
);
```

---

### 5. Schema & Plans

**File:** `src-tauri/src/schema.rs`

Defines the **AutopilotPlan** format - the shared representation for all recipes.

#### AutopilotPlan

```rust
pub struct AutopilotPlan {
    pub schema_version: String,
    pub recipe: RecipeKind, // WebsiteMonitor, InboxTriage, DailyBrief
    pub intent: String,
    pub provider: ProviderMetadata,
    pub allowed_primitives: Vec<PrimitiveId>,
    pub steps: Vec<PlanStep>,
}

pub struct PlanStep {
    pub id: String,
    pub label: String,
    pub primitive: PrimitiveId,
    pub requires_approval: bool,
    pub risk_tier: RiskTier,
}
```

#### Primitives

Actions that steps can perform:
- `ReadWebsite` - Fetch URL content
- `WriteOutcomeDraft` - Compose result (no send)
- `SendEmail` - Send email (approval required)

New primitives are **deny-by-default** and require explicit allowlisting.

---

## Safety Guarantees

### 1. Approval Gates

**All write/send actions require approval by default.**

Approval flow:
1. Runner executes step that needs approval
2. Transitions to `NeedsApproval` state
3. Creates `Approval` record with preview
4. Pauses execution
5. User approves/rejects
6. Runner resumes or cancels

### 2. Spend Caps

**Per-Run Caps:**
- Soft cap: $0.40 (requires approval to proceed)
- Hard cap: $0.80 (blocks execution, no approval option)

**Daily Caps (future):**
- Soft cap: $3.00
- Hard cap: $5.00

Caps are enforced **before** side-effects occur.

### 3. Idempotency

- Every run requires a unique `idempotency_key`
- Duplicate starts return existing run
- Spend ledger uses `(run_id, step_id, entry_kind)` uniqueness
- Prevents double-billing on retry

### 4. Atomic Transactions

**All state transitions are atomic:**
- `UPDATE runs SET state = X` + `INSERT INTO activities` → single transaction
- Failures rollback both changes
- No partial state updates

### 5. Terminal Receipts

Every terminal run (Succeeded/Failed/Blocked/Canceled) generates a **receipt**:
- Provider tier used
- Total spend (actual cents)
- Redacted sensitive data (API keys, emails)
- Stored in `outcomes` table

---

## Data Flow Example

**User Action:** "Monitor example.com for changes"

1. **Frontend → Backend (IPC):**
   ```typescript
   invoke('start_recipe_run', {
     autopilotId: 'auto_001',
     recipe: 'website_monitor',
     intent: 'Monitor example.com for changes',
     provider: 'openai',
     idempotencyKey: 'user_run_123',
     maxRetries: 2
   })
   ```

2. **Backend creates plan:**
   ```rust
   AutopilotPlan {
     recipe: WebsiteMonitor,
     steps: [
       { primitive: ReadWebsite, requires_approval: false },
       { primitive: WriteOutcomeDraft, requires_approval: true }
     ]
   }
   ```

3. **Runner persists run:**
   ```sql
   INSERT INTO runs (state='ready', plan=...) ...
   ```

4. **Frontend polls for tick:**
   ```typescript
   invoke('run_tick', { runId: 'run_001' })
   ```

5. **Runner executes step 1 (ReadWebsite):**
   - Calls `MockTransport` or `LocalHttpTransport`
   - Logs spend to ledger
   - Returns `RunRecord { state: Ready, current_step_index: 1 }`

6. **Frontend ticks again, hits approval:**
   ```typescript
   invoke('run_tick', { runId: 'run_001' })
   // Returns: { state: 'needs_approval' }
   ```

7. **Frontend shows approval UI:**
   ```typescript
   const approvals = await invoke('list_pending_approvals')
   // Show preview: "Approve step: Write draft outcome"
   ```

8. **User approves:**
   ```typescript
   invoke('approve_run_approval', { approvalId: 'approval_001' })
   ```

9. **Runner executes step 2, completes:**
   ```rust
   // Execute WriteOutcomeDraft
   // Transition to Succeeded
   // Write terminal receipt
   ```

10. **Frontend shows result:**
    ```typescript
    const run = await invoke('get_run', { runId: 'run_001' })
    // { state: 'succeeded', usd_cents_actual: 8 }
    ```

---

## Error Handling

### Provider Errors

**Retryable (automatic retry with backoff):**
```rust
ProviderError::RateLimitExceeded
ProviderError::TemporaryFailure
```

**Non-Retryable (immediate failure):**
```rust
ProviderError::InvalidApiKey
ProviderError::InvalidRequest
ProviderError::ContentPolicyViolation
```

### Spend Cap Violations

**Soft cap (pausable):**
```rust
// Run pauses, creates approval with step_id = "__soft_cap__"
// User can approve to proceed or reject to cancel
```

**Hard cap (terminal block):**
```rust
// Run transitions to Blocked state
// No approval option, execution stops
// Receipt written with block reason
```

### Retry Exhaustion

```rust
// After max_retries attempts:
// Transition to Failed state
// failure_reason = "Max retries exhausted (3 attempts)"
```

---

## Testing Strategy

See `TESTING.md` for full details.

**Unit Tests:**
- All business logic in `runner.rs` has test coverage
- Provider error classification
- Spend cap boundaries
- Idempotency guarantees
- Atomic transaction rollback

**Integration Tests:**
- End-to-end flows for all 3 recipes
- Approval/rejection paths
- Retry with resume

**Test Transports:**
- `MockTransport` for deterministic test execution
- Special intents trigger test scenarios:
  - `"simulate_provider_retryable_failure"` → retry test
  - `"simulate_cap_hard"` → hard cap test
  - `"simulate_spend_40"` → boundary test

---

## Configuration

### Environment Variables

**`TERMINUS_TRANSPORT`**
- `mock` (default) - Use MockTransport (no real API calls)
- `local_http` - Use LocalHttpTransport (real OpenAI/Anthropic)

### Secrets (macOS Keychain)

**OpenAI:**
```bash
security add-generic-password -a Terminus \
  -s terminus.openai.api_key \
  -w "$OPENAI_API_KEY" -U
```

**Anthropic:**
```bash
security add-generic-password -a Terminus \
  -s terminus.anthropic.api_key \
  -w "$ANTHROPIC_API_KEY" -U
```

Secrets are **never** stored in SQLite or version control.

---

## Future Architecture

### Planned Enhancements

1. **Relay Transport**
   - Hosted backend for shared API keys
   - Multi-tenant billing
   - Rate limit pooling

2. **Background Scheduler**
   - Cron-like recurring runs
   - Wake-on-demand triggers

3. **Multi-Provider Routing**
   - Automatic fallback on provider failure
   - Cost-based routing

4. **Workflow Composition**
   - Chain multiple recipes
   - Conditional branching

---

## Performance Characteristics

**SQLite:**
- All operations use indexed queries
- Transaction-scoped writes
- ~1ms per tick on M1 Mac

**Memory:**
- Plans stored as JSON in DB (not in memory)
- Stateless runner (fetch from DB each tick)
- ~10MB baseline memory footprint

**Startup Time:**
- Cold start: <500ms
- Schema bootstrap: <50ms
- Keychain read: <100ms

---

## Security Model

See `docs/threat_model.md` for full threat analysis.

**Key Mitigations:**

1. **No eval/arbitrary code execution** - Only predefined primitives
2. **Approval gates on all writes** - User confirms before side-effects
3. **Secrets in Keychain** - Never in SQLite or logs
4. **Redacted receipts** - API keys, emails, PII stripped
5. **Bounded execution** - Spend caps, retry limits, timeout protection

---

## Debugging

### Enable SQL Logging

```rust
// In db.rs:
conn.trace(Some(|stmt| println!("SQL: {}", stmt)));
```

### Inspect Database

```bash
cd ~/.openclaw/workspace/terminus/src-tauri
sqlite3 terminus.db

.tables
SELECT * FROM runs;
SELECT * FROM activities ORDER BY created_at DESC LIMIT 10;
```

### Test Mock Scenarios

```typescript
// Trigger specific test behavior via intent text:
const run = await invoke('start_recipe_run', {
  intent: 'simulate_provider_retryable_failure', // Forces retry
  // ...
})
```

---

## File Structure

```
terminus/
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── main.rs         # Tauri entry point + IPC commands
│   │   ├── runner.rs       # State machine (1800+ lines)
│   │   ├── db.rs           # SQLite schema + bootstrap
│   │   ├── schema.rs       # AutopilotPlan + recipes
│   │   ├── primitives.rs   # Primitive guard
│   │   ├── providers/      # Provider abstraction
│   │   │   ├── mod.rs      # ProviderRuntime
│   │   │   ├── types.rs    # Request/Response/Error
│   │   │   ├── keychain.rs # macOS Keychain reader
│   │   │   └── runtime.rs  # Error classification
│   │   └── transport/      # Execution transports
│   │       ├── mod.rs
│   │       ├── mock.rs     # Test transport
│   │       └── local_http.rs # Real API calls
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/                    # React frontend
│   ├── App.tsx             # Main UI
│   ├── types.ts            # TypeScript types
│   └── styles.css
├── docs/                   # Design docs
│   ├── plan.md
│   ├── plan_schema_examples.json
│   └── threat_model.md     # (from Phase B)
├── ARCHITECTURE.md         # This file
├── CONTRIBUTING.md         # Dev guide
├── TESTING.md              # Test strategy
├── README.md
└── mission_control.md      # Task tracker
```

---

**For setup instructions, see `CONTRIBUTING.md`**  
**For test details, see `TESTING.md`**
