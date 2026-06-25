use super::*;
use crate::missions::MissionStage;

fn stage(name: &str, harness: Option<&str>) -> MissionStage {
    MissionStage {
        name: name.to_string(),
        harness: harness.map(str::to_string),
        prompt: String::new(),
        gate: None,
    }
}

#[test]
fn check_harness_reports_supported_installed_harness() {
    assert_eq!(
        check_harness_with("claude", |_| true),
        HarnessAvailability::Installed
    );
}

#[test]
fn check_harness_reports_supported_missing_harness() {
    assert_eq!(
        check_harness_with("agy", |_| false),
        HarnessAvailability::NotInstalled {
            command: "agy".to_string(),
            install_hint: "Install it: https://antigravity.google/docs/cli-getting-started"
                .to_string(),
        }
    );
}

#[test]
fn check_harness_blocks_unknown_harness() {
    let availability = check_harness_with("fake", |_| true);
    match availability {
        HarnessAvailability::Unsupported { harness, message } => {
            assert_eq!(harness, "fake");
            assert!(message.contains("Unsupported mission harness 'fake'"));
            assert!(message.contains("claude, codex, opencode, agy"));
        }
        other => panic!("expected unsupported harness, got {other:?}"),
    }
}

#[test]
fn check_harness_blocks_known_non_mission_harnesses() {
    for harness in ["oz", "gemini", "unknown"] {
        let availability = check_harness_with(harness, |_| true);
        match availability {
            HarnessAvailability::Unsupported { message, .. } => {
                assert!(message.contains("isn't supported for Cockpit missions yet"));
            }
            other => panic!("expected unsupported harness for {harness}, got {other:?}"),
        }
    }
}

#[test]
fn check_harness_blocks_empty_harness() {
    let availability = check_harness_with("  ", |_| true);
    match availability {
        HarnessAvailability::Unsupported { harness, message } => {
            assert_eq!(harness, "");
            assert!(message.contains("empty harness name"));
        }
        other => panic!("expected unsupported harness, got {other:?}"),
    }
}

#[test]
fn check_harness_treats_definitely_absent_command_as_not_installed() {
    assert!(!cli_is_installed(
        "warp-cockpit-definitely-not-a-real-binary-xyz"
    ));
}

#[test]
fn required_harnesses_dedups_and_falls_back_to_default() {
    let stages = vec![
        stage("onboarding", None),
        stage("architect", Some("claude")),
        stage("impl", Some("codex")),
        stage("review", Some("claude")),
    ];
    assert_eq!(
        required_harnesses(&stages, "claude"),
        vec!["claude".to_string(), "codex".to_string()]
    );
}

#[test]
fn pick_default_harness_returns_a_known_config_name() {
    let picked = pick_default_harness();
    assert!(
        Harness::from_config_name(&picked).is_some(),
        "pick_default_harness returned an unknown config name: {picked}"
    );
}
