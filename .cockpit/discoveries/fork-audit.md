Excellent! Now I have a comprehensive understanding. Let me create the final audit report.

---

# Cockpit Missions Engineering Audit Report

## 1. STRENGTHS

The Missions system exhibits solid foundational design in several areas:

**1.1 Clean Separation of Concerns**
- `templates.rs` (lines 1-165): Well-encapsulated YAML template loading with builtin fallback mechanism; templates are never overwritten once user-edited, preserving user work.
- `scaffold.rs` (lines 80-130): Stateless mission scaffolding with atomic directory creation and slug uniquification (100-attempt loop ensures no silent reuse).
- `registry.rs` (lines 29-120): Straightforward in-memory registry pattern with proper event emission after mutations.

**1.2 Persistence Strategy**
- `persistence.rs` (lines 73-130): Atomic file writes using `.tmp` rename pattern (line 85-87); stale mission filtering on load (lines 117-126) prevents broken references.
- Round-trip testing (persistence.rs:174-239) validates group ID preservation across save/load cycles.

**1.3 Modal/View Integration**
- `start_mission_modal.rs` (lines 187-228): Proper on_open/on_close lifecycle with validation (lines 263-296); tilde expansion for paths (line 270).
- `mission_control_modal.rs` (lines 114-120): on_open sizing (row_mouse_states resized per mission count) prevents OOB access.
- `gate_dialog.rs` (lines 55-82): Minimal, focused implementation; set_message before rendering prevents null refs.

**1.4 Workspace Integration**
- `workspace/view.rs:7260-7266`: Prompt placeholder rendering isolates mission logic from harness execution details.
- `workspace/view.rs:7356-7365`: resume_mission_stage with conditional harness logic (claude --continue vs re-run) is well-reasoned.
- `workspace/view.rs:7379-7418`: regroup_resumed_mission_tab gracefully handles stale group IDs post-restore with fallback name matching (lines 7393-7409).

---

## 2. FRAGILITIES

### CRITICAL Issues

**2.1 TabGroupId Persistence + Session Restore Mismatch** — `CRITICAL`
- **File**: `persistence.rs:35-40`, `registry.rs:24`, `workspace/view.rs:7379-7418`
- **Problem**: Comment in `persistence.rs:35-37` explicitly acknowledges that session restore mints fresh `TabGroupId`s, invalidating persisted group IDs. The mitigation in `regroup_resumed_mission_tab` (line 7379) is a heuristic fallback:
  - If persisted `group_id` no longer exists (line 7387), it searches for a live group named "Mission: {template_name}" (lines 7392-7398).
  - When zero or multiple matches exist, creates a new group (lines 7402-7409).
  - **Risk**: If a user has two "Spec-Driven" missions resumed in the same session, the second tab could bind to the wrong group or create a duplicate group. No deterministic tie-breaker.

**2.2 Stale Mission Index Assumption** — `CRITICAL`
- **File**: `workspace/view.rs:7017-7034`, `7057-7145`
- **Problem**: `mission_next_stage()` (line 7017) falls back to `find_latest()` (line 7024) when the active tab isn't in a mission group. If:
  - User has multiple active missions from different templates.
  - User switches to a non-mission tab (e.g., settings).
  - User hits the "Mission Next Stage" hotkey.
  - The action advances the *most recently registered* mission, not the one they're viewing.
- **Symptom**: Silent wrong-mission advancement; user expects one mission, advances another.

**2.3 Mission Index Validity Not Validated** — `CRITICAL`
- **File**: `workspace/view.rs:7170-7212`, `7272-7295`, `7301-7372`
- **Problem**: After user interaction (gate dialog confirm, Mission Control "Next Stage", "Resume", "Abandon"), the mission index is assumed valid (e.g., line 7181 `registry.get(mission_index)` can return `None`). If:
  - Mission Control is open when a user removes a mission (line 7287).
  - User clicks a stale "Resume" button from a cached row.
  - Index shifts due to prior removal.
- **Result**: Silent failures logged (e.g., line 7182 `log::error!`) with no user feedback; UI state desynchronizes.

### IMPORTANT Issues

**2.4 Brief.md Read with No Error Propagation** — `IMPORTANT`
- **File**: `workspace/view.rs:7125`, `7246`
- **Problem**: `std::fs::read_to_string(...).unwrap_or_default()` silently returns empty string on IO failure (permission denied, file deleted mid-mission). Downstream:
  - `render_stage_prompt` (scaffold.rs:179-185) substitutes empty `{{briefing}}` into the prompt, corrupting placeholders if briefing was supposed to be there.
  - No error toast or warning; user sees a malformed prompt without knowing why.

**2.5 Mission Gate Dialog State Leak** — `IMPORTANT`
- **File**: `workspace/view.rs:7141-7161`
- **Problem**: 
  - `mission_gate_pending_index` is stored in workspace (line 1041).
  - If gate dialog is cancelled (line 7161), index is cleared.
  - **But**: If the user immediately abandons the mission from Mission Control, the registry mutation fires (line 7287), and the pending index now points to the wrong mission (stale reference in a shifted vec).
  - Next time gate dialog opens, it could advance a different mission.

