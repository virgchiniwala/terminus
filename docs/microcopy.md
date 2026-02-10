# UI Microcopy — Personal AI OS

**Voice:** Calm, confident, human. We explain what will happen before it happens. We show, don't tell. We earn trust through clarity, not marketing speak.

**Tone:** Minimal, warm, technically precise when needed. Never condescending. Assume intelligence, not expertise.

---

## Core Principles

1. **Say what will happen, not what you're doing.** ✅ "We'll check this website daily" vs ❌ "Configuring scheduler"
2. **Show cost and permissions upfront.** Trust = transparency.
3. **Errors suggest one-click fixes.** Never dead-end the user.
4. **Celebrate small wins.** "Your first automation just ran perfectly." feels good.
5. **Default to safe.** Drafts, previews, read-only until explicitly approved.

---

## 1. Onboarding Flow

### Welcome Screen
**Headline:**  
"Automate anything safely."

**Subhead:**  
"Describe what you want. Review the plan. Get repeatable results. No code required."

**CTA Button:**  
"Get Started" (primary blue)

**Skip Link (bottom):**  
"I've done this before" → skips to dashboard

---

### Connect LLM Provider
**Headline:**  
"First, connect an AI provider"

**Body:**  
"We use OpenAI or Anthropic to understand your requests and generate automations. Your API key stays on your device — we never see it or log it."

**Provider Cards:**  
- **OpenAI** — "GPT-4o" (badge: Recommended)
- **Anthropic** — "Claude Sonnet"

**Input Label:**  
"API Key" (password field)

**Help Text (below input):**  
"Get your key from platform.openai.com/api-keys. We store it securely in your system keychain."

**CTA Button:**  
"Continue" (disabled until valid key entered)

**Link (bottom):**  
"Where is my API key?" → opens help doc

---

### Choose Starter Automation
**Headline:**  
"Let's run your first automation"

**Subhead:**  
"Pick one of these starters to see how it works. You can create custom automations next."

#### Starter Cards (3)

**Card 1: Website Monitor**  
*Icon:* 🌐  
**Title:** "Monitor a Website"  
**Description:** "Get notified when a page changes. We'll summarize the updates and draft an email."  
**Time:** ~2 min setup  
**Badge:** Most Popular

**Card 2: Research Assistant**  
*Icon:* 🔍  
**Title:** "Research a Topic"  
**Description:** "Give us a topic. We'll search, summarize key findings, and save a structured report."  
**Time:** ~2 min setup

**Card 3: Inbox Triage**  
*Icon:* ✉️  
**Title:** "Triage Your Inbox"  
**Description:** "We'll scan unread emails, flag urgent ones, and draft replies. You approve before sending."  
**Time:** ~3 min setup  
**Badge:** Requires email connection

**Link (bottom):**  
"Skip to custom automation" → goes to intent bar

---

### Website Monitor Setup (Example)
**Headline:**  
"Monitor a Website"

**Step Indicator:**  
"Step 1 of 2"

**Input Label:**  
"Website URL"

**Placeholder:**  
"https://example.com/blog"

**Help Text:**  
"We'll check this page daily and let you know what changed."

**Optional Toggle:**  
☐ "Only notify me if these keywords appear" (collapsed by default)  
*When expanded:* Input field with placeholder "urgent, new feature, pricing"

**Buttons:**  
- "Cancel" (secondary, left)
- "Next" (primary, right, disabled until valid URL)

---

**Step 2: Review & Test**

**Headline:**  
"Review your automation"

**Plan Cards (4 cards shown as vertical list):**

**Card 1: Data Source**  
*Icon:* 🌐  
**Label:** "Will read from"  
**Value:** "example.com/blog"  
**Permission Badge:** Read-only • Safe  
**Action Link:** "Change URL"

**Card 2: Schedule**  
*Icon:* ⏰  
**Label:** "Runs"  
**Value:** "Daily at 9:00 AM"  
**Action Link:** "Change schedule"

**Card 3: Action**  
*Icon:* ✉️  
**Label:** "Will create"  
**Value:** "Email draft (requires your approval before sending)"  
**Permission Badge:** Draft mode • Safe  
**Action Link:** "Change action"

**Card 4: Cost Estimate**  
*Icon:* 💰  
**Label:** "Estimated cost per run"  
**Value:** "~$0.03" (±2,000 tokens)  
**Help Text:** "Based on typical page size. Actual cost may vary."

