I now have all the grounding I need. There's a `ProjectManagementModel` with persisted projects (`all_projects`, `upsert_project`), a `ProjectDataSource` + `SuggestedProjectsDataSource` powering the WelcomePalette, and a `Project` struct — exactly the existing infrastructure a visual project picker would reuse instead of the raw path text field in `start_mission_modal.rs`. I have enough to write the north-star UX doc.

---

# Cockpit Missions — Vibe-Coder UX North Star

**Persona:** a non-developer founder building software with AI agents. Wants a *methodology + guard rails + a modern UI across project workspaces*. Cannot/will-not configure terminals, install CLIs, manage API keys, or read raw shell output. Explicitly does NOT want "just a chat."

**What exists today (grounded in code):**
- Two modals: `app/src/missions/start_mission_modal.rs` (path text field + template dropdown + briefing textarea) and `app/src/missions/mission_control_modal.rs` (text-only mission rows with Resume/Next Stage/Abandon).
- A gate dialog `app/src/missions/gate_dialog.rs` — generic "Advance mission?" with Continue/Cancel.
- Templates as harness-agnostic YAML in `app/src/missions/templates.rs` (Spec-Driven, Solo), pt-BR descriptions.
- Lifecycle in `app/src/workspace/view.rs` (`start_mission`, `open_mission_stage_tab`, `mission_next_stage_for`, `advance_mission_stage`), persisted via `registry.rs`/`persistence.rs`, scaffolded on disk by `scaffold.rs` (`.cockpit/` + an auto-injected Onboarding stage when no `profile.md`).
- Reusable building blocks already in the codebase: `WelcomeView`/`WelcomePalette` (`app/src/pane_group/pane/welcome_view.rs`, `app/src/search/welcome_palette/view.rs`), `ProjectManagementModel` + `Project` (`app/src/projects.rs`), `ProjectDataSource`/`SuggestedProjectsDataSource` (recent projects/git repos), the `Harness` enum with `display_name()`/`config_name()` (`crates/warp_cli/src/agent.rs`), and the auth-secret FTUX flow (`app/src/terminal/view/ambient_agent/auth_secret_ftux_view.rs`, `show_create_auth_secret_modal` in `view.rs`).

---

## 1. JOURNEY GAPS — where the non-dev breaks today

Walk the path: *opens app → wants to build a feature → done.*

**Gap A — Cold start dumps you in a terminal.** First screen is `WelcomeView` (a "New tab"/terminal prompt). A vibe coder has no model for "tab," "session," or "cwd." Missions are reachable only via the `missions` palette pill / `OpenStartMissionModal` action — invisible unless you already know it exists. *There is no "start here" for the methodology.*

**Gap B — Must type a filesystem path.** `start_mission_modal.rs` opens focused on a single-line editor whose placeholder is `~/projetos/meu-projeto` (line 73). Validation is `Path::new(&expanded).is_dir()` → on miss, raw error `Not an existing directory: {expanded}` (line 272). A non-dev does not know absolute paths exist, let alone tilde expansion. **This is the hardest wall in the whole flow** — and it's gratuitous, because `ProjectManagementModel` already holds recent projects and the welcome palette already renders them visually.

**Gap C — No pre-flight on the agent.** `start_mission` hard-codes `default_harness: "claude"` (line 6953), then `open_mission_stage_tab` spawns a `PaneTemplate { harness: Some("claude"), prompt }` (line 7256). Nothing checks that the `claude` binary is installed, on PATH, or logged in. If it isn't, the agent tab just... emits shell errors into a terminal the user can't parse. **The single highest-severity failure mode: silent launch into a broken CLI.** The auth FTUX (`show_create_auth_secret_modal`) exists but is never wired into the mission launch path.

**Gap D — "Which template?" is a bare dropdown.** The methodology choice — the *product's whole value* — is a `Dropdown` of names (line 149) with a one-line gray description below it. "Spec-Driven" vs "Solo" means nothing to someone who's never heard of a spec. No "what will happen," no "how long," no "which is right for me."

**Gap E — The work itself is raw terminal output.** Each stage is a terminal tab running a CLI. The user sees streaming agent/tool chatter. There's no "the Architect is writing your spec…" abstraction — just text. No notion of *what stage I'm in* except the tab title `"{stage.name} — {slug}"`.

