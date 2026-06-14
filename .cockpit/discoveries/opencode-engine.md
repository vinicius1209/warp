I have all the facts I need. Both the new endpoint (`POST /permission/:requestID/reply`) and deprecated one (`POST /session/:sessionID/permissions/:permissionID`) are confirmed, with reply values `once`/`always`/`reject`. Now I'll write the architecture plan.

# Phase 2b — Embedding the OpenCode Server as a Native Agent Engine in Cockpit/Warp

## TL;DR / honest complexity read

This is a **medium-large** effort. The good news: Warp already has every primitive you need — an SSE-capable HTTP client (`crates/http_client`, `eventsource()`), a Node runtime + child-process spawner (`crates/node_runtime`, `crates/command`), a mission model with an `engine`/`harness` field already understood by the data layer (`harness: "opencode"` is already a valid value), and a gate-dialog pattern to clone for permission prompts. The hard parts are: (a) a brand-new **process supervisor** with port allocation + lifecycle tied to a mission, (b) a **new pane content type** to render structured assistant/tool-call streams (the existing AI views are coupled to Warp's own MAA/Oz backend, not a generic REST agent), and (c) the **SSE event→UI state machine** (80+ event types, partial-message deltas). None of it is exotic, but it's real surface area. Estimate: ~4-6 focused increments, each independently shippable.

Key version-drift caveat: opencode is moving fast. The permission endpoint just changed — `POST /session/:id/permissions/:permissionID` is **deprecated** in favor of `POST /permission/:requestID/reply`. Pin a version and probe `/global/health` (returns version) on spawn.

---

## Confirmed opencode facts (June 2026)

- **Server**: `opencode serve --port <n> --hostname 127.0.0.1`. Default port `4096`, default host `127.0.0.1`. `OPENCODE_SERVER_PASSWORD` (+ optional `OPENCODE_SERVER_USERNAME`) enables HTTP basic auth; `--cors` for browser origins (not needed for a same-host Rust client). Source: https://opencode.ai/docs/server/
- **One server per project dir** is the operating model — `/event` is project/dir-scoped; `opencode serve` runs in (and is scoped to) its cwd. Multiple clients can attach to one server/dir.
- **REST surface we need**:
  - `GET /global/health` — status + version (health check / pre-flight + version pin).
  - `POST /session` — create session; `GET /session`, `GET /session/:id`, `DELETE /session/:id`.
  - `POST /session/:id/message` (await) or `POST /session/:id/prompt_async` (fire-and-forget, drive via SSE). For our streaming UI we want the **async** form.
  - `POST /session/:id/command` — slash commands.
  - `POST /session/:id/abort` — stop a run.
  - `GET /agent` — list agents; `GET /config`, `GET /provider` — capability/auth introspection.
  - **Permission reply**: new `POST /permission/:requestID/reply` with body `{ "reply": "once" | "always" | "reject" }` (the old `POST /session/:id/permissions/:permissionID` is deprecated). Sources: https://opencode.ai/docs/permissions/ , https://deepwiki.com/anomalyco/opencode/6.2-permission-system , https://github.com/anomalyco/opencode/issues/15386
  - `GET /doc` — OpenAPI 3.1, the authoritative schema to codegen/validate against.
- **SSE `/event` stream** (the heart of the UI). Relevant types: `server.connected` (first), `session.created/updated`, `session.idle` (**agent finished → our auto-gate trigger**), `session.error`, `message.updated`, `message.part.updated` / `message.part.delta` (**assistant token + tool-call streaming**), `permission.updated` / `permission.asked` (**approval gate trigger**). The SDK's unified `Event` type covers 80+ subtypes; we only switch on a handful. Sources: https://opencode.ai/docs/server/ , https://deepwiki.com/sst/opencode/2.8-storage-and-migration-system , issue on dir-scoped attach https://github.com/anomalyco/opencode/issues/11522
- **SDK is JS/TS only** (`@opencode-ai/sdk`: `createOpencodeClient({baseUrl})`, `session.prompt`, `session.command`, `event.subscribe`, permission reply). **No Rust SDK** → Warp talks raw REST+SSE, which `http_client` already supports natively. Source: https://opencode.ai/docs/sdk/

---

## 1. Process management — spawn & supervise `opencode serve`

### What Warp already gives you
- **`crates/command`** (`src/async.rs`) — the async `Command` wrapper Warp uses everywhere to spawn children (used by `node_runtime` at `crates/node_runtime/src/lib.rs:253,412,458,466`). Cross-platform, handles Windows `cmd.exe /c` PATH quirks. This is your spawn primitive, not raw `std::process`.
- **`crates/node_runtime`** — gives you a vendored Node + npm (`node-v22.12.0`) under `warp_core::paths::data_dir()/node/...`, plus `find_working_node_binary()` (`lib.rs:406`) and `fetch_npm_package_version()` (`lib.rs:506`). opencode ships as an npm package and also as standalone binaries; for a "just works for non-devs" story you can `npm i -g`/`npx` opencode via this vendored Node, or detect a system `opencode` on PATH first.
- **`warp_cli::process_handle`** (`crates/warp_cli/src/process_handle.rs`) — Warp already has a child-process-handle abstraction for harness CLIs; reuse/extend rather than inventing.

### Proposed design: an `OpenCodeServerManager` singleton model
Create `app/src/ai/opencode/server_manager.rs` (new module under the existing `app/src/ai/` tree, sibling to `harness_availability.rs`). Make it a `SingletonEntity` model (same pattern as `MissionRegistry` at `app/src/missions/registry.rs:115-119` and `HarnessAvailability`).

Responsibilities:
- **Keyed pool**: `HashMap<PathBuf /*project_dir*/, ServerHandle>`. **One server per project dir** (matches opencode's constraint and its dir-scoped `/event`). Multiple missions/sessions in the same project **share** one server — do not spawn per-mission. Ref-count sessions so the server lives while any mission/pane in that dir is active.
- **Port allocation**: bind a `TcpListener` on `127.0.0.1:0`, read the assigned port, drop the listener, pass it to `--port` (classic race-tolerant ephemeral allocation). Avoids colliding with a user's own `opencode` on 4096. Store `(pid, port, child handle)`.
- **Spawn**: `opencode serve --hostname 127.0.0.1 --port <ephemeral>` with `cwd = project_dir`, via `command::async::Command`, in `run_in_background`-style detached supervision. Set `OPENCODE_SERVER_PASSWORD` to a random per-process token and send it via `basic_auth` from the client (defense-in-depth on a shared machine — even on loopback).
- **Health gate**: poll `GET /global/health` until 200 (timeout ~10s); capture and pin the returned version. Only then mark the handle `Ready` and let sessions attach.
- **Lifecycle / cleanup**: tie ref-count to mission/pane lifetime. When the last session in a dir closes (pane dropped / mission abandoned — see `abandon_mission` at `app/src/workspace/view.rs:7272`), `DELETE` sessions, then kill the child (graceful `SIGTERM`, then `SIGKILL`). On app quit, iterate the pool and kill all. Crash supervision: if the child exits unexpectedly while sessions are live, emit a `ServerManagerEvent::Crashed{dir}` so the pane can show "engine stopped — restart?".

Effort: **M**. Risk: child-process lifecycle on Windows (process groups / orphan kill) is the usual sharp edge — `command` crate already abstracts some of it.

---

## 2. Client — Rust HTTP + SSE client to the opencode server

### `http_client` already does SSE
Confirmed at `crates/http_client/src/lib.rs:453` — `RequestBuilder::eventsource()` returns an `EventSourceStream` (`futures::stream::BoxStream` of `reqwest_eventsource::Event`), wired through `reqwest_eventsource` with Tokio-compat. So **GET /event SSE needs zero new infra** — just `client.get(url).eventsource()` and match on `Event::Message`. JSON POST/GET, `basic_auth` (`lib.rs:522`), `bearer_auth`, and `timeout` are all there.

### Minimal Rust client surface
New module `app/src/ai/opencode/client.rs` — a thin typed wrapper over an `Arc<http_client::Client>` + `base_url` + auth token:
- `health() -> HealthInfo` (version pin / readiness).
- `create_session() -> SessionId`.
- `prompt_async(session, text, agent?)` → fire prompt, drive results via SSE (use `prompt_async`, not the blocking `message`).
- `command(session, name, args)` → slash command.
- `abort(session)`.
- `reply_permission(request_id, Reply::{Once|Always|Reject})` → `POST /permission/:requestID/reply` (with a fallback to the deprecated `POST /session/:id/permissions/:permissionID` if the pinned server version predates the new route — gate on version).
- `subscribe_events() -> impl Stream<Item = OpenCodeEvent>` over `GET /event`, deserializing into a Rust enum that mirrors only the subtypes we handle (`session.idle`, `session.error`, `message.updated`, `message.part.updated`, `permission.updated`/`permission.asked`) with a `#[serde(other)] Unknown` catch-all — same forward-compat trick `Harness::Unknown` uses (`crates/warp_cli/src/agent.rs:155`). This is the single most important robustness decision against version drift: **never hard-fail on an unknown event subtype.**

Generate the request/response structs from `/doc` (OpenAPI 3.1) but hand-trim to the subset — full codegen of an 80+-event schema is overkill and a drift magnet.

Effort: **S-M**. Risk: SSE reconnect semantics (issue #26697 reports a stream that closes right after `server.connected` in some versions) — implement reconnect-with-backoff and resync session state via `GET /session/:id/message` on reconnect.

---

## 3. UI — rendering the conversation in a Warp pane

### Build a new pane content type; do NOT reuse the Oz/AI views
The existing `app/src/ai/` and `app/src/ai_assistant/` views (`panel.rs`, `transcript.rs`) are coupled to Warp's own MAA/Oz agent backend and its GraphQL/agent-event model — bending them to a generic REST agent is more work than a focused new view and risks regressions in shipping surfaces. The mission stage path today spawns a **PTY tab** via `PaneTemplateType::PaneTemplate { pane_mode: PaneMode::Terminal, harness: Some(...) }` (`app/src/workspace/view.rs:7250-7259`). For the engine path we instead want a structured (non-PTY) pane.

Recommendation:
- Add a new pane mode / content variant alongside `PaneMode::Terminal` (the `PaneMode` enum referenced at `app/src/workspace/view.rs:7254`; the pane template type lives in `app/src/workspace/action.rs`) — e.g. `PaneMode::AgentEngine` backed by an `OpenCodePaneView`.
- `OpenCodePaneView` holds: the `client`, the `session_id`, an ordered `Vec<MessagePart>` (assistant text, tool-call cards, results), and a status line. It subscribes to the manager's event stream (filtered to its `session_id`) and applies `message.part.updated` deltas to the transcript. Render with existing `warpui` elements + `ui_components` (the gate dialog at `app/src/missions/gate_dialog.rs` shows the element/builder idioms: `Container`, `Stack`, `Dialog`, `appearance.ui_builder()`).
- **Lighter MVP**: P2b.2 can render into a read-only markdown/text element (reuse `crates/markdown_parser`) before building rich tool-call cards in P2b.3.

### Permission dialog: clone the gate dialog pattern
`permission.asked`/`permission.updated` → present a native modal. **Clone `MissionGateDialog`** (`app/src/missions/gate_dialog.rs`) into `OpenCodePermissionDialog`:
- Title = the permission (e.g. "Run command: `rm -rf build/`"), body = the suggested pattern/diff.
- Three buttons instead of two: **Allow once** → `reply: "once"`, **Always allow** → `reply: "always"`, **Reject** → `reply: "reject"` (the dialog today has Continue/Cancel at lines 113-131; add a third `ButtonVariant`). Keybindings registered like lines 27-40.
- On click → `client.reply_permission(request_id, …)`. Backdrop-blur + centered `Dialog` is already done for you (lines 159-164).

Effort: **L** (the rich transcript view is the bulk). Permission dialog itself: **S** (clone + 1 button + wire).

---

## 4. Mission integration — `engine: "opencode"` drives a session instead of a PTY

### The data model is already 90% there
- `MissionStage.harness: Option<String>` already documents `"opencode"` as a valid value (`app/src/missions/templates.rs:28-30`), and `Harness::OpenCode` round-trips through config/persistence (`crates/warp_cli/src/agent.rs:140,198,216`; `app/src/missions/persistence.rs:151`). The Spec-Driven template's harness field is the natural switch.
- The branch point is **`open_mission_stage_tab`** (`app/src/workspace/view.rs:7217-7267`). Today it unconditionally builds a `PaneMode::Terminal` PTY template. Add a fork:
  - If the resolved harness is `opencode` **and** the engine feature flag is on → call a new `open_mission_stage_engine_pane(...)`: ensure a server for `project_dir` via `OpenCodeServerManager`, create a session, open an `OpenCodePaneView` (the new pane content type), and send the rendered stage prompt (`render_stage_prompt`, line 7248) as the **first** `prompt_async`.
  - Else → existing PTY path, unchanged. **Both paths coexist**; this is purely additive and flag-gated.
- **`session.idle` → auto-gate**: when the engine pane receives `session.idle` for the stage's session, fire the same gate flow the PTY path uses. The gate machinery (`MissionGateDialog`, gate text from `render_stage_prompt(gate, …)` at `app/src/workspace/view.rs:7128`) is reused verbatim — engine integration just *triggers it automatically* on `session.idle` instead of waiting for the user to manually decide the CLI is done. This is the headline UX win: **no more guessing when the agent finished.**
- **Resume** (`resume_mission_stage`, `app/src/workspace/view.rs:7301`): the engine path resumes by re-attaching to the existing opencode session (`GET /session/:id`) rather than `claude --continue`. Cleaner than the CLI continue-flag hack at lines 7326-7341.

### Provider/auth (the user's subscriptions as backend)
opencode handles provider auth headless (`opencode auth login` writes `~/.local/share/opencode/auth.json`): ChatGPT plan, Copilot, OpenCode Zen, Ollama; Claude currently API-key only. Surface auth state via `GET /provider` + `GET /config` and reuse the existing **harness auth FTUX** the codebase already has (`app/src/ai/local_harness_setup.rs`, `auth_secret_types.rs`, and `show_create_auth_secret_modal` at `app/src/workspace/view.rs:15277`) so non-devs get a guided "sign in to your model" flow rather than a terminal command.

Effort: **M** (mostly wiring; the data model and gate flow already exist).

---

## 5. Phasing — shippable increments

| Phase | Deliverable | Depends on | Effort | Risk |
|---|---|---|---|---|
| **P2b.0 Pre-flight** | Detect `opencode` (PATH or vendored-Node `npx`); `GET /global/health` version probe; surface "not installed / version too old" with an install CTA. Reuse `harness_availability` pattern (`app/src/ai/harness_availability.rs`). | — | **S** | Low |
| **P2b.1 Spawn + supervise** | `OpenCodeServerManager` singleton: ephemeral port, spawn via `command::async`, health-gate, ref-counted per-dir pool, kill on quit/abandon. No UI yet — assert via integration test hitting `/global/health`. | P2b.0 | **M** | Child lifecycle on Windows |
| **P2b.2 Minimal session over REST** | `client.rs` wrapper; create session, send one prompt (`message` blocking is fine here), render the final assistant text in a bare new pane (markdown). Proves end-to-end plumbing. | P2b.1 | **M** | Low |
| **P2b.3 SSE streaming** | Switch to `prompt_async` + `subscribe_events()`; live-render `message.part.updated` deltas and tool-call cards; reconnect-with-backoff + resync. The "real streaming in Warp's UI" milestone. | P2b.2 | **L** | SSE reconnect/version drift |
| **P2b.4 Permission dialog** | `permission.asked` → native `OpenCodePermissionDialog` (clone of `gate_dialog.rs`) with Once/Always/Reject → `POST /permission/:id/reply`. | P2b.3 | **S-M** | Endpoint-version fork |
| **P2b.5 Mission integration** | Fork `open_mission_stage_tab` on `harness=="opencode"`; first stage prompt → session; `session.idle` → auto-gate; engine-aware resume; auth FTUX. Flag-gated; PTY path untouched. | P2b.3 (P2b.4 ideally) | **M** | Coexistence regressions |

Each phase is independently demoable. P2b.2 is the first "wow" (agent reply in a Warp pane, no PTY). P2b.3 + P2b.4 + P2b.5 together deliver the full "chat with guards and incredible UI" vision.

---

## 6. Risks & honest caveats

- **Version drift (highest).** opencode changes routes fast — permission reply already migrated to `POST /permission/:requestID/reply`; SSE has open bugs (streams closing early: #26697; dir-scoped attach not emitting: #11522). **Mitigations**: pin a known-good version, gate route choice on the `/global/health` version, generate types from that version's `/doc`, and use `#[serde(other)] Unknown` on the event enum so new subtypes never crash the pane.
- **opencode not installed / wrong Node.** Pre-flight (P2b.0) is mandatory. Offer vendored-Node `npx opencode` as the zero-setup fallback for non-devs; detect system install first.
- **One-server-per-dir constraint.** Enforce by keying the pool on `project_dir` and ref-counting sessions. Two missions in the same repo share a server — don't regress into per-mission servers (port exhaustion, duplicate file watchers, `/event` cross-talk).
- **Auth friction for non-devs (Claude gap).** Claude is API-key-only in opencode today, which clashes with "use my Claude subscription." For Claude-backed stages the PTY `claude` harness may stay the better path; opencode shines for ChatGPT-plan / Copilot / Zen / Ollama users. Be explicit in the model picker about which backends are subscription-driven vs key-driven.
- **Loopback security.** Even on `127.0.0.1`, set `OPENCODE_SERVER_PASSWORD` to a per-process random token and send `basic_auth` — on a shared/multi-user machine an unauthenticated agent server that can run shell is a liability.
- **UI scope creep.** The rich transcript (tool-call cards, diffs) is the single biggest time sink. Ship P2b.2/P2b.3 with a plain markdown transcript first; iterate on card fidelity.

---

### Key Warp-side integration points (file:line)
- Branch the stage launcher: `app/src/workspace/view.rs:7217` (`open_mission_stage_tab`), PTY template built at `:7250-7259`; gate text rendered at `:7128`; resume at `:7301`.
- Pane mode enum referenced at `app/src/workspace/view.rs:7254`; pane template type in `app/src/workspace/action.rs` (`InitContent` at `:50`, harness arg at `:618`).
- Clone for permission UI: `app/src/missions/gate_dialog.rs` (whole file; buttons `:113-131`, keybindings `:27-40`, backdrop `:159-164`).
- Singleton-model pattern to copy: `app/src/missions/registry.rs:115-119`; events/observers idiom throughout.
- SSE client (already SSE-capable): `crates/http_client/src/lib.rs:453` (`eventsource()`), `:522` (`basic_auth`).
- Process spawn + Node: `crates/command/src/async.rs`; `crates/node_runtime/src/lib.rs:406` (`find_working_node_binary`), `:506` (`fetch_npm_package_version`).
- Harness already models OpenCode: `crates/warp_cli/src/agent.rs:140,194-222`; `app/src/missions/templates.rs:28-30`; `app/src/missions/persistence.rs:151`; child-process handle abstraction at `crates/warp_cli/src/process_handle.rs`.
- Auth FTUX to reuse: `app/src/ai/local_harness_setup.rs`, `app/src/ai/auth_secret_types.rs`, `app/src/workspace/view.rs:15277` (`show_create_auth_secret_modal`); pre-flight pattern in `app/src/ai/harness_availability.rs`.
- New code lives under: `app/src/ai/opencode/` — `server_manager.rs`, `client.rs`, `pane_view.rs`, `permission_dialog.rs`.

Sources: https://opencode.ai/docs/server/ , https://opencode.ai/docs/sdk/ , https://opencode.ai/docs/permissions/ , https://deepwiki.com/anomalyco/opencode/6.2-permission-system , https://github.com/anomalyco/opencode/issues/15386 , https://github.com/anomalyco/opencode/issues/26697 , https://github.com/anomalyco/opencode/issues/11522