**Cost Summary (bottom of cards):**  
"Daily cost: ~$0.03 • Monthly: ~$0.90"

**CTA Buttons:**  
- "Back" (secondary, left)
- "Run Test" (primary, right)

**Legal Text (tiny, bottom):**  
"By running, you agree to our Terms. We never store page content — only summaries you create."

---

### Test Run (Live Activity)
**Headline:**  
"Running your first automation..."

**Activity Feed (live updates):**

```
✓ Connected to example.com/blog
✓ Fetched page content (1,243 words)
⏳ Analyzing changes...
✓ Found 3 new articles
⏳ Generating summary...
✓ Summary created
⏳ Drafting email...
✓ Draft ready
```

**Progress Bar:** Animates as steps complete

**CTA (appears when done):**  
"View Results" (primary)

---

### Test Results
**Headline:**  
"Your automation worked!"

**Subhead:**  
"Here's what we found:"

**Results Card:**  
**Summary (expandable text):**  
"3 new articles published today:
1. 'Introducing Dark Mode' — major UI update
2. 'Bug Fixes in v2.1' — maintenance release  
3. 'Hiring: Senior Engineer' — job post"

**Draft Email (preview):**
```
Subject: example.com/blog updates — 3 new articles

Hi,

Three new articles were published on example.com/blog today:

[... preview of draft ...]

— Your Personal AI
```

**Action Buttons:**
- "Edit Draft" → opens email composer
- "Send Now" → confirmation modal
- "Discard" (text link, subtle)

**Save Automation Panel (bottom):**  
"Want to run this daily?"  
**Toggle:** ☐ "Yes, run daily at 9:00 AM"  
**Button:** "Save Automation" (primary)

**Link:**  
"No thanks, just testing" → back to dashboard

---

### First Win Celebration
**Modal (appears after saving first automation):**

**Icon:** 🎉 (large, centered)

**Headline:**  
"You're automating!"

**Body:**  
"Your first automation is live. You'll get an email draft tomorrow at 9:00 AM."

**CTA Button:**  
"Create Another" (primary)

**Link:**  
"View Dashboard" (secondary)

---

## 2. Dashboard

### Main Dashboard (Empty State)
**Headline:**  
"Your Automations"

**Empty State:**  
*Illustration:* Simple line drawing of robot with checklist  
**Text:** "No automations yet. Let's create one."

**CTA Button:**  
"Create Automation" (primary, large)

**Link:**  
"Browse Starters" → back to starter selection

---

### Main Dashboard (With Automations)
**Headline:**  
"Your Automations"

**Filter Tabs:**  
- All (12)
- Active (8)
- Paused (3)
- Failed (1) ← badge: red dot

**Automation Cards (list):**

**Example Card: Website Monitor**  
*Icon:* 🌐 (left)  
**Title:** "example.com/blog updates"  
**Status Badge:** ✓ Active  
**Schedule:** "Daily at 9:00 AM"  
**Last Run:** "2 hours ago" (relative time)  
**Result:** "3 changes found" (green text) OR "No changes" (gray) OR "Failed" (red)  
**Actions (right icons):**
- ⏸️ Pause
- ⚙️ Edit
- ⋯ More (dropdown: Duplicate, Delete, View Logs)

**Quick Stats (top right):**  
"8 active • $2.34 spent today"

**CTA Button (top):**  
"+ Create Automation" (primary, medium)

---

### Automation Detail View
**Breadcrumb:**  
"Automations / example.com/blog updates"

**Header:**  
**Title (editable):** "example.com/blog updates"  
**Status Toggle:** "Active" (switch on) / "Paused" (switch off)  
**Actions (right):**
- "Run Now" (button)
- "Edit" (button)
- ⋯ More (Duplicate, Delete)

**Tabs:**
- **Overview** (default)
- **Activity** (recent runs)
- **Settings**

#### Overview Tab

**Plan Cards (read-only view):**  
Same as setup flow, but condensed:
- Data source: example.com/blog
- Schedule: Daily at 9:00 AM
- Action: Email draft
- Cost: ~$0.03/run

**Recent Runs (mini list, 3 most recent):**
- "2 hours ago • 3 changes found"
- "Yesterday • No changes"
- "2 days ago • Failed" (clickable → view error)

