Perfect! Now I have enough information to write a comprehensive report. Let me create it:

## Warp Tab-Group-First Architecture Investigation

### 1. TAB GROUP MODEL

**Core Definitions:**
- `TabGroup` struct (app/src/workspace/tab_group.rs:27-55): Represents a group with:
  - `id: TabGroupId` — UUID-based stable identity
  - `name: Option<String>` — user-editable group name
  - `color: SelectedTabColor` — visual indicator
  - `collapsed: bool` — collapse/expand state
  - `draggable_state: DraggableState` — drag-and-drop tracking
  - `pinned: bool` — pinned to front of list

- `TabData::group_id: Option<TabGroupId>` (app/src/tab.rs:156): Each tab references its group

- `Workspace::tab_groups: HashMap<TabGroupId, TabGroup>` (app/src/workspace/view.rs:978): Groups indexed by ID

**Group Lifecycle:**
- `TabGroup::new()` creates groups with fresh UUIDs (tab_group.rs:39-48)
- Groups created via:
  - `NewTabGroupFromTab(usize)` (action.rs:188): Group a single tab
  - `NewTabGroupFromSelectedTabs` (action.rs:222): Multi-selection grouping
  - `NewTabInGroup(TabGroupId)` (action.rs:234): Add new tab to existing group
  - `UngroupTabs(TabGroupId)` (action.rs:233): Remove tabs from group
  - `CloseTabGroup(TabGroupId)` (action.rs:182): Delete entire group

**Tab Membership Management:**
- `group_member_indices()` (view.rs:28272-28280): Iterate group members
- `group_member_index_range()` (view.rs:28286-28291): Get contiguous run of tabs
- `group_has_single_member()` (view.rs:28298-28300): Single-member detection
- Group membership is **contiguous** — the workspace enforces invariant that all tabs in a group must be adjacent in the tab list

**Group Actions (WorkspaceAction enum, action.rs:182-237):**
- `ToggleTabGroupCollapsed(TabGroupId)` — collapse/expand UI
- `RenameTabGroup(TabGroupId)` — inline rename editor
- `MoveTabGroupUp/Down(TabGroupId)` — reorder groups
- `CloseTabsOutsideGroup/Above/Below(TabGroupId)` — bulk close operations
- `StartGroupDrag(TabGroupId)` — drag-to-reorder groups
- `ToggleTabGroupRightClickMenu { group_id, anchor }` — context menu
- `MissionNextStageForGroup(TabGroupId)` — advance mission stage for group

---

### 2. SIDEBAR RENDERING

**Vertical Tabs Panel (app/src/workspace/view/vertical_tabs.rs):**
- **Group Headers** (line 1638+): Render collapsible group headers with:
  - Chevron icon (expand/collapse)
  - Group name (or "Untitled group")
  - Group color indicator
  - Kebab menu button (three-dot context menu)
  - Close button
- **Member Tabs**: Indented beneath group header by `TAB_GROUP_MEMBER_INDENT` (102 px)
- **Constants:**
  - `GROUP_HEADER_VERTICAL_PADDING: 4.0`
  - `TAB_GROUP_MEMBER_INDENT: 12.0`
  - `TAB_GROUP_ICON_SIZE: 16.0`
- **Icons:** Group members display icon collage via `render_group_member_icon_collage()` (view.rs:28152-28246)
  - Shows 1-4 unique pane kind icons (terminal, code, notebook, agent, etc.)
  - Positioned in grid/triangle layout
- **Detail Sidecar** (vertical_tabs.rs:117): Right-panel metadata display per tab/group:
  - Git branch info
  - Working directory
  - Agent status
  - Conversation metadata
  - Positioned at `VERTICAL_TABS_DETAIL_SIDECAR_POSITION_ID`

**Horizontal Tab Bar (app/src/workspace/view.rs:19759-19975):**
- Groups rendered as "tab slots" via `render_horizontal_tab_group()` (19759)
- **Group Header** in horizontal bar:
  - Icon collage (4-icon max) via `render_group_member_icon_collage()`
  - Group name label
  - Kebab menu for group actions
  - Size: `GROUP_ICON_COLLAGE_SIZE` + label
- **Member Tabs**: Rendered inline after header, can overflow into secondary row
- Hover states: `HorizontalTabGroupMouseStates` per group (view.rs:949)
- Position ID: `htab_group_position_id(group_id)` for drop-target hit testing

