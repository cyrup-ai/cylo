// ============================================================================
// File: packages/cylo/src/backends/apple/execution.rs
// ----------------------------------------------------------------------------
// Container execution logic for Apple containerization backend.
// ============================================================================

use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::time::Instant;

use crate::AsyncTaskBuilder;
use crate::backends::{
    AsyncTask, BackendError, BackendResult, ExecutionRequest, ExecutionResult,
};

use super::resource_stats;

/// Execute code in Apple container
///
/// # Arguments
/// * `image` - Container image specification
/// * `request` - Execution request with code and configuration
///
/// # Returns
/// AsyncTask that resolves to execution result
pub(super) fn execute_in_container(
    image: String,
    request: ExecutionRequest,
) -> AsyncTask<BackendResult<ExecutionResult>> {
    AsyncTaskBuilder::new(async move {
        let start_time = Instant::now();

        // Create unique container name
        let container_name = format!(
            "cylo-{}-{}",
            uuid::Uuid::new_v4().simple(),
            std::process::id()
        );

        // Prepare execution command based on language
        let exec_cmd = prepare_execution_command(&request.language, &request.code)?;

        // Build container run command
        let mut cmd = Command::new("container");
        cmd.args(["run", "--rm", "--name", &container_name]);

        // Add resource limits
        if let Some(memory) = request.limits.max_memory {
            cmd.args(["--memory", &format!("{memory}b")]);
        }

        if let Some(cpu_time) = request.limits.max_cpu_time {
            cmd.args(["--cpus", &format!("{cpu_time}")]);
        }

        // Add environment variables
        for (key, value) in &request.env_vars {
            cmd.args(["-e", &format!("{key}={value}")]);
        }

        // Set working directory if specified
        if let Some(workdir) = &request.working_dir {
            cmd.args(["-w", workdir]);
        }

        // Add timeout handling
        cmd.args(["--timeout", &format!("{}s", request.timeout.as_secs())]);

        // Specify image and command
        cmd.arg(&image);
        cmd.args(&exec_cmd);

        // Set up stdio
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.stdin(Stdio::piped());

        // Execute the container
        let mut child = cmd.spawn().map_err(|e| BackendError::ProcessFailed {
            details: format!("Failed to spawn container: {e}"),
        })?;

        // Write input if provided
        if let Some(input) = &request.input
            && let Some(stdin) = child.stdin.take()
        {
            use std::io::Write;
            let mut stdin = stdin;
            stdin
                .write_all(input.as_bytes())
                .map_err(|e| BackendError::ProcessFailed {
                    details: format!("Failed to write to container stdin: {e}"),
                })?;
        }

        // Wait for completion with timeout
        let timeout_duration = request.timeout;

        // Use a different approach - spawn a task that can kill the process
        let child_handle = tokio::spawn(async move { child.wait_with_output() });

        let output = match tokio::time::timeout(timeout_duration, child_handle).await {
            Ok(Ok(Ok(output))) => output,
            Ok(Ok(Err(e))) => {
                return Err(BackendError::ProcessFailed {
                    details: format!("Container execution failed: {e}"),
                });
            }
            Ok(Err(_)) => {
                return Err(BackendError::ProcessFailed {
                    details: "Container process task failed".to_string(),
                });
            }
            Err(_) => {
                // Timeout occurred - the process is still running but we can't kill it
                // from here since it's been moved into the task
                return Err(BackendError::ExecutionTimeout {
                    seconds: timeout_duration.as_secs(),
                });
            }
        };

        let duration = start_time.elapsed();

        // Parse resource usage from container stats (if available)
        let resource_usage = resource_stats::parse_resource_usage(&container_name)
            .await
            .unwrap_or_default();

        Ok(ExecutionResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            duration,
            resource_usage,
            metadata: {
                let mut meta = HashMap::new();
                meta.insert("backend".to_string(), "Apple".to_string());
                meta.insert("image".to_string(), image);
                meta.insert("container_name".to_string(), container_name);
                meta
            },
        })
    })
    .spawn()
}

/// Prepare execution command for specific language
///
/// # Arguments
/// * `language` - Programming language
/// * `code` - Source code to execute
///
/// # Returns
/// Command arguments for container execution
pub(super) fn prepare_execution_command(language: &str, code: &str) -> BackendResult<Vec<String>> {
    match language.to_lowercase().as_str() {
        "python" | "python3" => Ok(vec![
            "python3".to_string(),
            "-c".to_string(),
            code.to_string(),
        ]),
        "javascript" | "js" | "node" => {
            Ok(vec!["node".to_string(), "-e".to_string(), code.to_string()])
        }
        "rust" => {
            // For Rust, we need to create a temporary file and compile
            Ok(vec![
                "sh".to_string(),
                "-c".to_string(),
                format!(
                    "echo '{}' > /tmp/main.rs && cd /tmp && rustc main.rs && ./main",
                    code.replace('\'', "'\"'\"'")
                ),
            ])
        }
        "bash" | "sh" => Ok(vec!["sh".to_string(), "-c".to_string(), code.to_string()]),
        "go" => Ok(vec![
            "sh".to_string(),
            "-c".to_string(),
            format!(
                "echo '{}' > /tmp/main.go && cd /tmp && go run main.go",
                code.replace('\'', "'\"'\"'")
            ),
        ]),
        _ => Err(BackendError::UnsupportedLanguage {
            backend: "Apple",
            language: language.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_command_preparation() {
        let python_cmd = prepare_execution_command("python", "print('hello')")
            .expect("test should successfully prepare python execution command");
        assert_eq!(python_cmd, vec!["python3", "-c", "print('hello')"]);

        let js_cmd = prepare_execution_command("javascript", "console.log('hello')")
            .expect("test should successfully prepare javascript execution command");
        assert_eq!(js_cmd, vec!["node", "-e", "console.log('hello')"]);

        let bash_cmd = prepare_execution_command("bash", "echo hello")
            .expect("test should successfully prepare bash execution command");
        assert_eq!(bash_cmd, vec!["sh", "-c", "echo hello"]);

        let unsupported = prepare_execution_command("cobol", "some code");
        assert!(unsupported.is_err());
    }
}
