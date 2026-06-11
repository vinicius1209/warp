use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use parking_lot::Mutex;
use tempfile::NamedTempFile;
use warp_cli::agent::Harness;
use warp_managed_secrets::ManagedSecretValue;
use warpui::{ModelHandle, ModelSpawner};

use super::super::terminal::{CommandHandle, TerminalDriver};
use super::super::{AgentDriver, AgentDriverError};
use super::{
    write_temp_file, HarnessCleanupDisposition, HarnessRunner, JSONMCPServer, ResumePayload,
    SavePoint, ThirdPartyHarness,
};
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent_sdk::setup_observability::{
    OzRunTimelineEvent, SetupClientEventReporter, SetupStep,
};
use crate::ai::ambient_agents::task::HarnessModelConfig;
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::server::server_api::harness_support::HarnessSupportClient;
use crate::server::server_api::ServerApi;
use crate::terminal::model::block::BlockId;
use crate::terminal::CLIAgent;

/// Harness for the Antigravity CLI (`agy`), Google's successor to Gemini CLI.
///
/// Unlike the Gemini harness, this does not write any config files: agy
/// manages its own auth via OS-keyring OAuth and stores settings under
/// `~/.gemini/antigravity-cli/`, whose schema is not stable/documented.
/// Workspace trust is prompted by agy itself on first run in a directory.
pub(crate) struct AgyHarness;

/// Format slug sent to the server when creating an Antigravity conversation.
const AGY_CLI_FORMAT: &str = "antigravity_cli";
/// Slash command agy's TUI recognises as a graceful shutdown.
const AGY_EXIT_COMMAND: &str = "/quit";

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl ThirdPartyHarness for AgyHarness {
    fn harness(&self) -> Harness {
        Harness::Agy
    }

    fn cli_agent(&self) -> CLIAgent {
        CLIAgent::Agy
    }

    fn install_docs_url(&self) -> Option<&'static str> {
        Some("https://antigravity.google/docs/cli-getting-started")
    }

    fn build_runner(
        &self,
        prompt: &str,
        _system_prompt: Option<&str>,
        _resumption_prompt: Option<&str>,
        context: Option<&str>,
        _working_dir: &Path,
        _task_id: Option<AmbientAgentTaskId>,
        server_api: Arc<ServerApi>,
        terminal_driver: ModelHandle<TerminalDriver>,
        _resume: Option<ResumePayload>,
        _resolved_env_vars: &HashMap<OsString, OsString>,
        _resolved_secrets: &HashMap<String, ManagedSecretValue>,
        _resolved_mcp_servers: &HashMap<String, JSONMCPServer>,
        _third_party_harness_model_config: Option<&HarnessModelConfig>,
    ) -> Result<Box<dyn HarnessRunner>, AgentDriverError> {
        // agy does not support conversation resume from a stored transcript yet.
        // Prepend server context to the prompt if available.
        let effective_prompt = match context {
            Some(ctx) if !ctx.is_empty() => format!("{ctx}\n\n{prompt}"),
            _ => prompt.to_string(),
        };
        let client: Arc<dyn HarnessSupportClient> = server_api;
        Ok(Box::new(AgyHarnessRunner::new(
            self.cli_agent().command_prefix(),
            &effective_prompt,
            client,
            terminal_driver,
        )?))
    }
}

/// Build the shell command that launches the agy TUI.
///
/// `--dangerously-skip-permissions` auto-approves tool calls (agy's
/// replacement for Gemini CLI's `--yolo`). `-i` seeds the initial prompt and
/// continues in interactive TUI mode.
fn agy_command(cli_name: &str, prompt_path: &str) -> String {
    format!("{cli_name} --dangerously-skip-permissions -i \"$(cat '{prompt_path}')\"")
}

enum AgyRunnerState {
    Preexec,
    Running {
        /// `None` when the external conversation could not be registered on
        /// the server (e.g. running without a Warp login). The harness still
        /// runs; conversation snapshots are skipped.
        conversation_id: Option<AIConversationId>,
        block_id: BlockId,
    },
}

struct AgyHarnessRunner {
    command: String,
    /// The CLI name used to invoke agy.
    cli_name: String,
    /// Held so the temp file is cleaned up when the runner is dropped.
    _temp_prompt_file: NamedTempFile,
    client: Arc<dyn HarnessSupportClient>,
    terminal_driver: ModelHandle<TerminalDriver>,
    state: Mutex<AgyRunnerState>,
}