**Right-Click Menu (view.rs:10145-10248):**
- Group context menu via `show_tab_group_right_click_menu: Option<(TabGroupId, TabContextMenuAnchor)>`
- Items: Rename, Move Up/Down, Close, Delete all tabs outside/above/below, Mission stage advancement
- Single-tab vs. multi-tab selection drives menu display:
  - Single: `MoveTabToGroup { tab_index, group_id }`
  - Multi: `MoveSelectedTabsToGroup { group_id }`

---

### 3. MULTI-WORKSPACE SYSTEM

**Multiple Windows/Workspaces:**
- `MultiWorkspace` feature flag (warp_features/lib.rs:183): Gates multi-window support
- `WorkspaceRegistry` (workspace/registry.rs): Singleton tracking all open workspaces
  - `workspaces: HashMap<WindowId, WeakViewHandle<Workspace>>`
  - `register(window_id, workspace)` — called when window opens
  - `get(window_id, app)` — access workspace by window
  - `all_upgraded(app)` — enumerate all workspaces
- `CrossWindowTabDrag` (workspace/cross_window_tab_drag.rs): Handle drag-and-drop between windows
  - `preview_window_id: WindowId` — preview window for multi-tab drags
  - Ghost state for visual feedback
  - Tab can move across windows while preserving group membership

**Current Workspace Architecture:**
- Each `Workspace` view is per-window (one window = one Workspace instance)
- Workspace contains: tabs, tab_groups, pane_groups, UI state
- **No global workspace concept** — "workspace" == a window's collection of tabs
- Groups are **per-window** — TabGroupId is unique within a window but can be regenerated on restore (critical limitation, see below)

**Tab and Group Persistence to Tabs:**
- `TabSnapshot::group_id: Option<TabGroupId>` (app_state.rs:82)
- `TabGroupSnapshot` (app_state.rs:66-71) persists:
  - id, name, color, collapsed state
  - **NOT persisted:** pinned state (TODO at view.rs:3863), drag state, hover states
- Saved via `WindowSnapshot::tab_groups: Vec<TabGroupSnapshot>` (app_state.rs:62)

---

### 4. PERSISTENCE & RESTORATION (CRITICAL FINDING)

**TabGroupId Regeneration on Restore:**
Located in `/Users/viniciusmachado/projetos/warp/app/src/persistence/sqlite.rs:2467-2485`:

```rust
// Line 2467-2472: Mint a fresh `TabGroupId` per row
let mut tab_group_id_by_row_id: HashMap<i32, TabGroupId> = HashMap::new();
for group in tab_groups_for_window {
    let tab_group_id = TabGroupId::new();  // <- FRESH UUID EVERY RESTORE
    tab_group_id_by_row_id.insert(group.id, tab_group_id);
    // ...
}
```

**What This Breaks:**
- External references to TabGroupId (e.g., in missions, launch configs, external scripts) **become invalid after restart**
- `MissionRegistry::group_id: Option<TabGroupId>` (missions/registry.rs:24) is regenerated but missions aren't automatically re-bound
- Any persisted user data keying on `group_id` will be orphaned
- **Solution would require:** stable group identifiers (e.g., group name + path hash, or UUID stored separately)

**What's Persisted Correctly:**
- Group metadata: name, color, collapsed state
- Tab-to-group associations via SQLite foreign key (tab.tab_group_id = integer row ID, then looked up)
- Mission stage data (missions/persistence.rs) includes `group_id` but doesn't validate on load

---

### 5. GAPS FOR "TAB-GROUP-FIRST" VISION

### 5.1 Missing Per-Group Project Context

**Not Implemented:**
- `project_root: Option<PathBuf>` on `TabGroup` — **each group could have a default working directory**
- `git_branch: Option<String>` — **group-level git branch indicator**
- `git_status: Option<String>` — **show dirty status per group**
- No group-level configuration (e.g., "all tabs in this group use Python 3.11")

**Where to Add:**
- Extend `TabGroup` struct (workspace/tab_group.rs:27-55) with metadata fields
- Add fields to `TabGroupSnapshot` (app_state.rs:66-71) for persistence
- Update SQLite schema (persistence/sqlite.rs) to store project_root, git info
- Render in vertical tabs header (vertical_tabs.rs) and horizontal bar

**Current Workaround:** Individual tabs store `default_directory_color` (tab.rs:149) computed per-tab from pwd

---

### 5.2 Missing Group-Level Default Harness/Mission