**Link:**  
"View all activity" → Activity tab

#### Activity Tab

**Filter:**  
- All Runs (dropdown: All, Success, Failed, Canceled)
- Date Range (last 7 days)

**Activity List (chronological, newest first):**

**Run Card:**  
**Timestamp:** "Today at 9:03 AM"  
**Status Badge:** ✓ Success (green) / ✗ Failed (red) / ⏸ Canceled (gray)  
**Duration:** "12 seconds"  
**Cost:** "$0.03"  
**Summary:** "3 changes found → Email draft created"  
**Expand Link:** "View details" → shows step-by-step log

**Expanded Log (when clicked):**
```
✓ Connected to example.com/blog (0.8s)
✓ Fetched page content (1.2s)
✓ Detected 3 new articles (2.1s)
✓ Generated summary (4.3s)
✓ Created email draft (3.6s)
```

**Actions (per run):**
- "View Draft" (if email created)
- "Retry" (if failed)
- "Export JSON" (link)

#### Settings Tab

**Sections:**

**1. Schedule**  
**Current:** "Daily at 9:00 AM"  
**Button:** "Change Schedule"

**2. Notifications**  
**Toggle:** ☑ "Notify me when this runs"  
**Help:** "You'll get a system notification with results."

**3. Cost Limits**  
**Per-run cap:** "$0.50" (editable)  
**Daily cap:** "$2.00" (editable)  
**Help:** "Automation will pause if limits are exceeded."

**4. Danger Zone**  
**Red Section:**  
**Button:** "Delete Automation" (destructive, secondary)  
**Warning:** "This can't be undone. Activity logs will be preserved."

---

## 3. Create Custom Automation

### Intent Bar (Main Entry Point)
**Placeholder:**  
"Describe what you want to automate..."

**Examples (below bar, clickable to populate):**
- "Email me when TechCrunch mentions 'AI agents'"
- "Summarize my unread Slack messages daily"
- "Track competitor pricing and log to spreadsheet"

**Button (right of bar):**  
"Create" (primary, disabled until text entered)

**Help Link:**  
"What can I automate?" → opens help drawer with examples

---

### Planning (Generating)
**Headline:**  
"Planning your automation..."

**Body:**  
"Give us a moment to think through this."

**Spinner:** (centered)

**Progress Text (optional, if slow):**  
"Analyzing integrations needed..."  
"Checking permissions..."  
"Estimating cost..."

**Timeout Safety (if >30s):**  
"This is taking longer than expected. [Cancel and try simpler request]"

---

### Review Plan (Proposed)
**Headline:**  
"Review your automation plan"

**Subhead:**  
"We'll do this every time it runs. Approve if it looks right."

**Plan Cards (4-8 cards, vertical):**

**Example Cards:**

**Card: Data Source**  
*Icon:* 🌐  
**Title:** "Read from TechCrunch RSS"  
**Details:** "We'll check techcrunch.com/feed every 6 hours"  
**Permission:** Read-only • Safe  
**Action:** "Change source" (opens edit modal)

**Card: Filter**  
*Icon:* 🔍  
**Title:** "Look for mentions of 'AI agents'"  
**Details:** "Case-insensitive keyword match"  
**Action:** "Edit filter"

**Card: Action**  
*Icon:* ✉️  
**Title:** "Send email to you"  
**Details:** "your@email.com"  
**Permission:** ⚠️ Will send email (not a draft)  
**Toggle:** ☐ "Send drafts only (recommended)" ← default OFF, suggest ON  
**Action:** "Change recipient"

**Card: Schedule**  
*Icon:* ⏰  
**Title:** "Every 6 hours"  
**Details:** "Approximately 4 times per day"  
**Action:** "Change frequency"

**Card: Cost**  
*Icon:* 💰  
**Title:** "~$0.05 per run"  
**Details:** "~$0.20/day, $6/month"  
**Help:** "Includes fetching feed + AI analysis"

**Permission Summary (expandable section below cards):**  
"This automation will:"  
✓ Read public web content (techcrunch.com)  
⚠️ Send emails from your address  
✓ Store results locally on your device

**Action Buttons:**
- "Back" (secondary, left) → returns to intent bar
- "Run Test" (primary, right)

**Link:**  
"Save without testing" (subtle, bottom) → saves but doesn't run

---