impl AgyHarnessRunner {
    fn new(
        cli_command: &str,
        prompt: &str,
        client: Arc<dyn HarnessSupportClient>,
        terminal_driver: ModelHandle<TerminalDriver>,
    ) -> Result<Self, AgentDriverError> {
        let temp_file = write_temp_file("oz_prompt_", prompt, ".txt")?;
        let prompt_path = temp_file.path().display().to_string();

        Ok(Self {
            command: agy_command(cli_command, &prompt_path),
            cli_name: cli_command.to_string(),
            _temp_prompt_file: temp_file,
            client,
            terminal_driver,
            state: Mutex::new(AgyRunnerState::Preexec),
        })
    }
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl HarnessRunner for AgyHarnessRunner {
    fn harness_name(&self) -> &str {
        &self.cli_name
    }

    async fn start(
        &self,
        foreground: &ModelSpawner<AgentDriver>,
        setup_events: &SetupClientEventReporter,
    ) -> Result<CommandHandle, AgentDriverError> {
        // Try to create the external conversation record on the server. Unlike
        // other harnesses this is non-fatal: agy authenticates with the user's
        // Google account, so it can run without a Warp login — in that case we
        // skip server-side conversation tracking instead of refusing to launch.
        let conversation_id = setup_events
            .record_result(SetupStep::ThirdPartyHarnessExternalConversation, async {
                Ok::<_, AgentDriverError>(
                    self.client
                        .create_external_conversation(AGY_CLI_FORMAT)
                        .await
                        .map_err(|e| {
                            log::warn!(
                                "Failed to create external conversation for agy; \
                             continuing without conversation tracking: {e}"
                            );
                        })
                        .ok(),
                )
            })
            .await?;
        if let Some(id) = &conversation_id {
            log::info!("Created external conversation {id}");
        }

        let command = self.command.clone();
        let terminal_driver = self.terminal_driver.clone();
        let command_handle = foreground
            .spawn(move |_, ctx| {
                terminal_driver.update(ctx, |driver, ctx| driver.execute_command(&command, ctx))
            })
            .await??
            .await?;

        // Only store conversation info once the CLI command has started.
        *self.state.lock() = AgyRunnerState::Running {
            conversation_id,
            block_id: command_handle.block_id().clone(),
        };

        setup_events
            .post_timeline_event(OzRunTimelineEvent::AgentStarted)
            .await;

        Ok(command_handle)
    }

    async fn exit(&self, foreground: &ModelSpawner<AgentDriver>) -> Result<()> {
        log::info!("Sending /quit to agy CLI");
        let terminal_driver = self.terminal_driver.clone();
        foreground
            .spawn(move |_, ctx| {
                terminal_driver.update(ctx, |driver, ctx| {
                    driver.send_text_to_cli(AGY_EXIT_COMMAND.to_string(), ctx);
                });
            })
            .await
            .map_err(|_| anyhow::anyhow!("Agent driver dropped while sending /quit"))
    }

    async fn save_conversation(
        &self,
        save_point: SavePoint,
        foreground: &ModelSpawner<AgentDriver>,
    ) -> Result<()> {
        if matches!(save_point, SavePoint::Periodic)
            && !super::has_running_cli_agent(&self.terminal_driver, foreground).await
        {
            log::debug!("Will not save conversation, agy not in progress");
            return Ok(());
        }

        let (conversation_id, block_id) = match &*self.state.lock() {
            AgyRunnerState::Preexec => {
                log::warn!("save_conversation called before start");
                return Ok(());
            }
            AgyRunnerState::Running {
                conversation_id: None,
                ..
            } => {
                // No server-side conversation (e.g. logged-out run): nothing to save.
                return Ok(());
            }
            AgyRunnerState::Running {
                conversation_id: Some(conversation_id),
                block_id,
            } => (*conversation_id, block_id.clone()),
        };

        super::upload_current_block_snapshot(
            foreground,
            &self.terminal_driver,
            self.client.as_ref(),
            conversation_id,
            block_id,
        )
        .await
    }

    async fn cleanup(
        &self,
        _cleanup_disposition: HarnessCleanupDisposition,
        _foreground: &ModelSpawner<AgentDriver>,
    ) -> Result<()> {
        Ok(())
    }
}