**Not Implemented:**
- `default_harness: String` should be on `TabGroup` (currently only on `ActiveMission`, missions/registry.rs:22)
- Group could specify "default shell for new tabs" (e.g., /bin/bash vs /bin/zsh)
- Group could specify "default agent mode" for new tabs in the group

**Where to Add:**
- `TabGroup` struct extension with optional harness, execution profile, agent template
- `group_id` lookup in `new_terminal_options` (workspace/view.rs) to apply group defaults
- UI: Group settings/properties sheet

**Current Flow:** Mission binds to group at `MissionRegistry::set_group_id()` (missions/registry.rs:74-85), but only one mission per group

---

### 5.3 Missing Rich Group-Level Actions

**Currently Available:**
- Rename, move up/down, close, delete tabs outside/in group (action.rs)
- Mission stage advancement only (view.rs:7031-7206)

**Not Implemented:**
- **Per-group commands palette** — quick access to group-specific actions
- **Run command in group** — execute shell command in first terminal of group
- **Batch operations** — close all tabs in group and reopen them, duplicate group with all tabs
- **Group templates** — save/restore a group as a reusable project layout
- **Group profiles** — switch between named collections of groups
- **Export/import group** — share a project group configuration

**Where to Add:**
- New `WorkspaceAction` variants in action.rs (e.g., `BatchCloseAndReopenGroup`, `ExportGroup`)
- Group context menu expansion in view.rs:10145-10248
- View handles in Workspace struct for modals/editors

---

### 5.4 Missing Rich Sidebar

