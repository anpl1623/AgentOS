//! Subprocess execution.
//!
//! # Why there is no shell
//!
//! `terminal.exec` takes a program and an argument vector and executes them
//! directly. It does not go through `sh -c`. That single decision removes an
//! entire class of problem: there is no word splitting, no glob expansion, no
//! `$(...)`, no `;`, no `&&`, no redirection. An argument containing
//! `; rm -rf ~` is passed to the program as a literal string, because that is
//! what it is.
//!
//! This matters most when the arguments came, however indirectly, from a
//! webpage the agent read. Shell metacharacter escaping is a losing game; not
//! invoking a shell is not.
//!
//! On top of that: the program itself is a policy-checked resource, the working
//! directory is a policy-checked path, the environment is an allowlist rather
//! than the parent's, output is capped, and the process is killed on timeout or
//! cancellation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use agentos_core::permission::{Capability, ResourceRef, permission_domains};
use agentos_core::risk::RiskLevel;
use agentos_core::tool::ToolMetadata;
use agentos_core::trust::DataSource;
use agentos_permissions::path::{expand_home, resolve_secure};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::error::ToolError;
use crate::tool::{Tool, ToolContext, ToolOutput, ToolPlan, metadata_for, parse_arguments};

/// Environment variables passed through to child processes.
///
/// An allowlist, not the parent environment. Inheriting everything would leak
/// whatever credentials happen to be exported into the AgentOS process, and
/// those are exactly the things an injected command would go looking for.
pub const ENV_ALLOWLIST: &[&str] = &["PATH", "HOME", "LANG", "LC_ALL", "TZ", "TMPDIR"];

/// Longest a command may run without an explicit override.
pub const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest a command may run with an explicit override.
pub const MAX_COMMAND_TIMEOUT_SECS: u64 = 600;

/// Cap on captured stdout/stderr, per stream.
pub const MAX_CAPTURED_BYTES: usize = 256 * 1024;

/// Program extensions that are refused outright.
///
/// On Windows, `CreateProcess` runs a `.bat` or `.cmd` file by handing it to
/// `cmd.exe` — so for exactly these files, and only these, the argument vector
/// *is* reinterpreted by a shell after AgentOS has finished with it. The
/// escaping rules there are notoriously subtle and have produced CVEs in
/// language runtimes, Rust's included.
///
/// The whole reason `terminal.exec` takes an argv is that no shell should see
/// it, so batch files are refused rather than special-cased. An operator who
/// genuinely needs one can invoke `cmd.exe /c script.bat` explicitly, which at
/// least makes the shell visible in the policy and in the audit log instead of
/// implicit in a file extension.
///
/// Refused on every platform, not just Windows, so a policy behaves the same
/// wherever it is authored.
pub const REFUSED_EXTENSIONS: &[&str] = &["bat", "cmd"];

/// Arguments for `terminal.exec`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExecArgs {
    /// The program to run. Not a shell command line — no shell is involved.
    pub program: String,
    /// Arguments, passed to the program verbatim.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory. Defaults to the agent's workspace.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Seconds to allow before the process is killed.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Text to write to the process's standard input.
    #[serde(default)]
    pub stdin: Option<String>,
}

/// Runs a program.
#[derive(Debug)]
pub struct ExecuteCommand(ToolMetadata);

impl Default for ExecuteCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecuteCommand {
    /// Build the tool.
    #[must_use]
    pub fn new() -> Self {
        Self(metadata_for::<ExecArgs>(
            "terminal.exec",
            "Run a program directly with an argument vector and return its exit code, stdout \
             and stderr. No shell is involved: pipes, redirection, globs, `&&` and `;` are not \
             interpreted, and metacharacters in arguments are passed through literally. To \
             chain commands, run them one at a time.",
            RiskLevel::High,
            vec![Capability::new(permission_domains::TERMINAL, "exec")],
            true,
        ))
    }

