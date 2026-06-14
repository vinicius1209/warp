//! Cockpit Missions: templated, multi-stage agent workflows with human gates.
//!
//! - [`templates`]: the YAML template schema and loader.
//! - [`scaffold`]: the on-disk `.cockpit/` structure and prompt rendering.
//! - [`registry`]: the in-memory singleton registry of active missions.
//! - [`persistence`]: the on-disk mirror of the registry, so active missions
//!   survive app restarts.
//! - [`reconcile`]: pure restore reconciliation rebinding each mission's stale
//!   `group_id` to its live tab group by stable slug after a session restore.
//! - [`start_mission_modal`]: the "Start Mission" modal body view.
//! - [`mission_control_modal`]: the "Mission Control" active-missions overview modal.
//! - [`gate_dialog`]: the between-stages human gate confirmation dialog.

pub mod gate_dialog;
pub mod mission_control_modal;
pub mod persistence;
pub mod preflight;
pub mod reconcile;
pub mod registry;
pub mod scaffold;
pub mod start_mission_modal;
pub mod templates;

pub use preflight::{check_harness, pick_default_harness, required_harnesses, HarnessAvailability};
pub use reconcile::reconcile_mission_groups;
pub use registry::{ActiveMission, MissionRegistry, MissionRegistryEvent};
pub use scaffold::{
    effective_stages, mark_mission_abandoned, onboarding_stage, profile_path, render_stage_prompt,
    scaffold_mission, slugify, update_manifest_stage, ManifestStage, MissionManifest,
    ScaffoldedMission, StageStatus, COCKPIT_DIR,
};
pub use templates::{load_mission_templates, missions_dir, MissionStage, MissionTemplate};

use warpui::AppContext;

pub fn init(ctx: &mut AppContext) {
    // Rehydrate missions persisted by a previous app session. No observers
    // exist yet, so no Changed event is needed: the footer mission chip syncs
    // itself at construction (`sync_mission_button`), after this runs.
    ctx.add_singleton_model(|_| {
        let mut registry = registry::MissionRegistry::default();
        registry.rehydrate(persistence::load());
        registry
    });
    start_mission_modal::init(ctx);
    mission_control_modal::init(ctx);
    gate_dialog::init(ctx);
}