**Gap F — Gates are under-informed.** `gate_dialog.rs` shows the template's `gate` string (good, plain-language pt-BR) but **does not show what was produced** — it can't point at `spec.md`/`review.md`, can't preview it, and offers only Continue/Cancel. There is no "ask for changes" or "redo this stage." A non-dev approving a spec they can't see is approving blind.

**Gap G — No recovery when an agent errors.** If a stage fails, nothing detects it. `mission_next_stage_for` only knows "advance" vs "complete"; there's no failed state in `StageStatus` (`Pending|Running|Done` only, `scaffold.rs:42`). The user is stuck with a dead tab and no "retry / get help" affordance.

**Gap H — No completion moment.** When the last stage finishes there's no celebration, no "here's what changed," no "what now?" The mission just disappears from Mission Control on the next action.

**Gap I — Mission Control is a text wall.** `mission_control_modal.rs` renders each mission as 4 stacked text lines + a unicode `✓ ▶ ·` strip (line 139) + three buttons. No progress bar, no live status, no cost. It reads as a debug view, not a cockpit.

---

## 2. PRE-FLIGHT & GUARDS

The mission must not launch until the agent is **proven runnable**. Build a `missions::preflight` module and a `MissionPreflightModal` that runs *before* `start_mission`.

**2.1 Harness install + login detection (highest priority).**
New `app/src/missions/preflight.rs`:
```
enum HarnessReadiness { Ready, NotInstalled, NotLoggedIn, Unknown }
fn check(harness: Harness) -> HarnessReadiness
```
- *Installed?* Reuse `crate::util::path::resolve_executable` (already imported in `harness/mod.rs`) against the harness's binary name. Each `Harness` variant already knows its `config_name()` — add a `binary_name()`/`login_check_cmd()` next to it in `crates/warp_cli/src/agent.rs`.
- *Logged in?* Run the harness's cheap auth probe (e.g. `claude --version` / a whoami-style call) with a short timeout; non-zero / "not authenticated" → `NotLoggedIn`.
- **Guided fix, not an error.** On `NotInstalled`: a card "Claude Code isn't installed yet. Install it for you?" with a one-click action that runs the install in a managed, *captioned* sub-step (not raw terminal). On `NotLoggedIn`: route straight into the existing `show_create_auth_secret_modal(harness, ctx)` / `AuthSecretFtuxView` flow — that machinery already exists, it's just never reached from missions. Block "Start Mission" until green.

**2.2 "Which agent should I use?" — auto-selection.**
Today `default_harness` is hard-coded `"claude"`. Replace with `missions::pick_default_harness()` that returns the *first Ready* harness in a sensible preference order, so the user never picks. Surface it as a single reassuring line in pre-flight: "Your AI builder: **Claude Code** ✓ ready" with a quiet "change" link for power users. Templates stay harness-agnostic (stage `harness: None` already falls through to the mission default in `open_mission_stage_tab`, line 7256) — so auto-selection is a pure default-resolution change, no template churn.

**2.3 Visual project picker (replaces the path field).**
Rip out the `dir_editor` text input in `start_mission_modal.rs`. Replace with a project list built from existing infra:
- A scrollable list of **recent projects** from `ProjectManagementModel::all_projects()` (`app/src/projects.rs:76`) / `SuggestedProjectsDataSource` — same source the `WelcomePalette` already uses, each row showing the folder name + last-two-path-components (the `short_dir` helper already exists in `mission_control_modal.rs:128`) and a git badge.
- A **"+ Add a project folder"** row that opens the existing `ctx.open_file_picker(..., FilePickerConfiguration::new().folders_only())` (already used in `welcome_view.rs:151`), then `upsert_project`.
- Typing a path becomes impossible; you *pick*. The validation/error branch (`start_mission_modal.rs:265-275`) disappears entirely.

**2.4 Methodology picker with plain-language descriptions** → see §4.

**2.5 Confirmation before risky actions.**
- **Abandon** currently fires immediately from a row button (`MissionControlModalEvent::Abandon` → `abandon_mission`). Wrap it in a confirm dialog ("This stops the mission and keeps your files. Abandon?") — reuse the `Dialog` pattern from `gate_dialog.rs`.
- **Autonomy presets in plain language.** The autonomy machinery exists (`settings/onboarding.rs` → `AgentAutonomy::{Full,Partial,None}` → permission sets). Surface it per-mission as three cards: "Ask me before changes" / "Let it work, check at gates" / "Full autonomy" — mapping to the existing `OnboardingAutonomyPermissions`. Default to **Partial** (reads allowed, diffs ask) for non-devs.