**2.6 Atomic Update Ordering Issue** — `IMPORTANT`
- **File**: `workspace/view.rs:7187-7210`
- **Problem**: In `advance_mission_stage`:
  - Current stage marked Done (line 7188).
  - Next stage marked Running (line 7193).
  - New tab opened (line 7197).
  - **Registry advanced** *after* tab created (line 7208-7209).
  - **Race**: If observers (e.g., footer mission chips) react to the registry change event before the tab group move completes (line 7202), they see stale tab membership.

**2.7 Singleton Window Assumption** — `IMPORTANT`
- **File**: `workspace/view.rs:6911-6994`, `mission_control_modal.rs:56-318`
- **Problem**: The missions system assumes a single `Workspace` view per app. In a multi-window Warp instance:
  - Starting a mission in Window A opens a tab in Window A's workspace only.
  - Clicking "Mission Control" in Window B opens the modal, but resume/next/abandon actions dispatch to *Window A's workspace* if those methods are not replicated across windows.
  - No window context is passed to mission methods; they operate on whatever workspace they're called on.
  - **Expected**: Multi-window Cockpit should permit resuming/advancing missions from any window.

**2.8 No Shell Quoting for Paths in Prompts** — `IMPORTANT`
- **File**: `workspace/view.rs:7257`, `scaffold.rs:179-185`
- **Problem**: Paths with spaces or special shell characters are directly interpolated into the prompt:
  ```rust
  .replace("{{mission_dir}}", &mission_dir.display().to_string())
  ```
  If `mission_dir = "/Users/vinicius machado/project"`, the prompt becomes:
  ```
  Briefing: ...
  Spec: /Users/vinicius machado/project/.cockpit/missions/spec-driven-20260611-120000/spec.md
  ```
  A shell-aware harness (e.g., `claude` CLI) that naively tokenizes the prompt line may split on the space. **Mitigation exists**: the harness is passed via structured `prompt:` field (not shell string), so it should handle it correctly. **Risk**: Lower harnesses (third-party CLIs) may be vulnerable.

### MINOR Issues

**2.9 No Bounds Check on Current Stage Access** — `MINOR`
- **File**: `mission_control_modal.rs:248-252`
- **Problem**: 
  ```rust
  let stage_name = mission
      .stages
      .get(mission.current_stage)
      .map(|stage| stage.name.as_str())
      .unwrap_or("?");
  ```
  If `current_stage` >= `stages.len()`, shows "?" (acceptable fallback), but silent data inconsistency.

**2.10 Mission Manifest File Format Change Risk** — `MINOR`
- **File**: `persistence.rs:108-113`, `scaffold.rs:132-157`
- **Problem**: Manifest is `serde_json::from_str::<MissionManifest>` (line 148 scaffold.rs). If a future version adds a required field to `MissionManifest`, old persisted missions fail to deserialize with no graceful migration (line 111 logs warning, returns empty).

**2.11 No Concurrent Mission Limit** — `MINOR`
- **File**: `registry.rs:54-59`
- **Problem**: No upper bound on missions vector. User can start 10,000 missions, bloating memory and UI render performance. Mission Control modal renders all missions linearly (mission_control_modal.rs:423-425).

**2.12 TabGroupId Comparison is Equality-Only** — `MINOR`
- **File**: `registry.rs:108-112`
- **Problem**: `find_by_group` uses `rposition` (most recent match wins). If two missions have the same `group_id`, the second one shadows the first. No validation on mission insert that ensures group_id uniqueness.

---

## 3. GAPS vs. Product Vision

The stated vision: *"tab-group-first terminal for vibe coders with workflow+guards+incredible UI across multiple workspaces"*

**3.1 Missing Multi-Workspace Support** — `STRUCTURAL`
- Missions are scoped to a single workspace/window.
- No cross-workspace mission synchronization.
- **Gap**: "multiple workspaces" feature not realized for missions.

**3.2 Workflow Persistence and Checkpointing** — `STRUCTURAL`
- Missions track stage progress but not intermediate artifacts (e.g., generated specs, reviews).
- No checkpoint system to save/replay partial work if a stage fails.
- **Gap**: "workflow" assumes continuous forward progress; no recovery or branching.

**3.3 Guard/Gate Customization** — `PARTIAL`
- Gates exist (gate_dialog.rs) but are read-only confirmations with two buttons (Continue/Cancel).
- No conditional gates (e.g., "approve only if test pass") or multi-agent gates (e.g., requires 2 approvals).
- **Gap**: "guards" are minimal; no workflow orchestration beyond binary approval.

