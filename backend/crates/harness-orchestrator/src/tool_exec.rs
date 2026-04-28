//! External-tool execution: spawns a sandbox-wrapped command, streams
//! stdout/stderr lines as `RunEvent::Tool{Stdout,Stderr}`, and emits the
//! terminal `ToolFinish` / `ToolError` event.

use harness_core::{ContentBlock, RunEvent, WrappedCommand};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(crate) async fn run_external(
    tool_use_id: &str,
    tool_name: &str,
    cmd: WrappedCommand,
    cancel: &CancellationToken,
    tx: &mpsc::Sender<RunEvent>,
) -> ContentBlock {
    let mut process = Command::new(&cmd.program);
    process
        .args(&cmd.args)
        .envs(cmd.env.iter())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    if let Some(cwd) = &cmd.cwd {
        process.current_dir(cwd);
    }

    let mut child = match process.spawn() {
        Ok(c) => c,
        Err(err) => {
            return crate::emit_tool_failure(
                tx,
                tool_use_id,
                tool_name,
                "tool.other",
                &format!("failed to spawn '{}': {}", cmd.program, err),
            )
            .await;
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let (stdout_send, mut stdout_buf) = (tx.clone(), String::new());
    let (stderr_send, mut stderr_buf) = (tx.clone(), String::new());
    let id_for_stdout = tool_use_id.to_string();
    let id_for_stderr = tool_use_id.to_string();

    let stdout_task = stdout.map(|s| {
        tokio::spawn(async move {
            let mut reader = BufReader::new(s).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if !stdout_buf.is_empty() {
                    stdout_buf.push('\n');
                }
                stdout_buf.push_str(&line);
                let _ = stdout_send
                    .send(RunEvent::ToolStdout {
                        tool_use_id: id_for_stdout.clone(),
                        chunk: format!("{}\n", line),
                    })
                    .await;
            }
            stdout_buf
        })
    });

    let stderr_task = stderr.map(|s| {
        tokio::spawn(async move {
            let mut reader = BufReader::new(s).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if !stderr_buf.is_empty() {
                    stderr_buf.push('\n');
                }
                stderr_buf.push_str(&line);
                let _ = stderr_send
                    .send(RunEvent::ToolStderr {
                        tool_use_id: id_for_stderr.clone(),
                        chunk: format!("{}\n", line),
                    })
                    .await;
            }
            stderr_buf
        })
    });

    let exit_status = tokio::select! {
        _ = cancel.cancelled() => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return crate::emit_tool_failure(
                tx,
                tool_use_id,
                tool_name,
                "tool.cancelled",
                &format!("tool '{}' cancelled", tool_name),
            ).await;
        }
        status = child.wait() => status,
    };

    let stdout_text = match stdout_task {
        Some(h) => h.await.unwrap_or_default(),
        None => String::new(),
    };
    let stderr_text = match stderr_task {
        Some(h) => h.await.unwrap_or_default(),
        None => String::new(),
    };

    match exit_status {
        Ok(s) if s.success() => {
            let output = serde_json::Value::String(stdout_text);
            let _ = tx
                .send(RunEvent::ToolFinish {
                    tool_use_id: tool_use_id.to_string(),
                    name: tool_name.to_string(),
                    output: output.clone(),
                })
                .await;
            ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                is_error: false,
                content: output,
            }
        }
        Ok(s) => {
            let code = s.code().unwrap_or(-1);
            crate::emit_tool_failure(
                tx,
                tool_use_id,
                tool_name,
                "tool.non_zero_exit",
                &format!(
                    "tool '{}' exited with status {}: {}",
                    tool_name, code, stderr_text
                ),
            )
            .await
        }
        Err(err) => {
            crate::emit_tool_failure(
                tx,
                tool_use_id,
                tool_name,
                "tool.other",
                &format!("wait failed: {}", err),
            )
            .await
        }
    }
}
