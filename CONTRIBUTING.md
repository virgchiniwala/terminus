# Contributing to Terminus

**Last Updated:** 2026-02-13

Welcome! This guide will help you set up Terminus for local development and understand our contribution workflow.

---

## Quick Start

### Prerequisites

**Required:**
- **macOS** (Tauri currently targets macOS only)
- **Node.js 18+** (`node --version`)
- **Rust 1.70+** (`rustc --version`)
- **pnpm** (`npm install -g pnpm`)

**Optional:**
- OpenAI API key (for real execution testing)
- Anthropic API key (for real execution testing)

### 1. Clone the Repository

```bash
git clone https://github.com/virgchiniwala/terminus.git
cd terminus
```

### 2. Install Dependencies

```bash
pnpm install
```

This installs both frontend (React) and backend (Rust) dependencies.

### 3. Run in Development Mode

```bash
pnpm tauri dev
```

This starts:
- React dev server (Vite) on port 1420
- Tauri window with hot reload
- Rust backend with SQLite in-memory database

**First run takes ~60 seconds** (Rust compilation). Subsequent runs are faster (~5s).

---

## Project Structure

```
terminus/
├── src/                    # React frontend
│   ├── App.tsx             # Main UI component
│   ├── types.ts            # TypeScript type definitions
│   ├── main.tsx            # React entry point
│   └── styles.css          # Global styles
├── src-tauri/              # Rust backend
│   ├── src/
│   │   ├── main.rs         # Tauri + IPC commands
│   │   ├── runner.rs       # State machine (core logic)
│   │   ├── db.rs           # SQLite schema + operations
│   │   ├── schema.rs       # AutopilotPlan + recipes
│   │   ├── primitives.rs   # Primitive action guard
│   │   ├── providers/      # LLM provider abstraction
│   │   └── transport/      # Execution transports
│   ├── Cargo.toml          # Rust dependencies
│   └── tauri.conf.json     # Tauri configuration
├── docs/                   # Design documentation
├── ARCHITECTURE.md         # System design overview
├── CONTRIBUTING.md         # This file
├── TESTING.md              # Test strategy & guides
├── README.md               # Project README
└── mission_control.md      # Task tracker
```

---

## Development Workflow

### Running Tests

**Rust tests:**
```bash
cd src-tauri
cargo test
```

**Run specific test:**
```bash
cargo test --test test_name
```

**With output:**
```bash
cargo test -- --nocapture
```

**Frontend build validation:**
```bash
pnpm build
```

### Code Formatting

**Rust:**
```bash
cd src-tauri
cargo fmt
cargo clippy
```

**TypeScript:**
```bash
pnpm lint
pnpm format
```

### Database Inspection

Development uses in-memory SQLite. To inspect a persisted DB:

```bash
cd src-tauri
sqlite3 terminus.db

.tables
.schema runs
SELECT * FROM runs;
```

---

## Testing with Real Providers

By default, Terminus uses `MockTransport` (no real API calls). To test with real OpenAI/Anthropic:

### 1. Store API Keys in Keychain

**OpenAI:**
```bash
security add-generic-password \
  -a Terminus \
  -s terminus.openai.api_key \
  -w "sk-your-key-here" \
  -U
```

**Anthropic:**
```bash
security add-generic-password \
  -a Terminus \
  -s terminus.anthropic.api_key \
  -w "sk-ant-your-key-here" \
  -U
```

**Verify keys stored:**
```bash
security find-generic-password -a Terminus -s terminus.openai.api_key
```

### 2. Run with LocalHttpTransport

```bash
TERMINUS_TRANSPORT=local_http pnpm tauri dev
```

Now API calls will use real providers and charge your account.

---

## Making Changes

### 1. Create a Branch

```bash
git checkout -b feat/your-feature-name
```

**Branch naming:**
- `feat/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation
- `refactor/` - Code improvements
- `test/` - Test additions

### 2. Make Your Changes

**Guidelines:**
- Write tests for new features
- Keep commits small and focused
- Use descriptive commit messages
- Document complex logic

**Commit message format:**
```
<type>: <subject>

<body (optional)>
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`

**Example:**
```
feat: add retry exhaustion limit to runner