**3.4 Mission Templating Language Limitations** — `STRUCTURAL`
- YAML templates support simple placeholder substitution ({{briefing}}, {{mission_dir}}, {{profile}}).
- No conditional stages, loops, or parameterization beyond user briefing.
- No cross-stage data flow (e.g., passing stage output to next stage's prompt).
- **Gap**: "templated workflows" are static; no dynamic composition.

**3.5 Multi-Project/Monorepo Support** — `MISSING`
- Mission scaffolding assumes single project directory.
- No support for missions spanning multiple Git repos or monorepo subprojects.
- **Gap**: Modern dev workflows use polyrepo/monorepo; missions are locked to one project_dir.

**3.6 Async Agent Stages** — `MISSING`
- All stages block on human gate confirmations.
- No async/parallel stage execution or background mission progression.
- **Gap**: "workflow" suggests automation; human gates are synchronous bottlenecks.

---

## 4. TECH DEBT

**4.1 Duplicated Error Handling Pattern**
- `workspace/view.rs`: Lines 6932-6943, 7182, 7238, 7280, 7322 all follow `mission_data?.or_else(log error; return)` pattern.
- **Cleanup**: Extract `assert_mission_exists(index) -> Option<MissionData>` helper.

**4.2 Inconsistent Logging Severity**
- `persistence.rs:120` logs a warning for stale mission, but `workspace/view.rs:7100, 7182` log errors for invalid indices (same severity conceptually).
- **Cleanup**: Standardize to single convention.

**4.3 Render Helper Duplication**
- `mission_control_modal.rs:127-136` (short_dir), `128-154` (stage_strip) — similar string formatting logic.
- **Cleanup**: Could live in a shared `missions::ui_util` module.

**4.4 Hardcoded Default Harness**
- `workspace/view.rs:6953` hardcodes `"claude".to_string()` for all new missions.
- `templates.rs` MissionStage allows harness override, but no way to set a default-per-template.
- **Cleanup**: Allow `default_harness` in MissionTemplate struct.

**4.5 Gate Dialog Message Setup is Imperative**
- `workspace/view.rs:7138-7140` sets message before opening dialog.
- **Cleanup**: Could be a constructor parameter or event field for clarity.

**4.6 Profile Path Computed Multiple Times**
- `scaffold.rs:21-22`, `184` — computed twice during prompt rendering (once in template, once in render_stage_prompt).
- **Cleanup**: Cache in ActiveMission or pass as param.

---

## 5. TEST COVERAGE

**Tested:**
- `templates.rs:151-163` — builtin template parsing (2 tests).
- `scaffold.rs:215-254` — slugify, render_stage_prompt, path handling (3 tests).
- `persistence.rs:132-240` — save/load round trip, stale mission filtering, error handling (4 tests).
- **Total in missions/**: ~9 unit tests, all in modules (no integration tests).

**Untested (Critical Gaps):**
- `registry.rs` — No tests for register, advance_stage, set_group_id, remove, find_by_group, find_latest.
- `start_mission_modal.rs` — No tests for validation (empty dir, empty briefing, invalid path).
- `mission_control_modal.rs` — No tests for row rendering, button clicks, index out-of-bounds.
- `gate_dialog.rs` — No tests for confirm/cancel paths.
- `workspace/view.rs` mission methods — No integration tests for:
  - start_mission with/without GroupedTabs enabled.
  - mission_next_stage fallback to find_latest.
  - regroup_resumed_mission_tab with stale group_id.
  - advance_mission_stage tab group movement ordering.
  - Concurrent gate dialog + abandon.

**Integration Test Deficit**:
- No end-to-end mission flow tests (scaffold → open → gate → advance → complete).
- No multi-window mission tests.
- No session restore + mission resume tests.

---

## Summary Table

| Category | Severity | Count | Examples |
|----------|----------|-------|----------|
| **CRITICAL** | CRITICAL | 3 | TabGroupId restore mismatch, stale index assumption, index validity unchecked |
| **IMPORTANT** | IMPORTANT | 7 | Brief.md silent failure, gate state leak, atomic ordering, window assumption, shell quoting, bounds check, manifest migration |
| **MINOR** | MINOR | 5 | Concurrent limit, uniqueness validation, equality-only comparison |
| **Test Gaps** | N/A | 10+ | Registry, modals, integration paths untested |

---

## Recommendations (Priority Order)

1. **Validate mission index before every action** (fix 2.3): Add pre-check in mission_next_stage, advance_mission_stage, resume_mission_stage, abandon_mission.

2. **Resolve TabGroupId persistence** (fix 2.1): Implement deterministic tie-breaking (e.g., by mission slug, not just template name) or store mission slug alongside TabGroupId.

3. **Track mission context in gate dialog** (fix 2.5): Store mission slug (immutable) instead of index in mission_gate_pending_index; validate before use.

4. **Document window limitation** (mitigate 2.7): Add comment in start_mission, mission_next_stage methods stating single-window assumption; plan multi-window refactor as separate effort.

5. **Add integration tests** (fix 5): Write end-to-end test suite for mission lifecycle, session restore, and multi-window scenarios.

6. **Handle IO errors for brief.md** (fix 2.4): Return Result from open_mission_stage_tab; emit error toast if brief.md unreadable.

7. **Enforce atomic tab/registry updates** (fix 2.6): Ensure registry.advance_stage fires *after* tab group move completes; or defer registry mutation to after all workspace mutations.

This system is **production-ready for single-workspace, linear workflows** but **fragile for multi-mission, multi-window, or interrupted workflows**. The foundation is solid; the gaps are in edge cases and modern multi-window support.
