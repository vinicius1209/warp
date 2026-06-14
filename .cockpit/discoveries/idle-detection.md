Perfect! I now have all the information needed to produce a comprehensive report.

---

# SCOPE PHASE 2A: AUTOMATIC MISSION GATES VIA AGENT IDLE/COMPLETION DETECTION

## EXECUTIVE SUMMARY

The Warp codebase has **production-ready completion detection machinery** via `CLIAgentSessionsModel`. The signal already exists: when a CLI agent in a terminal pane reaches `CLIAgentSessionStatus::Success`, a `StatusChanged` event fires. This event is **already subscribed by `AgentNotificationsModel`** to show user-facing completion notifications.

**The mission gate system is currently 100% manual:** a user must click "Next Stage" in Mission Control to advance. **Goal:** Auto-trigger the gate dialog (or auto-advance) when a pane's agent session completes.

**Reliability is already battle-tested:** most first-party harnesses (Claude, OpenCode, Gemini) emit structured OSC 777 plugin events; legacy Codex uses OSC 9 fallback. Unsupported agents (Agy, Amp, Hermes, etc.) fall back to command-detection-only, which does NOT set `received_rich_notification`, so they emit no rich status—we can explicitly exclude them.

---

## 1. COMPLETION DETECTION MACHINERY: CLIAgentSessionsModel

**File:** `/Users/viniciusmachado/projetos/warp/app/src/terminal/cli_agent_sessions/mod.rs:15–241`

### Status Enum
```rust
// Line 16–21
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CLIAgentSessionStatus {
    InProgress,
    Success,
    Blocked { message: Option<String> },
}
```

### Status Transition Logic
Status flips via `apply_event()` (line 177–241):

| Event Type | New Status | Semantics |
|---|---|---|
| `PromptSubmit` | `InProgress` | Agent starts work |
| `ToolComplete` | `InProgress` | Permission unblocked; resume work |
| `Stop` | **`Success`** | **Agent finished (terminal state)** |
| `PermissionRequest` | `Blocked { message }` | User action required |
| `QuestionAsked` | `Blocked { message }` | User input needed |
| `PermissionReplied` | `InProgress` | User replied; resume |
| `IdlePrompt` | *(no change)* | At prompt, waiting for input—**not a completion signal** |
| `SessionStart` | *(no change)* | Session opened; context capture only |

**Key insight (line 229–231):** `IdlePrompt` explicitly does NOT change status:
```rust
CLIAgentEventType::IdlePrompt => return None,  // Skip!
```

This is **correct design:** Codex's OSC 9 notifications emit `IdlePrompt` when the shell prompt appears (raw text parsing ambiguity), but Warp still waits for a structured `Stop` event to declare success. **Lesson:** don't fire gates on intermediate idles—only on **`Success` → terminal state**.

---

## 2. EVENTS: CLIAgentSessionsModelEvent

**File:** `/Users/viniciusmachado/projetos/warp/app/src/terminal/cli_agent_sessions/mod.rs:244–299`

```rust
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum CLIAgentSessionsModelEvent {
    Started { terminal_view_id, agent },
    StatusChanged {
        terminal_view_id,
        agent,
        status: CLIAgentSessionStatus,
        session_context: Box<CLIAgentSessionContext>,
    },
    InputSessionChanged { terminal_view_id, agent, previous_input_state, new_input_state },
    Ended { terminal_view_id, agent },
    SessionUpdated { terminal_view_id, agent },
}
```

**Emission point (line 426–434):**
```rust
pub fn update_from_event(&mut self, terminal_view_id: EntityId, event: &CLIAgentEvent, ctx: &mut ModelContext<Self>) {
    // ...
    if let Some(new_status) = session.apply_event(event) {
        let agent = session.agent;
        ctx.emit(CLIAgentSessionsModelEvent::StatusChanged {
            terminal_view_id,
            agent,
            status: new_status,
            session_context: Box::new(session.session_context.clone()),
        });
    }
}
```