### Missing Integration (Modal)
**Triggered when plan needs unconnected integration**

**Headline:**  
"Connect Slack to continue"

**Body:**  
"This automation needs to read your Slack messages. We'll ask for minimal permissions."

**Permission List:**
- ✓ Read channels you're in
- ✓ Read direct messages
- ✗ Post messages (not needed)
- ✗ Access private channels (not needed)

**CTA Button:**  
"Connect Slack" (opens OAuth flow)

**Link:**  
"Cancel" → back to plan editing

---

### Test Run Results (Success)
**Headline:**  
"Test run successful!"

**Results Card:**  
**Found:** "2 articles mentioning 'AI agents'"

**Preview (expandable):**
1. "OpenAI's new agent framework..." (TechCrunch, 2h ago)
2. "AI agents in customer service..." (TechCrunch, 5h ago)

**Email Preview (if action = email):**
```
Subject: TechCrunch AI agents update — 2 new articles

Hi,

Found 2 new articles about AI agents on TechCrunch:

[... preview ...]
```

**CTA Buttons:**
- "Looks Good — Save Automation" (primary)
- "Edit Plan" (secondary)
- "Try Different Intent" (text link)

---

### Test Run Results (Failed)
**Headline:**  
"Test run failed"

**Error Card:**  
*Icon:* ⚠️ (yellow/orange)  
**Error Type:** "Connection Error"  
**Message:** "We couldn't reach techcrunch.com/feed. It might be temporarily down."

**Suggested Fixes (one-click):**
1. **Button:** "Retry Now"
2. **Button:** "Use Different RSS Feed"
3. **Link:** "Edit Plan Manually"

**Technical Details (expandable):**
```
HTTP 503 Service Unavailable
Attempted at: 2026-02-10 14:32:18 UTC
Retries: 3
```

**Bottom Actions:**
- "Cancel Automation" (subtle link)
- "Save Anyway" (secondary) → allows saving even if test failed

---

### Save Confirmation
**Toast (bottom-right, auto-dismiss after 4s):**  
✓ "Automation saved"  
**Action Link:** "View" → goes to automation detail

---

## 4. Edit Automation (Natural Language)

### Edit Entry (from Detail View)
**Button:** "Describe a Change" (secondary)

**Expands to:**  
**Input Bar:**  
**Placeholder:** "What would you like to change?"  
**Examples (below):**
- "Also check Hacker News"
- "Run every 2 hours instead"
- "Add 'machine learning' to keywords"

**Button:** "Propose Edit" (primary)

---

### Proposed Edit (Diff View)
**Headline:**  
"Proposed change"

**Diff Card (shows before/after):**

**Section: Filter**  
**Before:**  
"Keywords: 'AI agents'"

**After:**  
"Keywords: 'AI agents', 'machine learning'"  
*(highlighted in soft yellow)*

**Impact:**  
*Icon:* 💰  
**Cost:** "+~500 tokens/run (+$0.01)"  
**Permissions:** "No new permissions needed"

**CTA Buttons:**
- "Apply Change" (primary, green)
- "Cancel" (secondary)

**Toast after applying:**  
✓ "Change applied"  
**Sub-text:** "Used 0 tokens (direct edit)"

---

### Complex Edit (Re-plan Required)
**When edit can't be done directly:**

**Headline:**  
"This change needs a new plan"

**Body:**  
"Adding Hacker News means we'll need to fetch two sources. Let's rebuild the plan."

**Comparison:**  
**Current cost:** ~$0.05/run  
**New cost:** ~$0.08/run (+60%)

**Buttons:**
- "Generate New Plan" (primary)
- "Cancel"

---

### Undo Support
**After any edit, toast shows:**  
**Action Link:** "Undo" (3s window, then auto-dismiss)

**If clicked:**  
Previous state restored, new toast:  
✓ "Change undone"

---

## 5. Settings & Connections

### Settings Screen
**Tabs:**
- **General**
- **Connections**
- **Cost & Limits**
- **Privacy & Security**

#### General Tab

**Section: LLM Provider**  
**Current:** "OpenAI (GPT-4o)"  
**Button:** "Change Provider"  
**Help:** "Switching providers will re-estimate costs for all automations."

**Section: Notifications**  
**Toggle:** ☑ "Desktop notifications when automations complete"  
**Toggle:** ☐ "Daily summary email"

