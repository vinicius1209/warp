//! Persistence of active missions across app restarts.
//!
//! Active missions are mirrored to `missions-state.json` in the Warp data
//! directory on every registry mutation and rehydrated into the
//! [`crate::missions::MissionRegistry`] at startup.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::missions::registry::ActiveMission;
use crate::missions::templates::MissionStage;
use crate::workspace::tab_group::TabGroupId;

/// Path of the persisted missions state file.
fn state_file_path() -> PathBuf {
    warp_core::paths::data_dir().join("missions-state.json")
}

/// The on-disk form of an [`ActiveMission`].
#[derive(Serialize, Deserialize)]
struct PersistedMission {
    slug: String,
    template_name: String,
    project_dir: PathBuf,
    mission_dir: PathBuf,
    /// Effective stages were computed at mission start (including any
    /// onboarding stage); they're persisted verbatim so a restart never
    /// recomputes them differently.
    stages: Vec<MissionStage>,
    current_stage: usize,
    default_harness: String,
    /// Inner uuid of the tab group hosting the mission's stage tabs. Note:
    /// session restore mints fresh `TabGroupId`s (see `read_app_state` in
    /// `persistence/sqlite.rs`), so after a restart this id won't match any
    /// live group. It's rebound to the live group at startup by matching the
    /// mission's stable `slug` against each restored group's `mission_slug`
    /// (see `reconcile_restored_mission_groups` in `workspace/view.rs`); this
    /// stale value is kept only for round-trip fidelity.
    group_id: Option<Uuid>,
}

impl From<&ActiveMission> for PersistedMission {
    fn from(mission: &ActiveMission) -> Self {
        Self {
            slug: mission.slug.clone(),
            template_name: mission.template_name.clone(),
            project_dir: mission.project_dir.clone(),
            mission_dir: mission.mission_dir.clone(),
            stages: mission.stages.clone(),
            current_stage: mission.current_stage,
            default_harness: mission.default_harness.clone(),
            group_id: mission.group_id.map(|group_id| group_id.0),
        }
    }
}

impl From<PersistedMission> for ActiveMission {
    fn from(mission: PersistedMission) -> Self {
        Self {
            slug: mission.slug,
            template_name: mission.template_name,
            project_dir: mission.project_dir,
            mission_dir: mission.mission_dir,
            stages: mission.stages,
            current_stage: mission.current_stage,
            default_harness: mission.default_harness,
            group_id: mission.group_id.map(TabGroupId),
        }
    }
}

/// Writes the given missions to the default state file.
pub fn save(missions: &[ActiveMission]) -> anyhow::Result<()> {
    save_to(&state_file_path(), missions)
}

/// Writes the given missions to `path` atomically-ish: the contents are
/// written to a sibling temp file first, then renamed into place.
fn save_to(path: &Path, missions: &[ActiveMission]) -> anyhow::Result<()> {
    let persisted: Vec<PersistedMission> = missions.iter().map(Into::into).collect();
    let contents = serde_json::to_string_pretty(&persisted)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, contents)?;
    fs::rename(&temp_path, path)?;
    Ok(())
}

/// Loads persisted missions from the default state file. A missing file or a
/// parse failure yields an empty list (the latter with a warning); missions
/// whose `mission_dir` no longer exists on disk are dropped as stale.
pub fn load() -> Vec<ActiveMission> {
    load_from(&state_file_path())
}

fn load_from(path: &Path) -> Vec<ActiveMission> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Failed to read missions state file {path:?}: {err:?}");
            }
            return Vec::new();
        }
    };
    let persisted = match serde_json::from_str::<Vec<PersistedMission>>(&contents) {
        Ok(persisted) => persisted,
        Err(err) => {
            log::warn!("Failed to parse missions state file {path:?}: {err:?}");
            return Vec::new();
        }
    };
    persisted
        .into_iter()
        .filter(|mission| {
            let exists = mission.mission_dir.exists();
            if !exists {
                log::warn!(
                    "Dropping stale persisted mission {:?}: mission dir {:?} no longer exists",
                    mission.slug,
                    mission.mission_dir
                );
            }
            exists
        })
        .map(Into::into)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_mission(mission_dir: PathBuf, group_id: Option<TabGroupId>) -> ActiveMission {
        ActiveMission {
            slug: "spec-driven-20260612-120000".to_string(),
            template_name: "Spec-Driven".to_string(),
            project_dir: PathBuf::from("/tmp/project"),
            mission_dir,
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
            current_stage: 1,
            default_harness: "claude".to_string(),
            group_id,
        }
    }

    fn assert_missions_eq(actual: &ActiveMission, expected: &ActiveMission) {
        assert_eq!(actual.slug, expected.slug);
        assert_eq!(actual.template_name, expected.template_name);
        assert_eq!(actual.project_dir, expected.project_dir);
        assert_eq!(actual.mission_dir, expected.mission_dir);
        assert_eq!(actual.stages, expected.stages);
        assert_eq!(actual.current_stage, expected.current_stage);
        assert_eq!(actual.default_harness, expected.default_harness);
        assert_eq!(actual.group_id, expected.group_id);
    }

    #[test]
    fn test_save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("missions-state.json");
        let mission_dir = dir.path().join("mission-a");
        fs::create_dir_all(&mission_dir).unwrap();

        let with_group = sample_mission(mission_dir.clone(), Some(TabGroupId::new()));
        let without_group = ActiveMission {
            slug: "solo-20260612-130000".to_string(),
            group_id: None,
            ..sample_mission(mission_dir, None)
        };

        save_to(&state_path, &[with_group.clone(), without_group.clone()]).unwrap();
        let loaded = load_from(&state_path);

        assert_eq!(loaded.len(), 2);
        assert_missions_eq(&loaded[0], &with_group);
        assert_missions_eq(&loaded[1], &without_group);
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_from(&dir.path().join("does-not-exist.json")).is_empty());
    }

    #[test]
    fn test_load_unparsable_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("missions-state.json");
        fs::write(&state_path, "not json {").unwrap();
        assert!(load_from(&state_path).is_empty());
    }

    #[test]
    fn test_load_drops_missions_with_missing_mission_dir() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("missions-state.json");
        let live_dir = dir.path().join("live-mission");
        fs::create_dir_all(&live_dir).unwrap();

        let live = sample_mission(live_dir, None);
        let stale = ActiveMission {
            slug: "stale".to_string(),
            ..sample_mission(dir.path().join("deleted-mission"), None)
        };

        save_to(&state_path, &[stale, live.clone()]).unwrap();
        let loaded = load_from(&state_path);

        assert_eq!(loaded.len(), 1);
        assert_missions_eq(&loaded[0], &live);
    }

    #[test]
    fn test_save_overwrites_previous_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("missions-state.json");
        let mission_dir = dir.path().join("mission-a");
        fs::create_dir_all(&mission_dir).unwrap();

        save_to(&state_path, &[sample_mission(mission_dir, None)]).unwrap();
        save_to(&state_path, &[]).unwrap();
        assert!(load_from(&state_path).is_empty());
    }
}