**Subscriber pattern example:** `AgentNotificationsModel` (line 49–52):
```rust
let cli_sessions_model = CLIAgentSessionsModel::handle(ctx);
ctx.subscribe_to_model(&cli_sessions_model, |me, event, ctx| {
    me.handle_cli_agent_session_event(event, ctx);
});
```

**For missions, we need a similar subscriber in the Workspace to:**
- Match `terminal_view_id` to a mission's current-stage pane
- On `StatusChanged { status: Success, .. }`, trigger auto-gate logic
- Pass metadata (mission_index, pane_id) to the gate handler

---

## 3. PLUGIN DEPENDENCY & RELIABILITY

**Files:**
- `/Users/viniciusmachado/projetos/warp/app/src/terminal/cli_agent_sessions/listener/mod.rs`
- `/Users/viniciusmachado/projetos/warp/app/src/terminal/cli_agent_sessions/plugin_manager/mod.rs`

### Supported Agents (Rich Status Available)

| Agent | Plugin Status | Event Source | Notes |
|---|---|---|---|
| Claude | First-party | OSC 777 structured | ✓ Tested |
| OpenCode | First-party | OSC 777 structured | ✓ Tested |
| Gemini | First-party | OSC 777 structured | ✓ Tested |
| Codex | Hybrid | OSC 777 + OSC 9 fallback | ✓ Tested; falls back if plugin absent |
| Auggie, Pi | Community-maintained | OSC 777 structured | ✓ No install flow, but listen works |

**Listener (line 39–49):**
```rust
pub fn is_agent_supported(agent: &CLIAgent) -> bool {
    matches!(
        agent,
        CLIAgent::Claude
            | CLIAgent::OpenCode
            | CLIAgent::Codex
            | CLIAgent::Gemini
            | CLIAgent::Auggie
            | CLIAgent::Pi
    )
}
```

### Unsupported Agents (Command Detection Only)

Hermes, Agy, Amp, Droid, Copilot, CursorCli, Goose, Vibe, Unknown have **no listener** → emit no plugin events → **never set `received_rich_notification`** → status remains unreliable.

**Solution:** On mission-start, **validate the current pane's agent is in the "supported" list**. If unsupported, show a warning or fall back to manual gate.

---

## 4. EXISTING NOTIFICATIONS: AgentNotificationsModel

**File:** `/Users/viniciusmachado/projetos/warp/app/src/ai/agent_management/agent_management_model.rs:125–207`

This model **already demonstrates the proven signal path** we should reuse:

```rust
fn handle_cli_agent_session_event(
    &mut self,
    event: &CLIAgentSessionsModelEvent,
    ctx: &mut ModelContext<Self>,
) {
    match event {
        CLIAgentSessionsModelEvent::StatusChanged {
            terminal_view_id,
            agent,
            status,
            session_context,
        } => match status {
            // Line 159–181: Notification on Success
            CLIAgentSessionStatus::Success => {
                let title = session_context.display_title().unwrap_or_else(|| 
                    format!("{} completed", agent.display_name())
                );
                let message = "Task completed.";
                self.add_notification(title, message.to_owned(), NotificationCategory::Complete, ...);
            }
            CLIAgentSessionStatus::Blocked { message } => {
                let title = session_context.display_title().unwrap_or_else(|| 
                    format!("{} needs attention", agent.display_name())
                );
                self.add_notification(title, message.clone().unwrap_or_else(...), NotificationCategory::Request, ...);
            }
            _ => {}
        }
    }
}
```

**Gate logic should mirror this pattern:** subscribe to `CLIAgentSessionsModel`, filter by `StatusChanged { status: Success }`, then check if the `terminal_view_id` maps to an active mission's current-stage pane.

---

## 5. WORKSPACE MISSION INTEGRATION

**File:** `/Users/viniciusmachado/projetos/warp/app/src/workspace/view.rs`

### Current Manual Gate Flow

**Line 3090:** Workspace subscribes to `CLIAgentSessionsModel`:
```rust
let cli_sessions_model = CLIAgentSessionsModel::handle(ctx);
ctx.subscribe_to_model(&cli_sessions_model, |me, event, ctx| {
    me.handle_cli_agent_sessions_event(event, ctx);
});
```