    fn working_directory(args: &ExecArgs, context: &ToolContext) -> Result<PathBuf, ToolError> {
        match &args.cwd {
            None => Ok(context.workspace.clone()),
            Some(raw) => {
                let expanded = expand_home(raw).ok_or_else(|| {
                    ToolError::Failed(format!("cannot expand `~` in `{raw}`: no home directory"))
                })?;
                let absolute = if expanded.is_absolute() {
                    expanded
                } else {
                    context.workspace.join(expanded)
                };
                resolve_secure(&absolute).map_err(ToolError::Path)
            }
        }
    }

    fn timeout(args: &ExecArgs, context: &ToolContext) -> Duration {
        let requested = args
            .timeout_secs
            .map_or(DEFAULT_COMMAND_TIMEOUT, Duration::from_secs);
        // A model-supplied timeout may shorten the budget but never extend it
        // past the pipeline's own limit or the hard ceiling.
        requested
            .min(Duration::from_secs(MAX_COMMAND_TIMEOUT_SECS))
            .min(context.timeout)
    }
}

#[async_trait]
impl Tool for ExecuteCommand {
    fn metadata(&self) -> &ToolMetadata {
        &self.0
    }

    fn validate(&self, arguments: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: ExecArgs = parse_arguments(&self.0.name, arguments)?;
        if args.program.trim().is_empty() {
            return Err(ToolError::invalid(
                &self.0.name,
                "`program` must not be empty",
            ));
        }
        // A program name containing a NUL cannot be executed and is a sign of
        // something trying to confuse the argument parser.
        if args.program.contains('\0') || args.args.iter().any(|arg| arg.contains('\0')) {
            return Err(ToolError::invalid(
                &self.0.name,
                "arguments must not contain NUL bytes",
            ));
        }
        if let Some(extension) = batch_extension(&args.program) {
            return Err(ToolError::invalid(
                &self.0.name,
                format!(
                    "refusing to run `{}`: a .{extension} file is executed by passing it to \
                     cmd.exe, so its arguments go through a shell. Invoke `cmd.exe /c \
                     {}` explicitly if that is really what you want.",
                    args.program, args.program
                ),
            ));
        }
        Ok(arguments.clone())
    }

