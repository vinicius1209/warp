//! Scaffolding for the on-disk `.cockpit/` mission structure.
//!
//! Each mission lives in `<project>/.cockpit/missions/<slug>/` with a
//! `brief.md` (the user's briefing) and a `manifest.json` tracking stage
//! progress. The project-wide agent profile lives at
//! `<project>/.cockpit/profile.md`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::missions::templates::{MissionStage, MissionTemplate};

pub const COCKPIT_DIR: &str = ".cockpit";

const MANIFEST_FILE_NAME: &str = "manifest.json";

/// Path to the project profile (`<project>/.cockpit/profile.md`).
pub fn profile_path(project_dir: &Path) -> PathBuf {
    project_dir.join(COCKPIT_DIR).join("profile.md")
}

/// Converts a name into a filesystem-friendly slug: lowercase, non-alphanumeric
/// runs collapsed into a single '-', leading/trailing '-' trimmed.
pub fn slugify(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_string()
}

/// Status of a single stage within a mission manifest.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Pending,
    Running,
    Done,
}

/// A stage entry within a mission manifest.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ManifestStage {
    pub name: String,
    pub status: StageStatus,
}

/// The persisted state of a mission, stored at
/// `<mission_dir>/manifest.json`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MissionManifest {
    pub slug: String,
    pub template: String,
    pub project_dir: PathBuf,
    pub created_at: String,
    pub current_stage: usize,
    pub stages: Vec<ManifestStage>,
    /// RFC 3339 timestamp set when the user abandons the mission from
    /// Mission Control. `None` for live (or completed) missions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandoned_at: Option<String>,
}

/// A freshly scaffolded mission directory.
#[derive(Clone, Debug, PartialEq)]
pub struct ScaffoldedMission {
    pub slug: String,
    pub mission_dir: PathBuf,
}

/// Creates `<project>/.cockpit/missions/<slug>/` with `brief.md` and
/// `manifest.json` for a new mission based on `template`.
pub fn scaffold_mission(
    project_dir: &Path,
    template: &MissionTemplate,
    briefing: &str,
) -> anyhow::Result<ScaffoldedMission> {
    let now = chrono::Local::now();
    let missions_root = project_dir.join(COCKPIT_DIR).join("missions");
    fs::create_dir_all(&missions_root)
        .with_context(|| format!("failed to create missions directory {missions_root:?}"))?;
    let base_slug = format!(
        "{}-{}",
        slugify(&template.name),
        now.format("%Y%m%d-%H%M%S")
    );
    // Uniquify the slug so a retry never silently reuses another mission's dir.
    let (slug, mission_dir) = (0u32..100)
        .map(|n| {
            let slug = if n == 0 {
                base_slug.clone()
            } else {
                format!("{base_slug}-{n}")
            };
            let dir = missions_root.join(&slug);
            (slug, dir)
        })
        .find(|(_, dir)| fs::create_dir(dir).is_ok())
        .with_context(|| {
            format!("failed to create a unique mission directory in {missions_root:?}")
        })?;

    fs::write(mission_dir.join("brief.md"), briefing)?;

    let manifest = MissionManifest {
        slug: slug.clone(),
        template: template.name.clone(),
        project_dir: project_dir.to_path_buf(),
        created_at: now.to_rfc3339(),
        current_stage: 0,
        stages: effective_stages(template, project_dir)
            .into_iter()
            .map(|stage| ManifestStage {
                name: stage.name,
                status: StageStatus::Pending,
            })
            .collect(),
        abandoned_at: None,
    };
    write_manifest(&mission_dir, &manifest)?;

    Ok(ScaffoldedMission { slug, mission_dir })
}

fn write_manifest(mission_dir: &Path, manifest: &MissionManifest) -> anyhow::Result<()> {
    let contents = serde_json::to_string_pretty(manifest)?;
    fs::write(mission_dir.join(MANIFEST_FILE_NAME), contents)?;
    Ok(())
}