**Line 3643–3658:** Handler just notifies on any event (no mission logic):
```rust
fn handle_cli_agent_sessions_event(
    &mut self,
    event: &CLIAgentSessionsModelEvent,
    ctx: &mut ViewContext<Self>,
) {
    if matches!(event, CLIAgentSessionsModelEvent::Started { .. } | ...) 
        && self.workspace_contains_terminal_view(event.terminal_view_id(), ctx)
    {
        ctx.notify();  // ← Trigger UI re-render only
    }
}
```

### Current Manual Gate Trigger

**Line 6869–6910:** Workspace handles `MissionControlModalEvent::NextStage`:
```rust
fn handle_mission_control_modal_body_event(
    &mut self,
    event: &MissionControlModalEvent,
    ctx: &mut ViewContext<Self>,
) {
    match event {
        MissionControlModalEvent::NextStage { mission_index } => {
            self.mission_next_stage_for(*mission_index, ctx);
        }
        // ...
    }
}
```

**Line 7057–7145:** `mission_next_stage_for()` is the **gate entrypoint**:
- Reads mission from registry
- Checks if current stage is last (complete mission) or show gate
- **Line 7117–7130:** If `stage.gate.is_some()`, renders gate text and calls `set_message()`
- **Line 7131:** Sets `mission_gate_pending_index = Some(mission_index)` and opens dialog

**Line 7147–7168:** `handle_mission_gate_dialog_event()` processes Confirm/Cancel:
```rust
MissionGateDialogEvent::Confirm => {
    if let Some(mission_index) = self.mission_gate_pending_index.take() {
        self.advance_mission_stage(mission_index, ctx);  // ← Actual advance
    }
}
```

**Line 7170–7215:** `advance_mission_stage()` updates manifest and registry.

---

## 6. MISSION TEMPLATES & SCHEMA

**File:** `/Users/viniciusmachado/projetos/warp/app/src/missions/templates.rs:24–35`

Current schema:
```rust
pub struct MissionStage {
    pub name: String,
    #[serde(default)]
    pub harness: Option<String>,  // "claude" | "opencode" | "codex" | "agy"
    pub prompt: String,
    #[serde(default)]
    pub gate: Option<String>,  // Gate confirmation text (null = default message)
}
```

**New field to add:**
```rust
#[serde(default)]
pub auto_advance: Option<bool>,  // Auto-advance when stage completes (gate required)
```

Example in builtin templates (line 45–79):
```yaml
stages:
  - name: "Arquitecto"
    prompt: "..."
    gate: "Spec text"
    # auto_advance: null  (default: false / manual)
  - name: "Implementador"
    prompt: "..."
    gate: "Implementation review text"
    auto_advance: true  # ← NEW: skip manual dialog, auto-advance
```

---

## 7. IMPLEMENTATION PLAN

### Files to Modify

| File | Change | Purpose |
|---|---|---|
| `app/src/missions/templates.rs:26` | Add `auto_advance: Option<bool>` field | Schema support |
| `app/src/workspace/view.rs:3643–3658` | Expand `handle_cli_agent_sessions_event()` | Detect agent completion |
| `app/src/workspace/view.rs:3090` | (No change) | Already subscribed |
| (NEW) `app/src/workspace/view.rs:~7150` | Add `detect_mission_completion_for_pane()` | Pane → mission mapping + gate logic |
| (NEW) `app/src/workspace/view.rs:~6900` | Add `auto_trigger_mission_gate()` | Dialog or auto-advance |
| `app/src/missions/registry.rs:61–72` | (No change) | `get()` and `advance_stage()` unchanged |
| `app/src/missions/scaffold.rs:158` | (No change) | Manifest already tracks stages |

### Core Logic

**Step 1: Pane-to-Mission Mapping**

When workspace receives `CLIAgentSessionsModelEvent::StatusChanged { terminal_view_id, status: Success }`:

