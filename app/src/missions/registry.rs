//! In-memory registry of active Cockpit missions, exposed as a singleton model.

use std::path::PathBuf;

use warpui::{Entity, ModelContext, SingletonEntity};

use crate::missions::persistence;
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
    /// Mirrors the current missions to the on-disk state file so active
    /// missions survive app restarts. Failures are logged, never fatal.
    fn persist(&self) {
        if let Err(err) = persistence::save(&self.missions) {
            log::warn!("Failed to persist missions state: {err:?}");
        }
    }

    /// Replaces the registry's missions with state loaded from disk. Called
    /// once at startup, before any observers exist, so no event is emitted.
    pub fn rehydrate(&mut self, missions: Vec<ActiveMission>) {
        self.missions = missions;
    }

    /// Registers a new mission and returns its index in the registry.
    pub fn register(&mut self, mission: ActiveMission, ctx: &mut ModelContext<Self>) -> usize {
        self.missions.push(mission);
        self.persist();
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
            self.persist();
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
            self.persist();
            ctx.emit(MissionRegistryEvent::Changed);
        }
    }

    /// Removes the mission at `index` from the registry.
    pub fn remove(&mut self, index: usize, ctx: &mut ModelContext<Self>) {
        if index < self.missions.len() {
            self.missions.remove(index);
            self.persist();
            ctx.emit(MissionRegistryEvent::Changed);
        }
    }

    pub fn missions(&self) -> &[ActiveMission] {
        self.missions.as_slice()
    }

    /// Index of the mission with the given stable `slug`. The slug is the
    /// durable identity of a mission: unlike `group_id` it never changes, so
    /// every durable reference (gate-pending state, restore reconciliation)
    /// resolves through this at call time.
    pub fn find_by_slug(&self, slug: &str) -> Option<usize> {
        self.missions
            .iter()
            .position(|mission| mission.slug == slug)
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

#[cfg(test)]
mod tests {
    use warpui::App;

    use super::*;

    fn sample_mission(slug: &str, group_id: Option<TabGroupId>) -> ActiveMission {
        ActiveMission {
            slug: slug.to_string(),
            template_name: "Spec-Driven".to_string(),
            project_dir: PathBuf::from("/tmp/project"),
            mission_dir: PathBuf::from("/tmp/project/.cockpit/missions").join(slug),
            stages: vec![
                MissionStage {
                    name: "Arquiteto".to_string(),
                    harness: None,
                    prompt: "escreva a spec".to_string(),
                    gate: Some("revise a spec".to_string()),
                },
                MissionStage {
                    name: "Implementador".to_string(),
                    harness: Some("opencode".to_string()),
                    prompt: "implemente a spec".to_string(),
                    gate: None,
                },
            ],
            current_stage: 0,
            default_harness: "claude".to_string(),
            group_id,
        }
    }

    // The read-only finders never touch a `ModelContext`, so they can be
    // exercised against a rehydrated registry without the test app harness.
    fn rehydrated(missions: Vec<ActiveMission>) -> MissionRegistry {
        let mut registry = MissionRegistry::default();
        registry.rehydrate(missions);
        registry
    }

    /// Redirects `$HOME` to a fresh temp dir for its lifetime so the registry's
    /// `persist()` writes land in the sandbox, not the developer's real
    /// `~/.warp-oss`. Tests using it must be `#[serial_test::serial]` because
    /// `$HOME` is process-global. The original value is restored on drop.
    struct HomeGuard {
        _dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("HOME");
            std::env::set_var("HOME", dir.path());
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(previous) => std::env::set_var("HOME", previous),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn test_find_by_slug_resolves_stable_identity() {
        let registry = rehydrated(vec![
            sample_mission("spec-driven-20260612-120000", None),
            sample_mission("solo-20260612-130000", None),
        ]);
        assert_eq!(
            registry.find_by_slug("spec-driven-20260612-120000"),
            Some(0)
        );
        assert_eq!(registry.find_by_slug("solo-20260612-130000"), Some(1));
        assert_eq!(registry.find_by_slug("does-not-exist"), None);
    }

    #[test]
    fn test_find_by_group_returns_most_recent_match() {
        let shared = TabGroupId::new();
        let registry = rehydrated(vec![
            sample_mission("spec-driven-20260612-120000", Some(shared)),
            sample_mission("solo-20260612-130000", Some(shared)),
        ]);
        // Duplicate group ids: the most recently registered match wins.
        assert_eq!(registry.find_by_group(shared), Some(1));
        assert_eq!(registry.find_by_group(TabGroupId::new()), None);
    }

    #[test]
    #[serial_test::serial]
    fn test_register_appends_and_returns_index() {
        let _home = HomeGuard::new();
        App::test((), |mut app| async move {
            let registry = app.add_model(|_| MissionRegistry::default());
            let first = registry.update(&mut app, |registry, ctx| {
                registry.register(sample_mission("spec-driven-20260612-120000", None), ctx)
            });
            let second = registry.update(&mut app, |registry, ctx| {
                registry.register(sample_mission("solo-20260612-130000", None), ctx)
            });
            assert_eq!(first, 0);
            assert_eq!(second, 1);
            registry.read(&app, |registry, _| {
                assert_eq!(registry.missions().len(), 2);
                assert_eq!(registry.find_by_slug("solo-20260612-130000"), Some(1));
            });
        });
    }

    #[test]
    #[serial_test::serial]
    fn test_advance_stage_bumps_current_stage() {
        let _home = HomeGuard::new();
        App::test((), |mut app| async move {
            let registry = app.add_model(|_| MissionRegistry::default());
            let index = registry.update(&mut app, |registry, ctx| {
                registry.register(sample_mission("spec-driven-20260612-120000", None), ctx)
            });
            registry.update(&mut app, |registry, ctx| {
                registry.advance_stage(index, ctx);
            });
            registry.read(&app, |registry, _| {
                assert_eq!(registry.get(index).map(|m| m.current_stage), Some(1));
            });
        });
    }

    #[test]
    #[serial_test::serial]
    fn test_set_group_id_binds_live_group() {
        let _home = HomeGuard::new();
        App::test((), |mut app| async move {
            let registry = app.add_model(|_| MissionRegistry::default());
            let index = registry.update(&mut app, |registry, ctx| {
                registry.register(sample_mission("spec-driven-20260612-120000", None), ctx)
            });
            let group_id = TabGroupId::new();
            registry.update(&mut app, |registry, ctx| {
                registry.set_group_id(index, group_id, ctx);
            });
            registry.read(&app, |registry, _| {
                assert_eq!(registry.get(index).and_then(|m| m.group_id), Some(group_id));
                assert_eq!(registry.find_by_group(group_id), Some(index));
            });
        });
    }

    #[test]
    #[serial_test::serial]
    fn test_remove_drops_mission_and_shifts_indices() {
        let _home = HomeGuard::new();
        App::test((), |mut app| async move {
            let registry = app.add_model(|_| MissionRegistry::default());
            registry.update(&mut app, |registry, ctx| {
                registry.register(sample_mission("spec-driven-20260612-120000", None), ctx);
                registry.register(sample_mission("solo-20260612-130000", None), ctx);
            });
            registry.update(&mut app, |registry, ctx| {
                registry.remove(0, ctx);
            });
            registry.read(&app, |registry, _| {
                assert_eq!(registry.missions().len(), 1);
                // The surviving mission is resolvable by its stable slug at its
                // shifted index; stale index 1 no longer resolves.
                assert_eq!(registry.find_by_slug("solo-20260612-130000"), Some(0));
                assert_eq!(registry.find_by_slug("spec-driven-20260612-120000"), None);
            });
        });
    }

    #[test]
    #[serial_test::serial]
    fn test_slug_is_stable_across_group_rebind() {
        // The keystone invariant: a mission's slug never changes even as its
        // live `group_id` is rebound (as happens on every session restore).
        let _home = HomeGuard::new();
        App::test((), |mut app| async move {
            let registry = app.add_model(|_| MissionRegistry::default());
            let index = registry.update(&mut app, |registry, ctx| {
                registry.register(
                    sample_mission("spec-driven-20260612-120000", Some(TabGroupId::new())),
                    ctx,
                )
            });
            let rebound = TabGroupId::new();
            registry.update(&mut app, |registry, ctx| {
                registry.set_group_id(index, rebound, ctx);
            });
            registry.read(&app, |registry, _| {
                let mission = registry.get(index).unwrap();
                assert_eq!(mission.slug, "spec-driven-20260612-120000");
                assert_eq!(mission.group_id, Some(rebound));
                // The slug still resolves to the same mission after the rebind.
                assert_eq!(registry.find_by_slug(&mission.slug), Some(index));
            });
        });
    }
}