**Section: Theme**  
**Radio:**  
◉ Light  
○ Dark  
○ Auto (system)

#### Connections Tab

**Integration Cards:**

**Slack**  
*Status:* ✓ Connected  
**Account:** workspace.slack.com  
**Permissions:** Read channels, read DMs  
**Button:** "Disconnect" (destructive)  
**Link:** "Manage permissions" → opens Slack OAuth page

**Email**  
*Status:* ⚠️ Not connected  
**Button:** "Connect Email"  
**Help:** "Required for inbox triage and sending emails."

**Filesystem**  
*Status:* ✓ Sandboxed access  
**Folder:** ~/PersonalAI/automations/  
**Button:** "Change Folder"

#### Cost & Limits Tab

**Current Spend:**  
**Today:** $2.34 of $10.00 (progress bar)  
**This Month:** $42.18

**Per-Run Limits:**  
**Soft cap:** $0.50 (input)  
**Hard cap:** $1.00 (input)  
**Help:** "Soft cap warns you. Hard cap stops the run."

**Daily Limit:**  
**Input:** $10.00  
**Help:** "All automations pause if daily limit is hit."

**Monthly Budget Alert:**  
**Input:** $100.00  
**Toggle:** ☑ "Email me if I exceed this"

**Link:**  
"View detailed cost breakdown" → opens activity log filtered by cost

#### Privacy & Security Tab

**Section: Data Storage**  
**Text:** "All automation data stays on your device. We never send your content to our servers."

**Section: API Keys**  
**Text:** "API keys are stored in your system keychain and never logged or transmitted."  
**Button:** "View Stored Keys" → shows list, allows deletion

**Section: Audit Log**  
**Text:** "Every action is logged locally. Export anytime."  
**Button:** "Export Audit Log" → downloads JSON

**Section: Prompt Injection Protection**  
**Toggle:** ☑ "Block external content from issuing commands (recommended)"  
**Help:** "Prevents malicious websites from hijacking automations."

---

## 6. Errors & Edge Cases

### Error Categories

#### 1. Connection Errors
**Pattern:** "We couldn't reach [service]"

**Examples:**
- "We couldn't reach techcrunch.com. It might be temporarily down."
- "Slack isn't responding. Try again in a few minutes."

**Fixes:**
- "Retry Now" (button)
- "Check [service] status" (link to status page)

#### 2. Permission Errors
**Pattern:** "We don't have permission to [action]"

**Example:**
"We can't post to #general. You need to reconnect Slack with posting permissions."

**Fixes:**
- "Reconnect Slack" (opens OAuth)
- "Change to Read-Only" (converts action to draft mode)

#### 3. Cost Limit Errors
**Pattern:** "This would exceed your [limit type] limit"

**Example:**
"This run needs ~$1.20, but your per-run limit is $1.00."

**Fixes:**
- "Increase Limit to $1.50" (one-click)
- "Run Anyway (Once)" (bypass just this run)
- "Optimize Automation" (suggests ways to reduce cost)

#### 4. Rate Limit Errors
**Pattern:** "[Service] is rate-limiting us"

**Example:**
"OpenAI is rate-limiting us. We'll retry in 30 seconds."

**Fixes:**
- Automatic retry with countdown
- "Skip This Run" (manual override)

#### 5. Validation Errors (User Input)
**Pattern:** Clear field-level error

**Examples:**
- "Please enter a valid URL" (below URL field, red text)
- "API key should start with 'sk-'" (below API key field)
- "Schedule must be at least 30 minutes" (below frequency input)

**Style:** Red text, appears below field, icon: ⚠️

#### 6. Parse/Understanding Errors
**Pattern:** "We couldn't understand this request"

**Example:**
"We're not sure what 'blorgify my zips' means. Can you rephrase?"

**Fixes:**
- "Try Again" (clears input, focus)
- "Browse Examples" (opens help)
- "Start With a Starter" (link to starters)

#### 7. Quota/Token Exhausted
**Modal (blocking):**  
**Headline:** "Daily limit reached"  
**Body:** "You've spent $10.00 today. All automations are paused until tomorrow."

**Options:**
- "Increase Daily Limit" (input field + Save button)
- "Resume Tomorrow" (dismisses modal)
- "View Cost Breakdown" (link to activity log)

---

### Empty States