**Current Sidebar Rendering (vertical_tabs.rs):**
- Tab rows show: icon collage + title + git branch
- Detail sidecar (on hover) shows: full path, branch, agent status, conversation metadata
- **Missing from group headers:**
  - Project name (from group or from pwd of first tab)
  - Git status indicator (dirty, branch name, upstream divergence)
  - Mission status (active mission name, current stage)
  - Agent activity indicator (active agents in group's tabs)
  - Recent files/commands (for context)

**Where to Add:**
- Extend group header rendering in `render_horizontal_tab_group_header()` (view.rs:19873-19975)
- Extend vertical tabs group header in vertical_tabs.rs (around 1638+)
- New render functions for group-level metadata
- Compute group metadata on-demand (scan member tabs for mission, git, agent status)

**Files to Modify:**
- app/src/workspace/view/vertical_tabs.rs (group headers)
- app/src/workspace/view.rs (horizontal bar group rendering)
- app/src/workspace/tab_group.rs (add metadata fields)

---

### 5.5 Missing Quick Workspace Switching

**Not Implemented:**
- No built-in switcher to jump between open windows (other than Cmd+Tab OS-level)
- No group palette (quick search/jump to group in current window)
- No "quick switch group" keybinding

**Where to Add:**
- New command palette entry: "Switch to group" (search + select)
- New window switcher (OS-native-style) in app/src/root_view.rs
- WorkspaceRegistry integration to enumerate all groups across windows
- New WorkspaceAction: `SwitchToWindow(WindowId)`, `SwitchToGroup(TabGroupId, WindowId)`

**Files to Modify:**
- app/src/workspace/action.rs (new actions)
- app/src/workspace/registry.rs (group enumeration helper)
- app/src/root_view.rs (window management)
- app/src/workspace/view.rs (action handlers)

---

### 5.6 Group Stability / External Integration

**Critical Gap:**
- `TabGroupId` is **not stable across restarts** (see persistence section above)
- External tools cannot reliably reference groups (e.g., launch config: "open group named 'backend'")
- Mission binding breaks on restart (mission persists `group_id: TabGroupId` but UUID is regenerated)

**Solution Required:**
- Introduce **`GroupName` as a stable identifier** (within window scope):
  - Unique (enforced in UI when renaming)
  - Persisted as string, not UUID
- Optionally add **`GroupPath`**: project_root + group_name combo for global uniqueness
- Update `ActiveMission.group_id` to use group name instead of UUID
- Update WorkspaceAction dispatch to accept group name

**Files to Modify:**
- app/src/workspace/tab_group.rs (add stable ID field, e.g., `stable_name: String`)
- app/src/persistence/sqlite.rs (key groups by name in restore)
- app/src/missions/registry.rs and persistence.rs (use group name)
- app/src/workspace/action.rs (accept both name and UUID for compatibility)

---

### 6. RECOMMENDED ROADMAP FOR TAB-GROUP-FIRST

**Phase 1: Stabilize Groups**
1. Add stable group naming (enforce uniqueness per-window)
2. Fix persistence: key groups by name + project_root
3. Update mission binding to use stable group names
4. Add group rename validation (no duplicates)
Files: tab_group.rs, sqlite.rs, missions/registry.rs, action.rs

**Phase 2: Group Metadata & Context**
1. Extend TabGroup with `project_root`, `git_branch`, `default_harness`
2. Persist metadata in TabGroupSnapshot
3. Render in sidebar: project name, git status, harness label
4. Update vertical_tabs.rs and view.rs horizontal bar rendering
Files: tab_group.rs, app_state.rs, sqlite.rs, vertical_tabs.rs, view.rs

**Phase 3: Group-First Sidebar**
1. Group headers show: project name (or first member's pwd), git status, agent count
2. Detail sidecar for groups (show metadata on hover)
3. Inline group settings (right-click → "Group settings")
4. Group-level action buttons (run command, quick actions)
Files: vertical_tabs.rs, view.rs

**Phase 4: Enhanced Group Operations**
1. New actions: batch rename, duplicate, export, import
2. Group palette: Cmd+K to search/jump to group
3. Group templates: save/restore layout
4. Multi-window group aggregation (view all groups across windows)
Files: action.rs, workspace/view.rs, root_view.rs, registry.rs, new view for palette

**Phase 5: Integration & Polish**
1. Launch configs reference groups by stable name
2. Keyboard shortcuts: switch group (Ctrl+Shift+Right/Left?), open group menu
3. Heuristics to auto-group related tabs (detect git repos, group by project)
4. Group collapsing performance (hide tabs, keep group header)
Files: action.rs, workspace/view.rs, workspace/tab_settings.rs

---

### 7. FILE INVENTORY

| File | Lines | Purpose |
|------|-------|---------|
| **Core Model** |
| app/src/workspace/tab_group.rs | 56 | TabGroup struct, TabGroupId definition |
| **Actions** |
| app/src/workspace/action.rs | ~250+ | WorkspaceAction enum with 15+ group-related variants |
| **Views** |
| app/src/workspace/view.rs | ~29k | Workspace view, tab/group rendering, action handlers |
| app/src/workspace/view/vertical_tabs.rs | ~6800 | Vertical sidebar rendering, group headers, detail sidecar |
| app/src/workspace/view/tab_grouping.rs | 527 | Tab selection, group membership logic |
| app/src/workspace/view/left_panel.rs | ~500 | Left sidebar (agent conversations, project explorer, etc.) |
| **State & Persistence** |
| app/src/app_state.rs | ~500+ | TabGroupSnapshot, WindowSnapshot definitions |
| app/src/persistence/sqlite.rs | ~2800 | Group persistence, **TabGroupId regeneration (line 2467-2485)** |
| app/src/tab.rs | ~1900 | TabData struct (includes group_id field) |
| **Missions (Group Integration)** |
| app/src/missions/registry.rs | ~130 | ActiveMission struct with group_id, set_group_id() method |
| app/src/missions/persistence.rs | ~200+ | Mission state save/load |
| **Feature Flags** |
| crates/warp_features/src/lib.rs | ~1000+ | FeatureFlag::GroupedTabs, FeatureFlag::MultiWorkspace |
| **Workspace Management** |
| app/src/workspace/registry.rs | 60 | WorkspaceRegistry (per-window workspace tracking) |
| app/src/workspace/cross_window_tab_drag.rs | ~1700 | Cross-window drag-and-drop |

---

### 8. KEY INSIGHTS

1. **Architecture is sound but incomplete**: Groups are well-modeled in the core layer, but sidebar/UX treats tabs as primary, groups as secondary grouping.

2. **Persistence is fragile**: TabGroupId regeneration on restore breaks external references and mission binding. Requires name-based stability.

3. **Sidebar is tab-centric**: Group headers are minimal; most metadata (git, cwd, agent status) shown per-tab in detail sidecar, not per-group.

4. **Group-first UX not yet prioritized**: Menu options exist but no quick-access group palette, limited group operations, no group-level defaults (harness, config, etc.).

5. **Mission binding is half-baked**: Missions can bind to groups, but TabGroupId regeneration orphans the binding on restart. Needs stable group names.

6. **MultiWorkspace infrastructure exists but underdeveloped**: WindowId-based registry is ready, but no cross-window group visibility or workspace switching UI.

7. **Rich opportunities**: Extending TabGroup with project metadata, adding group-level commands, richer sidebar rendering, and stable identifiers would unlock the full "tab-group-first" vision without major refactors.