- Adds max_retries parameter to start_run
- Transitions to Failed after exhaustion
- Includes test coverage for edge cases
```

### 3. Run Tests

```bash
# Rust tests
cd src-tauri && cargo test

# Build validation
cd .. && pnpm build
```

All tests must pass before submitting.

### 4. Push and Create PR

```bash
git push origin feat/your-feature-name
```

Then create a Pull Request on GitHub.

**PR Guidelines:**
- Link related issues
- Describe what changed and why
- Include screenshots for UI changes
- Ensure CI passes

---

## Code Style

### Rust

**Follow Rust conventions:**
- `snake_case` for functions and variables
- `PascalCase` for types and structs
- Use `rustfmt` for formatting
- Run `clippy` for linting

**Error handling:**
```rust
// Good: use Result<T, E>
pub fn start_run(...) -> Result<RunRecord, RunnerError> {
    // ...
}

// Avoid: unwrap() in production code
let value = some_option.unwrap(); // ❌

// Better: proper error handling
let value = some_option.ok_or(RunnerError::Missing)?; // ✅
```

**Database transactions:**
```rust
// Always use transactions for multi-step writes
let tx = conn.transaction()?;
tx.execute("UPDATE runs SET state = ?1 WHERE id = ?2", ...)?;
tx.execute("INSERT INTO activities ...", ...)?;
tx.commit()?; // Atomic commit
```

### TypeScript

**Use TypeScript types:**
```typescript
// Good: explicit types
interface RunRecord {
  id: string;
  state: RunState;
  usd_cents_actual: number;
}

// Avoid: any types
const data: any = await invoke('get_run'); // ❌

// Better: typed invoke
const run: RunRecord = await invoke('get_run', { runId }); // ✅
```

**Tauri IPC calls:**
```typescript
import { invoke } from '@tauri-apps/api/core';

// Always handle errors
try {
  const result = await invoke('command_name', { param: value });
} catch (error) {
  console.error('Command failed:', error);
  // Handle error in UI
}
```

---

## Adding New Features

### Adding a New Primitive

1. **Define the primitive** in `src-tauri/src/schema.rs`:
```rust
pub enum PrimitiveId {
    ReadWebsite,
    WriteOutcomeDraft,
    SendEmail,
    YourNewPrimitive, // Add here
}
```

2. **Update the primitive guard** in `src-tauri/src/primitives.rs`:
```rust
pub fn check(primitive: &PrimitiveId, allowed: &[PrimitiveId]) -> Result<(), PrimitiveGuardError> {
    if !allowed.contains(primitive) {
        return Err(PrimitiveGuardError::Denied(
            format!("Action '{}' is not allowed yet.", primitive.as_str())
        ));
    }
    Ok(())
}
```

3. **Implement execution logic** in `src-tauri/src/runner.rs`:
```rust
fn execute_step(...) -> Result<ProviderResponse, RunnerError> {
    match step.primitive {
        PrimitiveId::YourNewPrimitive => {
            // Your implementation here
        }
        // ... existing primitives
    }
}
```

4. **Add tests**:
```rust
#[test]
fn your_new_primitive_works() {
    let mut conn = setup_conn();
    let plan = plan_with_primitive(PrimitiveId::YourNewPrimitive);
    // ... test implementation
}
```

### Adding a New Recipe

1. **Define recipe** in `src-tauri/src/schema.rs`:
```rust
pub enum RecipeKind {
    WebsiteMonitor,
    InboxTriage,
    DailyBrief,
    YourNewRecipe, // Add here
}
```

2. **Implement plan constructor**:
```rust
impl AutopilotPlan {
    pub fn from_intent(recipe: RecipeKind, intent: String, provider: ProviderId) -> Self {
        match recipe {
            RecipeKind::YourNewRecipe => Self {
                schema_version: "1.0".to_string(),
                recipe,
                intent,
                provider: ProviderMetadata::from_provider_id(provider),
                allowed_primitives: vec![/* your primitives */],
                steps: vec![/* your steps */],
            },
            // ... existing recipes
        }
    }
}
```

3. **Add happy path test**:
```rust
#[test]
fn your_recipe_happy_path() {
    let mut conn = setup_conn();
    let plan = AutopilotPlan::from_intent(
        RecipeKind::YourNewRecipe,
        "Test intent".to_string(),
        ProviderId::OpenAi
    );
    // ... test execution
}
```

4. **Update frontend** to expose the recipe in UI.

---

## Testing Guidelines

See `TESTING.md` for comprehensive testing guide.

**Key principles:**
- Write tests for happy paths
- Write tests for failure paths
- Test boundary conditions
- Use MockTransport for deterministic tests
- Use LocalHttpTransport for integration tests
- Verify database state after operations

**Test template:**
```rust
#[test]
fn descriptive_test_name() {
    let mut conn = setup_conn();
    
    // Setup: create test data
    let plan = plan_with_single_write_step("test intent");
    let run = RunnerEngine::start_run(&mut conn, "auto_test", plan, "idem_test", 1)
        .expect("start run");
    
    // Execute: perform the action being tested
    let result = RunnerEngine::run_tick(&mut conn, &run.id)
        .expect("tick");
    
    // Assert: verify expected behavior
    assert_eq!(result.state, RunState::Succeeded);
    
    // Verify database state if needed
    let db_run = RunnerEngine::get_run(&conn, &run.id).expect("get run");
    assert_eq!(db_run.usd_cents_actual, 12);
}
```

---

## Debugging

### Enable Rust Debug Logging

```bash
RUST_LOG=debug pnpm tauri dev
```

**Log levels:** `error`, `warn`, `info`, `debug`, `trace`

### Enable SQL Tracing

In `src-tauri/src/db.rs`, uncomment:
```rust
conn.trace(Some(|stmt| println!("SQL: {}", stmt)));
```

### Inspect SQLite Database

```bash
cd src-tauri
sqlite3 terminus.db

