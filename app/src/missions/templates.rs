//! Mission template schema and loader for Cockpit Missions.
//!
//! Templates are stored as YAML files in the Warp data directory
//! (`missions_dir()`). Builtin templates are written to disk on first load so
//! users can edit them.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::user_config::util::from_yaml;

/// A mission template: a named sequence of stages, each driven by a CLI agent
/// harness with an optional human gate between stages.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MissionTemplate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub stages: Vec<MissionStage>,
}

/// A single stage of a mission.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MissionStage {
    pub name: String,
    /// CLI harness: "claude" | "opencode" | "codex" | "agy". None = use mission default.
    #[serde(default)]
    pub harness: Option<String>,
    pub prompt: String,
    /// Gate text shown in the confirmation dialog before advancing PAST this stage.
    #[serde(default)]
    pub gate: Option<String>,
}

/// Directory where mission templates are stored.
pub fn missions_dir() -> PathBuf {
    warp_core::paths::data_dir().join("missions")
}

const SPEC_DRIVEN_FILE_NAME: &str = "spec-driven.yaml";
const SOLO_FILE_NAME: &str = "solo.yaml";

const SPEC_DRIVEN_TEMPLATE: &str = r#"name: "Spec-Driven"
description: "Arquiteto escreve a spec, implementador executa, revisor audita — com gates humanos entre estágios."
stages:
  - name: "Arquiteto"
    prompt: |
      Você é o arquiteto desta missão. NÃO escreva código de produção.
      Explore o projeto para entender a stack, a estrutura e as convenções existentes.
      Se o arquivo {{profile}} existir, leia-o antes de começar.

      A partir do briefing abaixo, escreva uma spec completa em {{mission_dir}}/spec.md contendo:
      - Objetivo
      - Critérios de aceite testáveis
      - Arquivos a criar/modificar, com justificativa
      - Casos de borda
      - Fora de escopo

      Ao terminar, resuma a spec no chat.

      Briefing: {{briefing}}
    gate: "O Arquiteto finalizou a spec ({{mission_dir}}/spec.md). Revise os critérios de aceite antes de aprovar a implementação."
  - name: "Implementador"
    prompt: |
      Implemente EXATAMENTE o que {{mission_dir}}/spec.md define.
      Siga as convenções de código existentes no projeto e não expanda o escopo.
      Atualize o checklist da spec conforme concluir cada item.
      Ao terminar, resuma o que mudou.
    gate: "Implementação concluída. Revise o diff antes de liberar a revisão final."
  - name: "Revisor"
    prompt: |
      Você é um revisor somente-leitura. NÃO corrija nada você mesmo.
      Compare o diff da árvore de trabalho com o que {{mission_dir}}/spec.md define.
      Rode os testes/lint do projeto, se disponíveis.
      Escreva {{mission_dir}}/review.md classificando os achados por severidade (critica/importante/aviso).
    gate: null
"#;

const SOLO_TEMPLATE: &str = r#"name: "Solo"
description: "Um único agente briefado executa a tarefa de ponta a ponta."
stages:
  - name: "Executor"
    prompt: |
      Leia {{profile}} se ele existir.
      Execute o briefing abaixo de ponta a ponta neste projeto, seguindo as convenções existentes.
      Explique suas decisões conforme avança.
      Ao final, escreva um resumo em {{mission_dir}}/log.md.

      Briefing: {{briefing}}
    gate: null
"#;

/// Writes the builtin templates to `dir` if they don't already exist. Never
/// overwrites existing files, so user edits are preserved.
fn ensure_builtin_templates(dir: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    for (file_name, contents) in [
        (SPEC_DRIVEN_FILE_NAME, SPEC_DRIVEN_TEMPLATE),
        (SOLO_FILE_NAME, SOLO_TEMPLATE),
    ] {
        let path = dir.join(file_name);
        if !path.exists() {
            fs::write(&path, contents)?;
        }
    }
    Ok(())
}

/// Loads all mission templates from `missions_dir()`, writing the builtin
/// templates first if they're absent. Templates that fail to parse are skipped
/// with a warning. The result is sorted by template name.
pub fn load_mission_templates() -> Vec<MissionTemplate> {
    let dir = missions_dir();
    if let Err(err) = ensure_builtin_templates(&dir) {
        log::warn!("Failed to write builtin mission templates to {dir:?}: {err:?}");
    }

    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) => {
            log::warn!("Failed to read missions directory {dir:?}: {err:?}");
            return Vec::new();
        }
    };

    let mut templates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let is_yaml = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension, "yaml" | "yml"));
        if !is_yaml {
            continue;
        }
        match from_yaml::<MissionTemplate>(path.clone()) {
            Ok(template) => templates.push(template),
            Err(err) => log::warn!("Failed to parse mission template at {path:?}: {err:?}"),
        }
    }
    templates.sort_by(|a, b| a.name.cmp(&b.name));
    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_templates_parse() {
        let spec_driven: MissionTemplate = serde_yaml::from_str(SPEC_DRIVEN_TEMPLATE).unwrap();
        assert_eq!(spec_driven.name, "Spec-Driven");
        assert_eq!(spec_driven.stages.len(), 3);
        assert!(spec_driven.stages[0].gate.is_some());
        assert!(spec_driven.stages[2].gate.is_none());

        let solo: MissionTemplate = serde_yaml::from_str(SOLO_TEMPLATE).unwrap();
        assert_eq!(solo.name, "Solo");
        assert_eq!(solo.stages.len(), 1);
        assert!(solo.stages[0].gate.is_none());
    }
}
