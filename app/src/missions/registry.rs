//! In-memory registry of active Cockpit missions, exposed as a singleton model.

use std::path::PathBuf;

use warpui::{Entity, ModelContext, SingletonEntity};

use crate::missions::templates::MissionStage;
use crate::workspace::tab_group::TabGroupId;

/// A mission currently running in this app session.
#[derive(Clone, Debug)]
pub struct ActiveMission {
    pub slug: String,
    pub template_name: String,
    pub project_dir: PathBuf,
    pub mission_dir: PathBuf,
    /// Effective stages for the mission (including any onboarding stage).
    pub stages: Vec<MissionStage>,
    pub current_stage: usize,
    /// Default CLI harness for stages that don't specify one, e.g. "claude".
    pub default_harness: String,
    /// Tab group hosting this mission's stage tabs, when `GroupedTabs` is enabled.
    pub group_id: Option<TabGroupId>,
}

/// A global model tracking active missions.
#[derive(Default)]
pub struct MissionRegistry {
    missions: Vec<ActiveMission>,
}

#[derive(Debug, Clone)]
pub enum MissionRegistryEvent {
    Changed,
}

impl MissionRegistry {
    /// Registers a new mission and returns its index in the registry.
    pub fn register(&mut self, mission: ActiveMission, ctx: &mut ModelContext<Self>) -> usize {
        self.missions.push(mission);
        ctx.emit(MissionRegistryEvent::Changed);
        self.missions.len() - 1
    }

    pub fn get(&self, index: usize) -> Option<&ActiveMission> {
        self.missions.get(index)
    }

    /// Advances the mission at `index` to its next stage.
    pub fn advance_stage(&mut self, index: usize, ctx: &mut ModelContext<Self>) {
        if let Some(mission) = self.missions.get_mut(index) {
            mission.current_stage += 1;
            ctx.emit(MissionRegistryEvent::Changed);
        }
    }

    /// Binds the mission at `index` to the tab group hosting its stage tabs.
    pub fn set_group_id(
        &mut self,
        index: usize,
        group_id: TabGroupId,
        ctx: &mut ModelContext<Self>,
    ) {
        if let Some(mission) = self.missions.get_mut(index) {
            mission.group_id = Some(group_id);
            ctx.emit(MissionRegistryEvent::Changed);
        }
    }

    /// Removes the mission at `index` from the registry.
    pub fn remove(&mut self, index: usize, ctx: &mut ModelContext<Self>) {
        if index < self.missions.len() {
            self.missions.remove(index);
            ctx.emit(MissionRegistryEvent::Changed);
        }
    }

    pub fn missions(&self) -> &[ActiveMission] {
        self.missions.as_slice()
    }

    /// Index of the most recently registered mission, for v1 single-mission flows.
    pub fn find_latest(&self) -> Option<usize> {
        self.missions.len().checked_sub(1)
    }

    /// Index of the mission whose stage tabs live in the given tab group.
    /// The most recently registered match wins.
    pub fn find_by_group(&self, group_id: TabGroupId) -> Option<usize> {
        self.missions
            .iter()
            .rposition(|mission| mission.group_id == Some(group_id))
    }
}

impl Entity for MissionRegistry {
    type Event = MissionRegistryEvent;
}

impl SingletonEntity for MissionRegistry {}