.schema
SELECT * FROM runs;
SELECT * FROM activities ORDER BY created_at DESC LIMIT 10;
```

### Test Specific Mock Scenarios

Use special intent keywords to trigger test behaviors:
- `"simulate_provider_retryable_failure"` - Triggers retry logic
- `"simulate_provider_non_retryable_failure"` - Triggers immediate failure
- `"simulate_cap_hard"` - Triggers hard spend cap
- `"simulate_cap_soft"` - Triggers soft spend cap
- `"simulate_cap_boundary"` - Exactly at hard cap boundary

**Example:**
```typescript
const run = await invoke('start_recipe_run', {
  intent: 'simulate_provider_retryable_failure',
  // ... other params
});
// Run will fail once, then retry and succeed
```

---

## Common Tasks

### Adding a New Tauri Command

1. **Define command** in `src-tauri/src/main.rs`:
```rust
#[tauri::command]
fn your_new_command(param: String) -> Result<YourReturnType, String> {
    // Implementation
    Ok(result)
}
```

2. **Register in builder**:
```rust
tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![
        your_new_command,
        // ... existing commands
    ])
```

3. **Call from frontend**:
```typescript
import { invoke } from '@tauri-apps/api/core';

const result = await invoke('your_new_command', { param: 'value' });
```

### Updating Database Schema

1. **Modify schema** in `src-tauri/src/db.rs`:
```rust
pub fn bootstrap_schema(conn: &mut Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS your_table (...)",
        []
    )?;
    // ... existing tables
    Ok(())
}
```

2. **Migration strategy** (for production):
   - Add schema versioning
   - Write migration logic
   - Test on copy of production DB
   - Document breaking changes

**For MVP:** Schema changes require fresh DB (delete `terminus.db` and restart).

---

## Getting Help

**Documentation:**
- `ARCHITECTURE.md` - System design & components
- `TESTING.md` - Test strategy & examples
- `README.md` - Project overview
- `docs/` - Design documents

**Questions:**
- Open an issue on GitHub
- Tag `@virgchiniwala` in PR comments

**Bug Reports:**
Include:
1. Steps to reproduce
2. Expected behavior
3. Actual behavior
4. Logs (if applicable)
5. Environment (macOS version, Rust version, Node version)

---

## Release Process

(To be defined as project matures)

**Current state:** MVP development, no formal releases yet.

---

## License

MIT - see `LICENSE` file for details.

---

**Thank you for contributing to Terminus!** 🦾