#### No Integrations Connected
**On Connections tab:**  
*Illustration:* Puzzle pieces  
**Text:** "No integrations connected yet. Connect one to unlock more automations."  
**Button:** "Browse Integrations"

#### No Activity Yet
**On Activity tab:**  
**Text:** "This automation hasn't run yet."  
**Button:** "Run Now"

#### No Failed Runs
**On Activity filtered to 'Failed':**  
**Text:** "🎉 No failures. Everything's running smoothly."

---

## 7. Tooltips & Help Text

### Tooltip Style
- Appears on hover (desktop) or tap-hold (mobile)
- Dark background, white text, small arrow pointing to element
- Max 2 lines, plain language

### Examples

**Token Budget (ⓘ icon):**  
"Tokens measure how much text the AI processes. More tokens = higher cost."

**Soft vs Hard Cap (ⓘ icon):**  
"Soft cap warns you. Hard cap stops the run immediately."

**Draft Mode (ⓘ icon):**  
"Drafts require your approval before sending. Safer for emails and posts."

**Sandbox (ⓘ icon):**  
"Files are isolated in a safe folder. Automations can't access your personal files."

**OAuth (ⓘ icon):**  
"We'll ask [service] for minimal permissions. You can revoke anytime."

**Retry Logic (ⓘ icon):**  
"If a step fails, we'll try 3 times before giving up."

---

## 8. Notifications & Toasts

### Toast Types

#### Success Toast (green accent)
✓ "Automation saved"  
✓ "Test run successful"  
✓ "Change applied"

#### Info Toast (blue accent)
ℹ️ "Checking for updates..."  
ℹ️ "Reconnecting to Slack..."

#### Warning Toast (yellow accent)
⚠️ "Approaching daily limit ($9.50 of $10.00)"  
⚠️ "API key expires in 7 days"

#### Error Toast (red accent)
✗ "Connection failed"  
✗ "Invalid URL"

**Action Links in Toasts:**  
Always provide one-click fix when possible:
- "Retry"
- "View Details"
- "Undo"
- "Increase Limit"

**Auto-Dismiss:**  
Success/Info: 4 seconds  
Warning: 7 seconds  
Error: Stays until dismissed (X button)

---

### System Notifications (macOS/Windows)

#### Automation Completed
**Title:** "Website Monitor • Success"  
**Body:** "3 changes found on example.com"  
**Action:** Click to view results

#### Automation Failed
**Title:** "Website Monitor • Failed"  
**Body:** "Couldn't reach example.com"  
**Action:** Click to retry

#### Daily Summary
**Title:** "Daily Summary • 8 automations ran"  
**Body:** "7 succeeded, 1 failed. Total: $2.34"  
**Action:** Click to view dashboard

---

## 9. Buttons & CTAs

### Button Hierarchy

**Primary (blue, bold):**  
Used for main action on screen. One per screen.  
Examples: "Create Automation", "Run Test", "Save", "Apply Change"

**Secondary (gray border, white bg):**  
Used for alternate actions.  
Examples: "Cancel", "Back", "Edit", "Duplicate"

**Destructive (red border, white bg):**  
Used for dangerous actions.  
Examples: "Delete Automation", "Disconnect", "Stop Run"

**Ghost (text only, no border):**  
Used for tertiary actions.  
Examples: "Skip", "Learn More", "View Details"

### Button Labels

**Be verb-first and specific:**
- ✅ "Run Test" (not "Test")
- ✅ "Save Automation" (not "Save")
- ✅ "Connect Slack" (not "Connect")
- ✅ "Increase Limit" (not "Change")

**Avoid generic labels:**
- ❌ "OK" → use "Got It" or "Continue"
- ❌ "Submit" → use "Create" or "Save"
- ❌ "Yes" → use "Delete Automation" or "Disconnect Slack"

---

## 10. Voice Samples (By Context)

### Calm & Confident
"We'll check this page daily and let you know what changed."  
"Your automation worked perfectly. Here's what we found."

### Transparent About Cost
"This will use ~2,000 tokens per run, approximately $0.03."  
"You're at $9.50 of your $10.00 daily limit."

### Empowering, Not Prescriptive
"Want to run this daily?" (not "You should run this daily")  
"Approve if it looks right." (not "You must review carefully")

### Human, Not Corporate
"Give us a moment to think through this." (not "Processing request...")  
"This is taking longer than expected." (not "Operation timeout imminent")