1. Iterate active missions in `MissionRegistry`
2. For each mission, find the current stage's tab group
3. For that tab group, find all panes in all tabs
4. Check if `terminal_view_id` matches any pane → **Found mission**
5. Verify the pane's agent is in the "supported" list (Claude, OpenCode, Gemini, Codex)

**Code sketch:**
```rust
fn detect_mission_completion_for_pane(
    &self,
    terminal_view_id: EntityId,
    ctx: &AppContext,
) -> Option<usize> {
    let registry = MissionRegistry::as_ref(ctx);
    for (mission_index, mission) in registry.missions().iter().enumerate() {
        let group_id = mission.group_id?;
        // Find tab hosting this group
        if let Some(tab) = self.tabs.iter().find(|t| t.group_id == Some(group_id)) {
            let pane_group = tab.pane_group.as_ref(ctx);
            if pane_group.contains_terminal_view(terminal_view_id, ctx) {
                return Some(mission_index);
            }
        }
    }
    None
}
```

**Step 2: Gate or Auto-Advance**

Once mission is mapped:

```rust
fn auto_trigger_mission_gate(
    &mut self,
    mission_index: usize,
    ctx: &mut ViewContext<Self>,
) {
    let registry = MissionRegistry::as_ref(ctx);
    let mission = match registry.get(mission_index) {
        Some(m) => m,
        None => return,
    };
    
    let stage = match mission.stages.get(mission.current_stage) {
        Some(s) => s,
        None => return,
    };
    
    // Check if stage is last
    if mission.current_stage + 1 >= mission.stages.len() {
        // Complete mission (same as manual gate confirm on last stage)
        // ... (existing code from mission_next_stage_for lines 7118-7130)
        return;
    }
    
    // Not last stage: check auto_advance flag
    if stage.auto_advance.unwrap_or(false) {
        // Auto-advance: skip dialog, call advance_mission_stage directly
        self.advance_mission_stage(mission_index, ctx);
    } else {
        // Manual gate: show dialog (same as current flow)
        self.mission_next_stage_for(Some(mission_index), ctx);
    }
}
```

**Step 3: Check Agent Support**

Before triggering, verify the pane's agent is supported:

```rust
let cli_sessions_model = CLIAgentSessionsModel::as_ref(ctx);
if let Some(session) = cli_sessions_model.session(terminal_view_id) {
    let agent = session.agent;
    if !listener::is_agent_supported(&agent) {
        // Unsupported agent: skip auto-gate
        return;
    }
}
```

**Step 4: Debounce / Idempotency**

Track `last_auto_gate_terminal_id: Option<EntityId>` to ensure one pane completion fires once, even if events re-emit:

```rust
if self.last_auto_gate_terminal_id == Some(terminal_view_id) {
    return;  // Already processed this completion
}
self.last_auto_gate_terminal_id = Some(terminal_view_id);
```

---

## 8. DETAILED FILE MODIFICATIONS

### A. `app/src/missions/templates.rs`

**Add field to `MissionStage` (line 26):**
```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MissionStage {
    pub name: String,
    #[serde(default)]
    pub harness: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub gate: Option<String>,
    #[serde(default)]  // NEW
    pub auto_advance: Option<bool>,  // Defaults to false if absent
}
```

**No changes to SPEC_DRIVEN_TEMPLATE or SOLO_TEMPLATE needed** (users can edit YAML directly).

### B. `app/src/workspace/view.rs` — Enhanced Event Handler

**Replace line 3643–3658:**

```rust
fn handle_cli_agent_sessions_event(
    &mut self,
    event: &CLIAgentSessionsModelEvent,
    ctx: &mut ViewContext<Self>,
) {
    let terminal_view_id = event.terminal_view_id();
    
    if !self.workspace_contains_terminal_view(terminal_view_id, ctx) {
        return;
    }
    
    ctx.notify();
    
    // PHASE 2A: Auto-gate on agent completion
    if let CLIAgentSessionsModelEvent::StatusChanged {
        terminal_view_id,
        status: CLIAgentSessionStatus::Success,
        ..
    } = event
    {
        self.try_auto_trigger_mission_gate(*terminal_view_id, ctx);
    }
}
```