    async fn plan(
        &self,
        arguments: &serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolPlan, ToolError> {
        let args: ExecArgs = parse_arguments(&self.0.name, arguments)?;
        let cwd = Self::working_directory(&args, context)?;

        let rendered = if args.args.is_empty() {
            args.program.clone()
        } else {
            format!("{} {}", args.program, args.args.join(" "))
        };

        // Two capabilities: which program may run, and where it may run. A
        // policy can allow `git` without allowing it to run anywhere on disk.
        Ok(ToolPlan::new(
            RiskLevel::High,
            format!("Run `{rendered}` in {}", cwd.display()),
        )
        .requiring(
            Capability::new(permission_domains::TERMINAL, "exec").with_resource(
                ResourceRef::Program {
                    program: args.program.clone(),
                },
            ),
        )
        .requiring(
            Capability::new(permission_domains::FILESYSTEM, "read").with_resource(
                ResourceRef::Path {
                    path: cwd.display().to_string(),
                },
            ),
        ))
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
        context: &ToolContext,
        cancel: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let args: ExecArgs = parse_arguments(&self.0.name, &arguments)?;
        let cwd = Self::working_directory(&args, context)?;
        let timeout = Self::timeout(&args, context);

        let environment: HashMap<&str, String> = ENV_ALLOWLIST
            .iter()
            .filter_map(|key| std::env::var(key).ok().map(|value| (*key, value)))
            .collect();

        let mut command = tokio::process::Command::new(&args.program);
        command
            .args(&args.args)
            .current_dir(&cwd)
            .env_clear()
            .envs(&environment)
            .stdin(if args.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Without this the child survives its parent being killed.
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .map_err(|source| ToolError::io(format!("starting `{}`", args.program), source))?;

        if let Some(input) = &args.stdin
            && let Some(mut pipe) = child.stdin.take()
        {
            pipe.write_all(input.as_bytes())
                .await
                .map_err(|source| ToolError::io("writing to stdin", source))?;
            // Dropping closes the pipe, which many programs wait for.
            drop(pipe);
        }

        let output = tokio::select! {
            biased;

            // Returning here drops the branch that owns the child, and
            // `kill_on_drop` reaps it. There is no path that leaves an orphan.
            () = cancel.cancelled() => return Err(ToolError::Cancelled),
            result = tokio::time::timeout(timeout, child.wait_with_output()) => match result {
                Err(_elapsed) => {
                    // `kill_on_drop` handles the child; the future was dropped
                    // by the timeout, so the process is already going away.
                    return Err(ToolError::TimedOut {
                        tool: self.0.name.clone(),
                        seconds: timeout.as_secs(),
                    });
                }
                Ok(Err(source)) => {
                    return Err(ToolError::io(format!("running `{}`", args.program), source));
                }
                Ok(Ok(output)) => output,
            },
        };

        let exit_code = output.status.code();
        let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
        let stderr = truncate(&String::from_utf8_lossy(&output.stderr));

        let mut body = String::new();
        body.push_str(&format!(
            "exit code: {}\n",
            exit_code.map_or_else(|| "signal".to_owned(), |code| code.to_string())
        ));
        if !stdout.is_empty() {
            body.push_str(&format!("\n--- stdout ---\n{stdout}"));
        }
        if !stderr.is_empty() {
            body.push_str(&format!("\n--- stderr ---\n{stderr}"));
        }

        Ok(ToolOutput::text(
            DataSource::Terminal {
                program: args.program.clone(),
            },
            body,
        )
        .with_structured(serde_json::json!({
            "program": args.program,
            "args": args.args,
            "cwd": cwd.display().to_string(),
            "exit_code": exit_code,
            "stdout": stdout,
            "stderr": stderr,
        })))
    }
}

/// The refused extension of a program path, if it has one.
fn batch_extension(program: &str) -> Option<&'static str> {
    let lowered = program.to_ascii_lowercase();
    REFUSED_EXTENSIONS
        .iter()
        .find(|extension| lowered.ends_with(&format!(".{extension}")))
        .copied()
}

fn truncate(text: &str) -> String {
    if text.len() <= MAX_CAPTURED_BYTES {
        return text.to_owned();
    }
    let mut cut = MAX_CAPTURED_BYTES;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… [{} bytes truncated]", &text[..cut], text.len() - cut)
}

/// Every terminal tool, ready to register.
#[must_use]
pub fn all() -> Vec<std::sync::Arc<dyn Tool>> {
    vec![std::sync::Arc::new(ExecuteCommand::new())]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_files_are_refused_on_every_platform() {
        let tool = ExecuteCommand::new();
        for program in [
            "deploy.bat",
            "DEPLOY.BAT",
            r"C:\scripts\build.cmd",
            "/opt/tools/run.Cmd",
        ] {
            let error = tool
                .validate(&serde_json::json!({"program": program}))
                .unwrap_err();
            assert!(
                error.to_string().contains("through a shell"),
                "`{program}` should be refused, got: {error}"
            );
        }
    }

    #[test]
    fn ordinary_programs_are_accepted() {
        let tool = ExecuteCommand::new();
        for program in [
            "git",
            "/usr/bin/env",
            r"C:\Windows\System32\where.exe",
            "batch",
        ] {
            assert!(
                tool.validate(&serde_json::json!({"program": program}))
                    .is_ok(),
                "`{program}` should be accepted"
            );
        }
    }

    #[test]
    fn empty_and_nul_bearing_arguments_are_refused() {
        let tool = ExecuteCommand::new();
        assert!(
            tool.validate(&serde_json::json!({"program": "  "}))
                .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({"program": "git\u{0}x"}))
                .is_err()
        );
        assert!(
            tool.validate(&serde_json::json!({"program": "git", "args": ["a\u{0}b"]}))
                .is_err()
        );
    }
}