---

## 3. INTUITIVE / MODERN UI

**3.1 The FIRST screen should be a Mission Home, not a terminal.**
Replace/augment `WelcomeView` (`app/src/pane_group/pane/welcome_view.rs`) so a vibe-coder's default surface is a **home with one hero action: "Start a Mission."** Below it: "Continue" cards for active missions (live from `MissionRegistry`), a "Recent projects" strip (from `ProjectManagementModel`), and a small "Open a terminal" escape hatch for the rare power user. Terminal-first stays available behind a setting, but it is not the cold-start default for this persona. This directly closes Gap A and gives the methodology a front door.

**3.2 Richer Mission Control** (rewrite `mission_control_modal.rs` rendering):
- **Progress bar per mission** instead of the `✓ ▶ ·` text strip: filled segments = `current_stage / stages.len()`.
- **Stage timeline** — horizontal pills (Onboarding → Architect → Implementer → Reviewer), current one pulsing, done ones checked, with the plain-language stage name. The data is already there (`mission.stages`, `mission.current_stage`).
- **Live agent status** — "Architect is working…" vs "Waiting for you" (at a gate) vs "Needs attention" (errored). Requires a `StageStatus::Failed` + a `Waiting` distinction (see §1 Gap G) and observing the stage tab's agent state.
- **Cost/token meter per mission.** The footer already surfaces CLI-agent usage; aggregate it per `group_id` and show "$0.42 · 12k tokens" on each card. This is the trust signal a budget-conscious founder wants.
- **Primary action is contextual:** one button that reads "Continue" / "Review & approve" / "Retry" depending on stage status — not three equal-weight buttons.

**3.3 Empty states with intent.**
`mission_control_modal.rs` currently renders bare `"No active missions."` (line 415). Replace with an illustrated empty state + a primary "Start your first mission" button (which already exists as `NewMission`). Same for the home screen with no projects: "Point me at a folder and I'll build in it."

**3.4 Workspace switching is tab-group-first.**
Missions already create a named `TabGroup` ("Mission: {template}", `view.rs:6971`). Lean into it: the project/mission *is* the workspace. A left rail or top switcher listing mission groups (one per active mission) lets the user jump between "the auth feature" and "the landing page" without thinking about tabs. `find_by_group` / `set_group_id` already maintain the mapping.

**3.5 Notifications when a stage finishes.**
The agent-notification setting already exists (`show_agent_notifications` in `settings/onboarding.rs`). Wire a notification on the `Running → gate` transition in `mission_next_stage_for`: "✅ The Architect finished your spec — ready for your review." This lets a non-dev walk away and get pulled back at exactly the human-gate moments, which is the entire point of the methodology.

---

## 4. METHODOLOGY AS PRODUCT

Make "follow a methodology" feel like a guided product, not a config choice.

**4.1 Templates as visual cards** (replace the `Dropdown` in `start_mission_modal.rs:149`).
Each `MissionTemplate` renders as a selectable card:
- Title + a **plain-language pitch** ("Best for real features you'll ship — the AI plans first, you approve, then it builds and double-checks itself").
- A **mini stage timeline** rendered from `template.stages` (Architect → Implementer → Reviewer) with a one-liner per stage.
- A **"gates" count** ("2 check-in points") derived from how many stages have a non-null `gate`.
- A "Recommended" badge on Spec-Driven for first-timers; Solo positioned as "quick one-off tasks."
This needs richer template metadata — extend the YAML schema in `templates.rs` with optional `pitch`, `best_for`, and per-stage `plain_summary` fields (all `#[serde(default)]`, so existing templates keep parsing). The `MissionStage` struct already has `name`/`gate`; add `summary: Option<String>`.

**4.2 Per-stage explanations in plain language.**
The agent prompts (`templates.rs`) are technical instructions to the model — never show them. Instead, each stage tab and timeline pill carries the human `summary`: "Right now the **Architect** is reading your project and writing a plan. No code is being changed yet."