### C. `app/src/workspace/view.rs` — New Auto-Gate Methods

**Add to Workspace struct (around line 2000):**
```rust
/// Last terminal view ID that triggered an auto-gate, to debounce re-emits.
last_auto_gate_terminal_id: Option<EntityId>,
```

**Initialize in `new()` method:**
```rust
last_auto_gate_terminal_id: None,
```

**Add methods (after line 7215):**

```rust
/// Attempts to auto-trigger a mission gate when an agent session completes.
/// Maps the terminal view to a mission's current stage, checks support, and either:
/// - Auto-advances if the stage has auto_advance: true
/// - Shows the gate dialog if auto_advance is false/unset
/// - Does nothing if the pane is not part of an active mission
fn try_auto_trigger_mission_gate(
    &mut self,
    terminal_view_id: EntityId,
    ctx: &mut ViewContext<Self>,
) {
    // Debounce: avoid re-processing the same pane's completion
    if self.last_auto_gate_terminal_id == Some(terminal_view_id) {
        return;
    }
    self.last_auto_gate_terminal_id = Some(terminal_view_id);
    
    // Verify agent is supported (has rich status)
    let cli_sessions_model = CLIAgentSessionsModel::as_ref(ctx);
    if let Some(session) = cli_sessions_model.session(terminal_view_id) {
        use crate::terminal::cli_agent_sessions::listener;
        if !listener::is_agent_supported(&session.agent) {
            return;  // Unsupported agent: skip
        }
    } else {
        return;  // No session for this pane
    }
    
    // Map pane to mission
    let mission_index = match self.detect_mission_for_pane(terminal_view_id, ctx) {
        Some(idx) => idx,
        None => return,  // Not part of any mission
    };
    
    // Trigger gate or auto-advance
    self.auto_trigger_mission_gate(mission_index, ctx);
}

/// Detects which active mission (if any) owns the current-stage pane
/// containing the given terminal view.
fn detect_mission_for_pane(
    &self,
    terminal_view_id: EntityId,
    ctx: &AppContext,
) -> Option<usize> {
    let registry = MissionRegistry::as_ref(ctx);
    for (mission_index, mission) in registry.missions().iter().enumerate() {
        // Only check missions with assigned tab groups
        let group_id = mission.group_id?;
        
        // Find tab hosting this group
        let tab = self.tabs.iter().find(|t| t.group_id == Some(group_id))?;
        let pane_group = tab.pane_group.as_ref(ctx);
        
        // Check if pane is in this group
        if pane_group.contains_terminal_view(terminal_view_id, ctx) {
            return Some(mission_index);
        }
    }
    None
}

/// Shows the mission gate dialog or auto-advances based on stage config.
/// Called after a mission's current-stage pane completes.
fn auto_trigger_mission_gate(
    &mut self,
    mission_index: usize,
    ctx: &mut ViewContext<Self>,
) {
    let registry = MissionRegistry::as_ref(ctx);
    let mission = match registry.get(mission_index) {
        Some(m) => m,
        None => return,
    };
    
    let current_stage_idx = mission.current_stage;
    let stage_count = mission.stages.len();
    
    let stage = match mission.stages.get(current_stage_idx) {
        Some(s) => s,
        None => return,
    };
    
    // Last stage: mark complete and remove mission
    if current_stage_idx + 1 >= stage_count {
        let mission_dir = mission.mission_dir.clone();
        let project_dir = mission.project_dir.clone();
        
        if let Err(err) =
            missions::update_manifest_stage(&mission_dir, current_stage_idx, StageStatus::Done)
        {
            log::warn!("Failed to update manifest: {err:?}");
        }
        
        MissionRegistry::handle(ctx).update(ctx, |registry, ctx| {
            registry.remove(mission_index, ctx);
        });
        
        self.toast_stack.update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::success("Missão concluída 🎉".to_string()),
                ctx,
            );
        });
        return;
    }
    
    // Check auto_advance flag
    if stage.auto_advance.unwrap_or(false) {
        // Auto-advance without dialog
        self.advance_mission_stage(mission_index, ctx);
    } else {
        // Show gate dialog (existing flow)
        // This reuses mission_next_stage_for, which handles gate rendering
        self.mission_next_stage_for(Some(mission_index), ctx);
    }
}
```