### Suggest, Don't Scold
"Draft mode is safer for emails." (not "You must enable draft mode")  
"We recommend read-only access first." (not "Read-only access is required")

---

## 11. Loading & Progress States

### Spinner Copy

**Short tasks (<3s):**  
"Loading..."  
"Connecting..."

**Medium tasks (3-10s):**  
"Planning your automation..."  
"Analyzing page content..."  
"Generating summary..."

**Long tasks (>10s):**  
Start with specific step, then show progress:  
"Fetching 20 articles..."  
"Step 2 of 4: Filtering results..."  
"Almost done..."

**Timeout warning (>30s):**  
"This is taking longer than expected. [Cancel and simplify request]"

### Progress Bars

**Show percentage when known:**  
"Downloading... 47%"

**Show step count when multi-step:**  
"Step 3 of 5 • Generating draft..."

**Show time estimate when long:**  
"~2 minutes remaining..."

---

## 12. Confirmation Modals

### Delete Automation
**Headline:**  
"Delete 'Website Monitor'?"

**Body:**  
"This can't be undone. Activity logs will be kept, but the automation will stop running."

**Buttons:**
- "Cancel" (secondary, left)
- "Delete Automation" (destructive, right)

---

### Disconnect Integration
**Headline:**  
"Disconnect Slack?"

**Body:**  
"3 automations use Slack. They'll stop working until you reconnect."

**List:**
- "Daily Slack summary"
- "Urgent mention alerts"
- "Team standup digest"

**Buttons:**
- "Cancel"
- "Disconnect"

---

### Exceed Cost Limit
**Headline:**  
"This will exceed your daily limit"

**Body:**  
"Running this now will cost ~$1.20. Your remaining budget today is $0.50."

**Options (radio):**  
◉ "Increase daily limit to $15.00"  
○ "Run anyway (just this once)"  
○ "Cancel run"

**Button:**  
"Continue" (enabled only when option selected)

---

## 13. Keyboard Shortcuts & Accessibility

### Shortcut Hints (shown on hover or in help)

- `⌘N` — New automation
- `⌘R` — Run test
- `⌘S` — Save automation
- `⌘/` — Focus search/intent bar
- `Esc` — Close modal
- `⌘Z` — Undo last change

### Screen Reader Labels

**All icons must have alt text:**
- Icon: 🌐 → "Website"
- Icon: ✉️ → "Email"
- Icon: ⏰ → "Schedule"
- Icon: ✓ → "Success"
- Icon: ✗ → "Error"

**Button states:**
- "Loading..." (disabled state announced)
- "Save Automation (⌘S)" (shortcut announced)

**Live regions for dynamic updates:**
- Activity feed updates announce: "New run completed: 3 changes found"
- Cost meter updates announce: "Daily spend: $2.34 of $10.00"

---

## 14. Onboarding Tips (Contextual)

**After first automation saved:**  
💡 "Tip: You can edit this automation anytime by describing changes in plain English."

**After first failed run:**  
💡 "Tip: Failed runs don't count toward your daily budget. Retry freely."

**After first cost warning:**  
💡 "Tip: Set per-automation limits in Settings → Cost & Limits."

**After connecting first integration:**  
💡 "Tip: Disconnect anytime from Settings → Connections. Your automations will pause."

**After first manual run:**  
💡 "Tip: Schedule this to run automatically in Settings."

**Style:**  
Light blue background, dismissible (X), shows once per session, stored in local state.

---

## Appendix: Word Choice

**Use:**
- "Automation" (not "workflow" or "agent")
- "Run" (not "execute" or "trigger")
- "Connect" (not "authenticate" or "authorize")
- "Draft" (not "preview" or "pending")
- "Cost" (not "price" or "spend")
- "Approve" (not "confirm" or "authorize")
- "Plan" (not "spec" or "config")

**Avoid:**
- Technical jargon: "API", "webhook", "OAuth" (explain in help text if needed)
- Corporate speak: "leverage", "utilize", "facilitate"
- Uncertainty: "might", "possibly", "perhaps" (be specific or honest about unknowns)
- False urgency: "Act now!", "Don't miss out!"

---

**Version:** 1.0  
**Last Updated:** 2026-02-10  
**Maintainer:** Design Team

**Usage:** Copy directly into designs. Devs: use exact wording unless UX approves changes.
