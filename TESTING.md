# Testing Guide

**Last Updated:** 2026-02-13  
**Test Suite:** 26 tests (26 passing)

---

## Overview

Terminus uses a comprehensive test strategy covering unit tests, integration tests, and end-to-end flows. All business logic is tested with high coverage, especially around state transitions, error handling, and spend safety.

---

## Running Tests

### Full Test Suite

```bash
cd src-tauri
cargo test
```

**Expected output:**
```
running 26 tests
test runner::tests::... ok
...
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured
```

### Run Specific Test

```bash
cargo test test_name
```

**Example:**
```bash
cargo test retry_exhaustion
```

### Run with Output

```bash
cargo test -- --nocapture
```

Shows `println!` output from tests.

### Run with Backtrace

```bash
RUST_BACKTRACE=1 cargo test
```

Shows full stack trace on failures.

---

## Test Organization

Tests are organized in `#[cfg(test)]` modules at the bottom of source files:

```
src-tauri/src/
├── runner.rs
│   └── mod tests { ... }      # 23 tests - core state machine
├── schema.rs
│   └── mod tests { ... }      # 1 test - plan construction
└── transport/local_http.rs
    └── mod tests { ... }      # 2 tests - real API gating
```

---

## Test Categories

### 1. State Machine Tests

**Location:** `src/runner.rs` (tests module)

**Coverage:**
- ✅ Happy paths for all 3 recipes
- ✅ Retry logic (retryable vs non-retryable)
- ✅ Retry exhaustion
- ✅ Approval flow (approve/reject)
- ✅ Spend caps (soft/hard, boundary cases)
- ✅ Idempotency (duplicate keys, spend ledger)
- ✅ Concurrent runs
- ✅ State transition atomicity
- ✅ Activity logging
- ✅ Provider error classification
- ✅ Database constraints

**Key Tests:**

```rust
#[test]
fn website_monitor_happy_path_shared_runtime()
#[test]
fn retries_only_retryable_provider_errors()
#[test]
fn retry_exhaustion_transitions_to_failed_state()
#[test]
fn approval_rejection_transitions_to_canceled()
#[test]
fn spend_cap_boundary_cases_are_precise()
#[test]
fn idempotency_key_collision_returns_existing_run()
```

### 2. Schema Tests

**Location:** `src/schema.rs` (tests module)

**Coverage:**
- ✅ Shared plan schema for all recipes
- ✅ Provider metadata mapping
- ✅ Primitive allowlists

```rust
#[test]
fn builds_shared_plan_schema_for_all_three_recipes()
```

### 3. Transport Tests

**Location:** `src/transport/local_http.rs` (tests module)

**Coverage:**
- ✅ Real API calls are env-gated
- ✅ MockTransport is default

```rust
#[test]
fn live_openai_call_is_env_gated()
#[test]
fn live_anthropic_call_is_env_gated()
```

---

## Test Transports

Terminus uses **test transports** for deterministic testing.

### MockTransport (Default)

**Behavior:**
- No real API calls
- Deterministic responses
- Special intent keywords trigger test scenarios

**Trigger Keywords:**

| Intent Text | Behavior |
|------------|----------|
| `"simulate_provider_retryable_failure"` | First attempt fails (retryable), second succeeds |
| `"simulate_provider_non_retryable_failure"` | Fails immediately, no retry |
| `"simulate_cap_hard"` | 95 cents (exceeds hard cap) |
| `"simulate_cap_soft"` | 45 cents (triggers soft cap approval) |
| `"simulate_cap_boundary"` | 80 cents (at hard cap boundary) |
| Normal text | 12 cents (succeeds) |

**Example:**
```rust
let plan = plan_with_single_write_step("simulate_provider_retryable_failure");
let run = RunnerEngine::start_run(&mut conn, "auto_test", plan, "key", 2)?;

let first = RunnerEngine::run_tick(&mut conn, &run.id)?;
assert_eq!(first.state, RunState::Retrying); // Fails first time

// Force retry to be due
conn.execute("UPDATE runs SET next_retry_at_ms = 0 WHERE id = ?1", params![run.id])?;

let resumed = RunnerEngine::resume_due_runs(&mut conn, 10)?;
assert_eq!(resumed[0].state, RunState::Succeeded); // Succeeds on retry
```

### LocalHttpTransport (Real APIs)

**Requires:**
- API keys in macOS Keychain
- `TERMINUS_TRANSPORT=local_http` env variable

**Testing:**
```bash
# Store keys (once)
security add-generic-password -a Terminus \
  -s terminus.openai.api_key -w "sk-..." -U

# Run with real APIs
TERMINUS_TRANSPORT=local_http cargo test
```

**Cost:** Real API calls cost money. Use sparingly.

---

## Test Helpers

### setup_conn()

Creates in-memory SQLite connection with schema:

```rust
fn setup_conn() -> Connection {
    let mut conn = Connection::open_in_memory().expect("open memory db");
    bootstrap_schema(&mut conn).expect("bootstrap schema");
    conn
}
```