---

## 9. TRAPS & MITIGATIONS

| Trap | Cause | Mitigation |
|---|---|---|
| **Rapid re-gates** | Multiple `Success` events from same session | Debounce via `last_auto_gate_terminal_id` |
| **Unsupported agents** (Agy, Hermes) | No plugin → `received_rich_notification` stays false | Gate logic checks `is_agent_supported()` before triggering |
| **Pane not in mission** | User switched tabs while mission runs | `detect_mission_for_pane()` returns `None` → skip |
| **Agent finishes but user is looking at it** | Completion while focus is on stage pane | Gate still fires (user sees it) or auto-advances (seamless). Design choice. |
| **Mission replaced mid-stage** | Rare: another workflow removes & re-adds | `registry.get(mission_index)` returns `None` → safe no-op |
| **Manifest out of sync** | File edited externally | `update_manifest_stage()` already handles errors |

---

## 10. TESTING STRATEGY

### Unit Tests

1. **Template parsing:** Verify `auto_advance` field parses from YAML (add to `mod tests`)
2. **Debounce:** Send two `StatusChanged { Success }` for same pane → verify second is dropped
3. **Pane mapping:** Mock missions + pane groups → verify correct mission detected
4. **Agent filtering:** Unsupported agent completion → verify gate not triggered

### Integration Tests

1. **Manual gate flow (existing):** Stage with `auto_advance: false` → dialog appears
2. **Auto-advance flow (new):** Stage with `auto_advance: true` → no dialog, immediately next stage
3. **Last stage (existing):** Any completion on last stage → mission removed
4. **Unsupported agent (new):** Codex OSC 9 only → no auto-gate, but notification still fires

### QA Checklist

- [ ] Start Spec-Driven mission with modified template (add `auto_advance: true` to stage 1)
- [ ] Run Claude harness in stage 1 pane
- [ ] Verify gate dialog **does not** appear; stage auto-advances
- [ ] Set `auto_advance: false` on stage 2
- [ ] Run Claude harness in stage 2 pane
- [ ] Verify gate dialog **does** appear; require manual confirm
- [ ] Test unsupported agent (Agy, Hermes): no auto-gate, but completion notification fires
- [ ] Test pane not in mission: completion in unrelated tab → no auto-gate
- [ ] Test last stage completion: mission removed, toast shown

---

## 11. FORWARD COMPATIBILITY

The `auto_advance` field defaults to `None` (serde `default`), which we treat as `false`. **Existing templates remain unchanged.** Users can opt-in by editing YAML:

```yaml
stages:
  - name: "Arquiteto"
    # No auto_advance field → defaults to false (manual gate)
  - name: "Implementador"
    auto_advance: true  # Optional: auto-advance on completion
```

---

## SUMMARY: EXACT MODIFICATIONS CHECKLIST

1. **`app/src/missions/templates.rs:26`**
   - Add `#[serde(default)] pub auto_advance: Option<bool>` to `MissionStage`

2. **`app/src/workspace/view.rs:~2000`**
   - Add `last_auto_gate_terminal_id: Option<EntityId>` field to `Workspace` struct

3. **`app/src/workspace/view.rs:3643–3658`**
   - Expand `handle_cli_agent_sessions_event()` to call `self.try_auto_trigger_mission_gate()` on `Success`

4. **`app/src/workspace/view.rs:~7220`** (after `advance_mission_stage`)
   - Add `try_auto_trigger_mission_gate()`
   - Add `detect_mission_for_pane()`
   - Add `auto_trigger_mission_gate()`

5. **No other files modified** — the event subscription, gate dialog, and registry are already in place.
