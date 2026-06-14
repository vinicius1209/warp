# Cockpit — fork institutional memory

Cockpit is a fork of Warp: a **tab-group-first** terminal that lets non-developer
"vibe coders" run AI-agent development **missions** with a methodology, guard rails,
and a modern UI — not just a chat, not manual config.

Read this before working on mission/UI code. Deep references live in
`.cockpit/discoveries/` (generated from a multi-agent audit, 2026-06).

## The four rules that prevent re-deriving / re-breaking

### 1. Mission binding — key on the STABLE SLUG, never the ephemeral TabGroupId
`ActiveMission.slug` (registry.rs) is stable. `TabGroupId` is **regenerated on every
session restore** (`app/src/persistence/sqlite.rs` `read_app_state` mints fresh ids).
Therefore: durable mission references (gate-pending state, persistence, pane→mission
maps) MUST key on `slug`. Resolve to a live `group_id`/index **at call time** via a
single helper (`assert_mission_by_slug(slug) -> Option<usize>`). **Never** guess with
`find_latest()` — if the active context maps to no mission, no-op with a toast.
This one rule is the root cause of the three original CRITICAL bugs.
→ `.cockpit/discoveries/fork-audit.md`, `cockpit-mission-architecture`

### 2. Warp UI — two footguns
- `MouseStateHandle` must be **long-lived** (stored in the view struct), not created
  per-render, or hover/click silently breaks.
- **Never** acquire a nested `TerminalModel.lock()` — it deadlocks/freezes the UI
  (the macOS beachball). Pass already-locked refs down.
Modal recipe, component catalog, theming, animation → `.cockpit/discoveries/ui-mastery.md`

### 3. CLI agent completion — the real signal
`CLIAgentSessionsModel` emits `CLIAgentSessionStatus::Success` (consumed today by
`AgentNotificationsModel`). That is the completion signal for idle auto-gates.
`IdlePrompt` is **not** completion. `is_agent_supported()` says which harnesses emit
rich OSC-777 status vs command-detection heuristics (reliability varies per harness).
→ `.cockpit/discoveries/idle-detection.md`, `warp-cli-agent-completion-signals`

### 4. opencode engine (Phase 2b) — pin against drift
`opencode serve` REST+SSE, one server per project dir, `/global/health` version probe,
`prompt_async`, `/event` (incl. `session.idle`, `permission.asked`),
`POST /permission/:requestID/reply`. Defenses: pin a version, gate routes on health
version, `#[serde(other)] Unknown` on the event enum, SSE reconnect-with-backoff.
→ `.cockpit/discoveries/opencode-engine.md`

## Mission lifecycle
scaffold (`.cockpit/missions/<slug>/` + manifest.json) → register (`MissionRegistry`)
→ persist (`~/.warp-oss/missions-state.json`) → stage tab (harness CLI seeded with
rendered prompt) → gate (human approval) → advance → complete/abandon.
Templates: harness-agnostic YAML in `~/.warp-oss/missions/` (Spec-Driven, Solo).

## Build/run (this machine)
`export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"` before
cargo (else Homebrew rustc 1.87). `cargo build -p warp --bin warp-oss`. Needs git-lfs,
protobuf, Xcode Metal toolchain (one-time).
