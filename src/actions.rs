use crate::errors::{AppError, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info, instrument};

#[instrument(skip(command_template), fields(command = %command_template, path = %path.display()))]
pub async fn execute_action(command_template: &str, path: &Path) -> Result<()> {
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::PathNonUtf8(path.to_path_buf()))?;

    let command_to_run = command_template.replace("{}", path_str);

    if command_to_run.trim().is_empty() {
        return Err(AppError::EmptyCommand {
            event_kind: command_to_run,
            path: path.to_path_buf(),
        });
    }

    info!("Executing action");
    debug!("Running command: {}", command_to_run);

    let mut command = if cfg!(target_os = "windows") {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", &command_to_run]);
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", &command_to_run]);
        cmd
    };

    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let output = command.output().await.map_err(|e| AppError::ActionExec {
        command: command_to_run.clone(),
        source: e,
    })?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !stdout.trim().is_empty() {
            debug!(output = %stdout.trim(), "Command executed successfully");
        } else {
            debug!("Command executed successfully (no output)");
        }
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.trim().is_empty() {
            debug!(stderr = %stderr.trim(), "Command stderr output");
        }
        Err(AppError::ActionExec {
            command: command_to_run,
            source: std::io::Error::new(std::io::ErrorKind::Other, "Command failed"),
        })
    }
}
