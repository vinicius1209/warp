//! Mission harness pre-flight checks.
//!
//! Before a mission launches, every harness it will use must actually be
//! installed and on `PATH`. Otherwise the stage tab spawns a CLI command that
//! the user can't see fail, and the mission silently dies in a terminal a
//! non-developer can't parse (the #1 silent-failure mode, "Gap C" in
//! `.cockpit/discoveries/vibe-coder-ux.md`).
//!
//! This module maps a harness config-name (e.g. `"claude"`) to its CLI command
//! and detects whether that command resolves on `PATH` — reusing the same
//! [`resolve_executable`] machinery the agent driver and local-harness setup
//! already use (`crate::ai::local_harness_setup::local_cli_is_installed`,
//! `crate::ai::agent_sdk::driver::harness::validate_cli_installed`). When a
//! harness is missing, it returns a short, human install hint built from the
//! `install_docs_url` each harness driver already declares.

use warp_cli::agent::Harness;

#[cfg(not(target_family = "wasm"))]
use crate::util::path::resolve_executable;

/// Preference order used to auto-select a mission's default harness. We pick
/// the first one of these that is actually installed. Claude leads because the
/// builtin templates and resume flow (`claude --continue`) are written around
/// it; the rest follow as reasonable fallbacks.
pub const DEFAULT_HARNESS_PREFERENCE: &[Harness] = &[
    Harness::Claude,
    Harness::Codex,
    Harness::OpenCode,
    Harness::Agy,
];

/// Whether a harness is runnable locally for missions, and if not, how to fix
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HarnessAvailability {
    /// The harness's CLI resolves on `PATH`.
    Installed,
    /// The template names a harness this mission launcher cannot run.
    Unsupported {
        /// The unsupported harness value from the template.
        harness: String,
        /// A friendly, one-line explanation.
        message: String,
    },
    /// The harness's CLI is not on `PATH`. Carries the command we looked for and
    /// a short, plain-language hint for installing it.
    NotInstalled {
        /// The CLI command we tried to resolve (e.g. `"claude"`).
        command: String,
        /// A friendly, one-line install hint (e.g. an install URL).
        install_hint: String,
    },
}

/// The CLI command used to launch a harness, matching the command prefixes the
/// stage tabs actually run (see `CLIAgent::command_prefix`). Returns `None` for
/// harnesses that aren't runnable as a mission stage: Oz runs in-process,
/// Unknown is a future-server placeholder, and Gemini is excluded from the
/// local-child set (`parse_local_child_harness`), so a mission never gates on
/// or launches it.
fn harness_command(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("claude"),
        Harness::Codex => Some("codex"),
        Harness::OpenCode => Some("opencode"),
        Harness::Agy => Some("agy"),
        Harness::Gemini | Harness::Oz | Harness::Unknown => None,
    }
}

fn supported_harnesses_hint() -> String {
    DEFAULT_HARNESS_PREFERENCE
        .iter()
        .map(|harness| harness.config_name())
        .collect::<Vec<_>>()
        .join(", ")
}

fn unsupported_harness_message(harness: &str, parsed: Option<Harness>) -> String {
    let supported = supported_harnesses_hint();
    let harness = harness.trim();
    if harness.is_empty() {
        return format!(
            "A mission stage has an empty harness name. Supported mission harnesses: {supported}."
        );
    }

    if let Some(parsed) = parsed {
        return format!(
            "{} isn't supported for Cockpit missions yet. Supported mission harnesses: {supported}.",
            parsed.display_name()
        );
    }

    format!("Unsupported mission harness '{harness}'. Supported mission harnesses: {supported}.")
}

/// A short, human install hint per harness — the `install_docs_url` each harness
/// driver declares in `app/src/ai/agent_sdk/driver/harness/*.rs`, kept in sync
/// here so this module has no dependency on the (crate-private) harness traits.
fn harness_install_hint(harness: Harness) -> String {
    let url = match harness {
        Harness::Claude => "https://code.claude.com/docs/en/quickstart",
        Harness::Codex => "https://developers.openai.com/codex/cli",
        Harness::OpenCode => "https://opencode.ai/docs",
        Harness::Agy => "https://antigravity.google/docs/cli-getting-started",
        Harness::Gemini | Harness::Oz | Harness::Unknown => "",
    };
    format!("Install it: {url}")
}

/// Returns whether a CLI command resolves on the process's `PATH`. Mirrors
/// `crate::ai::local_harness_setup::local_cli_is_installed`.
#[cfg(not(target_family = "wasm"))]
fn cli_is_installed(command: &str) -> bool {
    resolve_executable(command).is_some()
}

#[cfg(target_family = "wasm")]
fn cli_is_installed(_command: &str) -> bool {
    false
}

/// Checks whether the named harness can run locally for a mission.
///
/// `harness` is the mission's stage/default harness string (a
/// [`Harness::config_name`], e.g. `"claude"`). Unrecognized names and harnesses
/// with no local mission command are blocked before scaffolding so a bad
/// template cannot leave behind an orphaned `.cockpit` mission directory.
pub fn check_harness(harness: &str) -> HarnessAvailability {
    check_harness_with(harness, cli_is_installed)
}

fn check_harness_with(
    harness: &str,
    cli_is_installed: impl Fn(&str) -> bool,
) -> HarnessAvailability {
    let Some(parsed) = Harness::from_config_name(harness) else {
        return HarnessAvailability::Unsupported {
            harness: harness.trim().to_string(),
            message: unsupported_harness_message(harness, None),
        };
    };
    let Some(command) = harness_command(parsed) else {
        return HarnessAvailability::Unsupported {
            harness: harness.trim().to_string(),
            message: unsupported_harness_message(harness, Some(parsed)),
        };
    };
    if cli_is_installed(command) {
        HarnessAvailability::Installed
    } else {
        HarnessAvailability::NotInstalled {
            command: command.to_string(),
            install_hint: harness_install_hint(parsed),
        }
    }
}

/// The distinct set of harnesses a mission's stages will actually run, in stage
/// order, deduplicated. Each stage's effective harness is its own
/// `stage.harness` when set, otherwise `default_harness`.
pub fn required_harnesses(
    stages: &[crate::missions::MissionStage],
    default_harness: &str,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for stage in stages {
        let effective = stage.harness.as_deref().unwrap_or(default_harness);
        if !out.iter().any(|h| h == effective) {
            out.push(effective.to_string());
        }
    }
    out
}

/// Picks a mission default harness: the first installed harness in
/// [`DEFAULT_HARNESS_PREFERENCE`]. Falls back to `"claude"` when none are
/// installed, so the pre-flight gate fires with a useful "install Claude Code"
/// hint instead of silently launching into a broken CLI.
pub fn pick_default_harness() -> String {
    for harness in DEFAULT_HARNESS_PREFERENCE {
        if let Some(command) = harness_command(*harness) {
            if cli_is_installed(command) {
                return harness.config_name().to_string();
            }
        }
    }
    Harness::Claude.config_name().to_string()
}

#[cfg(all(test, not(target_family = "wasm")))]
#[path = "preflight_tests.rs"]
mod tests;