**4.3 The gate dialog is the product's signature moment** (rewrite `gate_dialog.rs`).
Today it shows only `message` + Continue/Cancel. Make it an artifact-aware review card:
- **WHAT was produced:** the dialog should receive the stage's output artifact path (e.g. `spec.md`, `review.md` — the templates already write these to `{{mission_dir}}`) and render a **preview** (rendered markdown) inline, with an "Open full file" affordance.
- **Three plain-language choices**, replacing Continue/Cancel:
  - **Approve & continue** → current Confirm path (`advance_mission_stage`).
  - **Ask for changes** → opens a small note field; re-runs the *current* stage with the user's feedback appended to the prompt (new "re-run with feedback" branch off `open_mission_stage_tab`).
  - **Redo this stage** → re-run current stage fresh (resume machinery in `resume_mission_stage` is the seed for this).
- This requires `MissionGateDialog` to carry an artifact reference, not just a string — extend `set_message` to `set_gate(GateContext { message, artifact_path, stage_name })`, and add `MissionGateDialogEvent::{Approve, RequestChanges(String), Redo}`.

**4.4 Make the auto-onboarding stage legible.**
`scaffold.rs` silently injects an "Onboarding" stage on the first mission in a project. Today it's invisible magic. Frame it in the UI: "First, I'll spend a minute learning your project, then we'll start." It already has a gate (`ONBOARDING_GATE`) — show it as the first timeline pill.

---

## 5. PRIORITIZATION (impact-for-non-dev × effort)

| # | Feature | Impact | Effort | Why |
|---|---------|--------|--------|-----|
| 1 | **Harness pre-flight (install + login + auto-select)** | 🔴 Critical | Med | Closes the #1 silent-failure (Gap C). Mostly *wiring existing parts*: `resolve_executable`, the `Harness` enum, and the already-built `AuthSecretFtuxView`. Without this, everything else launches into a broken terminal. |
| 2 | **Visual project picker (replace the path field)** | 🔴 Critical | Low–Med | Closes the hardest wall (Gap B). Reuses `ProjectManagementModel` + `SuggestedProjectsDataSource` + the folders-only file picker already wired in `welcome_view.rs`. Low effort because the data sources exist — it's a swap of `dir_editor` for a list. |
| 3 | **Artifact-aware gate dialog (Approve / Ask for changes / Redo + preview)** | 🔴 High | Med | This *is* the methodology's payoff and the trust mechanism. Turns "approve blind" into "review what was made." Builds on `gate_dialog.rs` + the artifacts templates already write. |

**The 3 highest-leverage wins to build next: (1) pre-flight guards, (2) visual project picker, (3) the artifact-aware gate dialog.** Together they convert the flow from "type a path, hope claude is installed, watch raw output, approve blind" into "pick your project, we make sure the AI is ready, watch plain-language progress, review what was made at each gate."

**Second wave** (high impact, higher effort): Mission Home as first screen (§3.1), richer Mission Control with progress/cost/live-status (§3.2), and stage-finish notifications (§3.5). These need a `StageStatus::Failed`/`Waiting` distinction and per-group cost aggregation, so they're best sequenced after the foundational guards land.

**Quick wins to slot in anytime** (low effort, real polish): abandon-confirmation dialog, template *cards* with plain-language pitches (extend the YAML schema with `#[serde(default)]` fields so nothing breaks), and the illustrated empty states in Mission Control.

**Key files this work lives in:** `app/src/missions/start_mission_modal.rs` (project picker + template cards), new `app/src/missions/preflight.rs` (+ a `MissionPreflightModal`), `app/src/missions/gate_dialog.rs` (artifact-aware gates), `app/src/missions/mission_control_modal.rs` (progress/timeline/cost), `app/src/missions/templates.rs` + `scaffold.rs` (richer schema, legible onboarding), `app/src/missions/registry.rs`/`scaffold.rs` (`StageStatus::Failed`/`Waiting`), `app/src/pane_group/pane/welcome_view.rs` (Mission Home first screen), and the orchestration in `app/src/workspace/view.rs` (`start_mission`, `open_mission_stage_tab`, `mission_next_stage_for` — for pre-flight gating, re-run-with-feedback, and notifications). Reuse `crates/warp_cli/src/agent.rs` (`Harness`), `app/src/projects.rs` (`ProjectManagementModel`), and `app/src/terminal/view/ambient_agent/auth_secret_ftux_view.rs` (login flow).