### plan_with_single_write_step()

Creates minimal test plan:

```rust
fn plan_with_single_write_step(intent: &str) -> AutopilotPlan {
    AutopilotPlan {
        schema_version: "1.0".to_string(),
        recipe: RecipeKind::WebsiteMonitor,
        intent: intent.to_string(),
        provider: ProviderMetadata::from_provider_id(ProviderId::OpenAi),
        allowed_primitives: vec![PrimitiveId::WriteOutcomeDraft],
        steps: vec![PlanStep {
            id: "step_1".to_string(),
            label: "Write draft outcome".to_string(),
            primitive: PrimitiveId::WriteOutcomeDraft,
            requires_approval: false,
            risk_tier: RiskTier::Low,
        }],
    }
}
```

---

## Writing New Tests

### Test Template

```rust
#[test]
fn descriptive_test_name() {
    // Setup
    let mut conn = setup_conn();
    let plan = plan_with_single_write_step("test intent");
    
    // Execute
    let run = RunnerEngine::start_run(&mut conn, "auto_test", plan, "idem_test", 1)
        .expect("start run");
    
    // Tick to advance state
    let result = RunnerEngine::run_tick(&mut conn, &run.id)
        .expect("tick");
    
    // Assert
    assert_eq!(result.state, RunState::Succeeded);
    
    // Verify database state (optional)
    let db_run = RunnerEngine::get_run(&conn, &run.id).expect("get run");
    assert_eq!(db_run.usd_cents_actual, 12);
}
```

### Testing Error Paths

```rust
#[test]
fn handles_non_retryable_error() {
    let mut conn = setup_conn();
    let plan = plan_with_single_write_step("simulate_provider_non_retryable_failure");
    let run = RunnerEngine::start_run(&mut conn, "auto_fail", plan, "idem_fail", 1)?;
    
    let result = RunnerEngine::run_tick(&mut conn, &run.id)?;
    
    assert_eq!(result.state, RunState::Failed);
    assert_eq!(result.retry_count, 0);
    assert!(result.failure_reason.is_some());
}
```

### Testing Approvals

```rust
#[test]
fn requires_approval_for_write_action() {
    let mut conn = setup_conn();
    let mut plan = plan_with_single_write_step("approval test");
    plan.steps[0].requires_approval = true; // Force approval gate
    
    let run = RunnerEngine::start_run(&mut conn, "auto_approve", plan, "idem_approve", 1)?;
    
    // Tick to approval gate
    let paused = RunnerEngine::run_tick(&mut conn, &run.id)?;
    assert_eq!(paused.state, RunState::NeedsApproval);
    
    // Verify approval was created
    let approvals = RunnerEngine::list_pending_approvals(&conn)?;
    let approval = approvals.iter().find(|a| a.run_id == run.id).unwrap();
    
    // Approve and verify continuation
    let resumed = RunnerEngine::approve(&mut conn, &approval.id)?;
    assert_eq!(resumed.state, RunState::Ready);
}
```

### Testing Database Atomicity

```rust
#[test]
fn transaction_rolls_back_on_failure() {
    let mut conn = setup_conn();
    let plan = plan_with_single_write_step("atomicity test");
    let run = RunnerEngine::start_run(&mut conn, "auto_atomic", plan, "idem_atomic", 1)?;
    
    // Use a test helper that forces transaction failure
    RunnerEngine::transition_state_with_forced_failure(
        &mut conn,
        &run.id,
        RunState::Ready,
        RunState::Failed,
    ).expect_err("forced failure should abort");
    
    // Verify state wasn't changed (transaction rolled back)
    let post = RunnerEngine::get_run(&conn, &run.id)?;
    assert_eq!(post.state, RunState::Ready);
}
```

---

## Test Coverage Goals

### Core Coverage (Mandatory)

- ✅ All state transitions
- ✅ Error handling (retryable/non-retryable)
- ✅ Approval flow (approve/reject)
- ✅ Spend caps (soft/hard)
- ✅ Idempotency guarantees
- ✅ Atomic transactions
- ✅ Activity logging

### Edge Cases (High Priority)

- ✅ Retry exhaustion
- ✅ Concurrent runs
- ✅ Spend cap boundaries (39¢, 40¢, 79¢, 80¢, 81¢)
- ✅ Duplicate idempotency keys
- ✅ Invalid state transitions
- ✅ Orphaned approvals

### Integration (Nice to Have)

- ✅ End-to-end recipe flows
- ✅ Real provider calls (env-gated)
- ⬜ Frontend ↔ backend IPC
- ⬜ Keychain integration
- ⬜ Database migrations

---

## Debugging Failed Tests

### 1. Read the Assertion

```
assertion `left == right` failed
  left: Retrying
 right: Succeeded
```

**Diagnosis:** State machine transitioned to unexpected state.

**Fix:** Check MockTransport behavior for the intent text used.

### 2. Enable SQL Logging

In `src-tauri/src/db.rs`:
```rust
conn.trace(Some(|stmt| println!("SQL: {}", stmt)));
```

