//! Cockpit Missions: templated, multi-stage agent workflows with human gates.
//!
//! - [`templates`]: the YAML template schema and loader.
//! - [`scaffold`]: the on-disk `.cockpit/` structure and prompt rendering.
//! - [`registry`]: the in-memory singleton registry of active missions.
//! - [`start_mission_modal`]: the "Start Mission" modal body view.
//! - [`gate_dialog`]: the between-stages human gate confirmation dialog.

pub mod gate_dialog;
pub mod registry;
pub mod scaffold;
pub mod start_mission_modal;
pub mod templates;

pub use registry::{ActiveMission, MissionRegistry, MissionRegistryEvent};
pub use scaffold::{
    effective_stages, onboarding_stage, profile_path, render_stage_prompt, scaffold_mission,
    slugify, update_manifest_stage, ManifestStage, MissionManifest, ScaffoldedMission, StageStatus,
    COCKPIT_DIR,
};
pub use templates::{load_mission_templates, missions_dir, MissionStage, MissionTemplate};

use warpui::AppContext;

pub fn init(ctx: &mut AppContext) {
    ctx.add_singleton_model(|_| registry::MissionRegistry::default());
    start_mission_modal::init(ctx);
    gate_dialog::init(ctx);
}