/// Updates the status of the stage at `stage_index` in the mission's
/// `manifest.json`, rewriting the file.
pub fn update_manifest_stage(
    mission_dir: &Path,
    stage_index: usize,
    status: StageStatus,
) -> anyhow::Result<()> {
    let manifest_path = mission_dir.join(MANIFEST_FILE_NAME);
    let contents = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read mission manifest {manifest_path:?}"))?;
    let mut manifest = serde_json::from_str::<MissionManifest>(&contents)?;
    let stage = manifest
        .stages
        .get_mut(stage_index)
        .with_context(|| format!("stage index {stage_index} out of bounds"))?;
    stage.status = status;
    if matches!(status, StageStatus::Running) {
        manifest.current_stage = stage_index;
    }
    write_manifest(mission_dir, &manifest)
}

/// Marks the mission as abandoned by stamping `abandoned_at` in its
/// `manifest.json`, leaving stage statuses as-is.
pub fn mark_mission_abandoned(mission_dir: &Path) -> anyhow::Result<()> {
    let manifest_path = mission_dir.join(MANIFEST_FILE_NAME);
    let contents = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read mission manifest {manifest_path:?}"))?;
    let mut manifest = serde_json::from_str::<MissionManifest>(&contents)?;
    manifest.abandoned_at = Some(chrono::Local::now().to_rfc3339());
    write_manifest(mission_dir, &manifest)
}

/// Renders a stage prompt by substituting the `{{briefing}}`, `{{mission_dir}}`,
/// and `{{profile}}` placeholders.
pub fn render_stage_prompt(
    stage_prompt: &str,
    briefing: &str,
    mission_dir: &Path,
    project_dir: &Path,
) -> String {
    stage_prompt
        .replace("{{briefing}}", briefing)
        .replace("{{mission_dir}}", &mission_dir.display().to_string())
        .replace(
            "{{profile}}",
            &profile_path(project_dir).display().to_string(),
        )
}

const ONBOARDING_PROMPT: &str = "Explore este projeto: identifique a stack, os comandos de build/teste/lint, a estrutura de diretórios e as convenções de código.\nEscreva um perfil conciso em {{profile}} que qualquer agente de código possa ler para trabalhar neste projeto.\nNão modifique nenhum outro arquivo.";

const ONBOARDING_GATE: &str = "Onboarding concluído: o perfil do projeto foi gerado em .cockpit/profile.md. Revise antes de iniciar a missão.";

/// Returns an extra "Onboarding" stage if the project has no
/// `.cockpit/profile.md` yet, so the first mission generates one.
pub fn onboarding_stage(project_dir: &Path) -> Option<MissionStage> {
    if profile_path(project_dir).exists() {
        return None;
    }
    Some(MissionStage {
        name: "Onboarding".to_string(),
        harness: None,
        prompt: ONBOARDING_PROMPT.to_string(),
        gate: Some(ONBOARDING_GATE.to_string()),
    })
}

/// The stages a mission will actually run: an optional onboarding stage
/// followed by the template's stages.
pub fn effective_stages(template: &MissionTemplate, project_dir: &Path) -> Vec<MissionStage> {
    onboarding_stage(project_dir)
        .into_iter()
        .chain(template.stages.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Spec-Driven"), "spec-driven");
        assert_eq!(slugify("Solo"), "solo");
        assert_eq!(slugify("  Minha Missão!  "), "minha-miss-o");
        assert_eq!(slugify("a__b--c"), "a-b-c");
        assert_eq!(slugify("---"), "");
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn test_render_stage_prompt() {
        let project_dir = Path::new("/tmp/project");
        let mission_dir = Path::new("/tmp/project/.cockpit/missions/solo-20260611-1200");
        let rendered = render_stage_prompt(
            "Briefing: {{briefing}}\nSpec: {{mission_dir}}/spec.md\nPerfil: {{profile}}",
            "fazer algo",
            mission_dir,
            project_dir,
        );
        assert_eq!(
            rendered,
            "Briefing: fazer algo\nSpec: /tmp/project/.cockpit/missions/solo-20260611-1200/spec.md\nPerfil: /tmp/project/.cockpit/profile.md"
        );
    }

    #[test]
    fn test_render_stage_prompt_no_placeholders() {
        let rendered = render_stage_prompt(
            "sem placeholders",
            "briefing",
            Path::new("/tmp/m"),
            Path::new("/tmp/p"),
        );
        assert_eq!(rendered, "sem placeholders");
    }
}