### 3. Inspect Database State

```rust
#[test]
fn debug_database_state() {
    let mut conn = setup_conn();
    // ... test setup ...
    
    // Dump database state
    let runs: Vec<String> = conn.prepare("SELECT * FROM runs")
        .unwrap()
        .query_map([], |row| {
            Ok(format!("{:?}", row))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    
    println!("Runs: {:#?}", runs);
    
    // ... assertions ...
}
```

### 4. Add Tracing

```rust
let result = RunnerEngine::run_tick(&mut conn, &run.id)?;
println!("After tick: state={:?}, retry_count={}", result.state, result.retry_count);
```

---

## Continuous Integration

### GitHub Actions (Future)

**Planned workflow:**

```yaml
name: Test
on: [push, pull_request]
jobs:
  test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      - run: cd src-tauri && cargo test
      - run: pnpm install && pnpm build
```

**Current state:** No CI yet (MVP development).

---

## Performance Benchmarks

### Test Suite Performance

**Current:**
- 26 tests
- ~0.08 seconds total
- ~3ms per test average

**Targets:**
- Keep under 1 second for full suite
- Keep under 5ms per test

### Database Operations

**Typical costs:**
- `start_run`: ~1ms
- `run_tick`: ~1-2ms
- `approve`: ~1ms
- `get_run`: <1ms

---

## Test Data Patterns

### Intent Text Conventions

Use descriptive intent text for readability:

```rust
// Good
let plan = plan_with_single_write_step("Test retry exhaustion with 2 max retries");

// Avoid
let plan = plan_with_single_write_step("test");
```

### Idempotency Keys

Use unique keys per test:

```rust
// Good - includes test name
let run = RunnerEngine::start_run(&mut conn, "auto_test", plan, "idem_test_retry", 1)?;

// Avoid - generic keys
let run = RunnerEngine::start_run(&mut conn, "auto", plan, "key1", 1)?;
```

### Autopilot IDs

Use descriptive autopilot IDs:

```rust
// Good
let run = RunnerEngine::start_run(&mut conn, "auto_website_monitor", plan, "key", 1)?;

// Avoid
let run = RunnerEngine::start_run(&mut conn, "auto1", plan, "key", 1)?;
```

---

## Common Pitfalls

### 1. Forgetting to Force Retry Timing

❌ **Wrong:**
```rust
let run = RunnerEngine::start_run(&mut conn, "auto", plan, "key", 1)?;
let first = RunnerEngine::run_tick(&mut conn, &run.id)?;
let resumed = RunnerEngine::resume_due_runs(&mut conn, 10)?; // Won't resume yet!
```

✅ **Right:**
```rust
let run = RunnerEngine::start_run(&mut conn, "auto", plan, "key", 1)?;
let first = RunnerEngine::run_tick(&mut conn, &run.id)?;

// Force retry to be due immediately
conn.execute("UPDATE runs SET next_retry_at_ms = 0 WHERE id = ?1", params![run.id])?;

let resumed = RunnerEngine::resume_due_runs(&mut conn, 10)?; // Now it resumes
```

### 2. Assuming MockTransport Behavior

MockTransport only fails **once** per correlation_id, then succeeds. Don't expect persistent failures:

❌ **Wrong assumption:**
```rust
// Expecting this to fail forever
let plan = plan_with_single_write_step("simulate_provider_retryable_failure");
```

✅ **Right understanding:**
```rust
// First attempt fails, retry succeeds
let plan = plan_with_single_write_step("simulate_provider_retryable_failure");
```

### 3. Not Verifying Database State

Tests should verify both return values AND database state:

✅ **Good:**
```rust
let run = RunnerEngine::run_tick(&mut conn, &run.id)?;
assert_eq!(run.state, RunState::Succeeded);

// Also verify database
let db_run = RunnerEngine::get_run(&conn, &run.id)?;
assert_eq!(db_run.state, RunState::Succeeded);
```

---

## Adding Test Coverage

When adding new features, always add tests:

**1. Write the test first** (TDD-style):
```rust
#[test]
fn new_feature_works() {
    // This will fail initially
    todo!("implement new feature");
}
```

**2. Implement the feature**

**3. Verify test passes**

**4. Add edge case tests:**
- Happy path ✅
- Error path ✅
- Boundary conditions ✅
- Concurrent access (if applicable) ✅

---

## Test Maintenance

### Keeping Tests Fast

- Use in-memory SQLite
- Avoid real API calls (use MockTransport)
- Don't add `sleep()` calls
- Keep test data small

### Keeping Tests Reliable

- Don't depend on external services
- Use deterministic MockTransport
- Avoid race conditions
- Clean up test data (in-memory DB auto-cleans)

### Keeping Tests Readable

- Use descriptive test names
- Add comments for complex setups
- Use helper functions
- Group related assertions

---

**For development setup, see `CONTRIBUTING.md`**  
**For architecture details, see `ARCHITECTURE.md`**
