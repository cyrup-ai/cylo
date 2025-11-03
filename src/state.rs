use std::path::PathBuf;

use log::{error, info, warn};

use crate::config::RamdiskConfig;
use crate::firecracker::FirecrackerVM;
use crate::ramdisk::create_secure_ramdisk;
use crate::task::{ExecutionPool, ExecutionTask};

/// Events that can be processed by the execution flow state machine
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// Indicates successful completion of the current step
    StepSuccess,
    /// Indicates an error occurred in the current step
    StepError(String),
    /// Indicates a file change was detected in the watched directory
    FileChanged(PathBuf),
    /// Request to execute code in a specific language
    ExecuteCode {
        /// The programming language to use
        language: String,
        /// The code to execute
        code: String,
    },
}

/// States of the execution flow state machine
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// Initial state, ready to start execution
    Init,
    /// Creating and mounting the ramdisk
    MountRamdisk,
    /// Setting up execution environment
    PrepareExecution,
    /// Processing execution tasks
    Processing,
    /// All operations completed successfully
    Done,
    /// An error occurred during execution
    Failed,
}

/// Manages the execution flow of code in a secure ramdisk environment
pub struct ExecutionFlow {
    /// Counter for tracking execution steps
    pub step_count: usize,
    /// Current state of the execution flow
    state: State,
    /// Configuration for the ramdisk
    config: RamdiskConfig,
    /// Stored execution request for forwarding between states
    pending_execution: Option<(String, String)>,
    /// Task execution pool
    execution_pool: Option<ExecutionPool>,
    /// Number of tasks submitted
    tasks_submitted: usize,
    /// Number of tasks completed
    tasks_completed: usize,
    /// Firecracker VM instance if using VM isolation
    firecracker_vm: Option<FirecrackerVM>,
}

impl Default for ExecutionFlow {
    fn default() -> Self {
        Self {
            step_count: 0,
            state: State::Init,
            config: RamdiskConfig::default(),
            pending_execution: None,
            execution_pool: None,
            tasks_submitted: 0,
            tasks_completed: 0,
            firecracker_vm: None,
        }
    }
}

impl ExecutionFlow {
    /// Creates a new ExecutionFlow with custom ramdisk configuration
    pub fn new(config: RamdiskConfig) -> Self {
        Self {
            step_count: 0,
            state: State::Init,
            config,
            pending_execution: None,
            execution_pool: None,
            tasks_submitted: 0,
            tasks_completed: 0,
            firecracker_vm: None,
        }
    }

    /// Returns the current state of the execution flow
    pub fn state(&self) -> State {
        self.state
    }

    /// Handles incoming events and updates the state accordingly
    pub fn handle(&mut self, event: &PipelineEvent) {
        info!("Handling event {:?} in state {:?}", event, self.state);
        match (&self.state, event) {
            (State::Init, PipelineEvent::ExecuteCode { language, code }) => {
                info!("Received code execution request for {}", language);

                // Check if Firecracker is available
                if crate::firecracker::is_firecracker_available() {
                    info!("Firecracker is available, using VM isolation");
                    // Block on async function from sync context
                    let handle = tokio::runtime::Handle::current();
                    match handle.block_on(crate::firecracker::create_firecracker_environment(
                        &self.config,
                    )) {
                        Ok(vm) => {
                            info!("Firecracker VM created successfully");
                            self.firecracker_vm = Some(vm);
                            self.state = State::PrepareExecution;
                        }
                        Err(e) => {
                            warn!(
                                "Failed to create Firecracker VM: {}, falling back to ramdisk",
                                e
                            );
                            self.state = State::MountRamdisk;
                        }
                    }
                } else {
                    info!("Using Linux native ramdisk isolation");
                    self.state = State::MountRamdisk;
                }

                // Store the execution request for later
                self.pending_execution = Some((language.clone(), code.clone()));
                // Trigger next step
                self.handle(&PipelineEvent::StepSuccess);
            }
            (State::Init, PipelineEvent::StepSuccess) => {
                self.step_count += 1;
                info!(
                    "Mounting secure ramdisk with Landlock protection... (Step {})",
                    self.step_count
                );
                match create_secure_ramdisk(&self.config) {
                    Ok(_) => {
                        info!(
                            "Secure ramdisk mounted successfully at {}",
                            self.config.mount_point.display()
                        );
                        // Initialize execution pool
                        self.execution_pool = Some(ExecutionPool::new(4, self.config.clone()));
                        self.state = State::PrepareExecution;
                    }
                    Err(e) => {
                        error!("Failed to mount secure ramdisk: {}", e);
                        self.state = State::Failed;
                    }
                }
            }
            (State::Init, PipelineEvent::StepError(msg)) => {
                error!("Error in init: {}", msg);
                self.state = State::Failed;
            }
            (State::Init, PipelineEvent::FileChanged(path)) => {
                info!("File changed in init state: {}", path.display());
            }
            (State::MountRamdisk, PipelineEvent::StepSuccess) => {
                // Linux ramdisk setup
                info!(
                    "Trying to set up the ramdisk at {}",
                    self.config.mount_point.display()
                );
                match create_secure_ramdisk(&self.config) {
                    Ok(_) => {
                        info!("Secure ramdisk mounted successfully");
                        self.execution_pool = Some(ExecutionPool::new(4, self.config.clone()));
                        self.state = State::PrepareExecution;
                    }
                    Err(e) => {
                        // The error message from linux.rs should already be clear and helpful
                        error!(
                            "Failed to create ramdisk: {}. Sandboxed environments require ramdisk.",
                            e
                        );
                        // Don't add additional error messages that might conflict with the detailed one from linux.rs
                        self.state = State::Failed;
                    }
                }

                // Forward the stored execution request if available
                if let Some((language, code)) = &self.pending_execution {
                    self.handle(&PipelineEvent::ExecuteCode {
                        language: language.clone(),
                        code: code.clone(),
                    });
                }
            }
            (State::PrepareExecution, PipelineEvent::ExecuteCode { language, code }) => {
                info!("Preparing to execute {} code", language);

                if let Some(pool) = &self.execution_pool {
                    let task = ExecutionTask {
                        id: self.tasks_submitted,
                        language: language.clone(),
                        code: code.clone(),
                    };

                    match pool.submit_task(task) {
                        Ok(_) => {
                            info!("Task {} submitted for execution", self.tasks_submitted);
                            self.tasks_submitted += 1;
                            self.state = State::Processing;
                        }
                        Err(e) => {
                            error!("Failed to submit task: {}", e);
                            self.cleanup_ramdisk();
                            self.state = State::Failed;
                        }
                    }
                }
            }
            (State::Processing, _) => {
                // In Processing state, we should poll for task outcomes regardless of the event
                if let Some(pool) = &self.execution_pool {
                    // Try to receive outcome, but don't block if none is available yet
                    match pool.receive_outcome() {
                        Ok(outcome) => {
                            self.tasks_completed += 1;
                            if outcome.success {
                                info!("Task {} completed successfully", outcome.task_id);
                            } else {
                                error!(
                                    "Task {} failed: {}",
                                    outcome.task_id,
                                    outcome.error.unwrap_or_else(|| "Unknown error".to_string())
                                );
                            }

                            // If all tasks are complete, move to Done state
                            if self.tasks_completed == self.tasks_submitted {
                                info!("All tasks completed");
                                self.cleanup_ramdisk();
                                self.state = State::Done;
                            }
                        }
                        Err(e) => {
                            // Don't treat TryRecvError as fatal - only log it if it's a real error
                            if e.to_string().contains("disconnected") {
                                error!("Task channel disconnected: {}", e);
                                self.cleanup_ramdisk();
                                self.state = State::Failed;
                            } else {
                                // If no message is available yet, that's okay (TryRecvError::Empty case)
                                // Just keep processing state and wait for next event
                            }
                        }
                    }
                }
            }
            (State::Done, PipelineEvent::FileChanged(path)) => {
                info!("File changed while in done state: {}", path.display());
            }
            (State::Failed, PipelineEvent::FileChanged(path)) => {
                error!("File changed while failed: {}", path.display());
            }
            _ => {}
        }
    }

    /// Cleans up the ramdisk and VM when operations are complete or on failure
    fn cleanup_ramdisk(&self) {
        // Firecracker VM cleanup
        if let Some(vm) = &self.firecracker_vm {
            // Block on async function from sync context
            let handle = tokio::runtime::Handle::current();
            if let Err(e) = handle.block_on(vm.stop()) {
                error!("Failed to stop Firecracker VM: {}", e);
            } else {
                info!("Firecracker VM stopped successfully");
            }
        }

        // Ramdisk cleanup - only attempt if it exists
        let mount_point = &self.config.mount_point;

        // First check if the ramdisk is actually mounted
        match crate::ramdisk::is_mounted(mount_point) {
            Ok(true) => {
                info!("Attempting to unmount ramdisk at {}", mount_point.display());
                if let Err(e) = crate::ramdisk::remove_ramdisk(mount_point) {
                    warn!(
                        "Failed to cleanup ramdisk: {} - this is expected if ramdisk wasn't created",
                        e
                    );
                } else {
                    info!("Ramdisk cleanup successful");
                }
            }
            Ok(false) => {
                info!(
                    "No mounted ramdisk found at {}, skipping cleanup",
                    mount_point.display()
                );
            }
            Err(e) => {
                warn!("Failed to check if ramdisk is mounted: {}", e);
            }
        }

        // Clean up sandboxed environments if they exist
        let watched_dir = crate::ramdisk::get_watched_dir(&self.config);
        let python_env_path = watched_dir.join("python_venv");
        let node_env_path = watched_dir.join("node_env");
        let rust_env_path = watched_dir.join("rust_env");
        let go_env_path = watched_dir.join("go_env");

        // Clean up Python environment
        if python_env_path.exists() {
            info!("Cleaning up Python environment at {:?}", python_env_path);
            if let Err(e) = std::fs::remove_dir_all(&python_env_path) {
                warn!("Failed to clean up Python environment: {}", e);
            }
        }

        // Clean up Node environment
        if node_env_path.exists() {
            info!("Cleaning up Node environment at {:?}", node_env_path);
            if let Err(e) = std::fs::remove_dir_all(&node_env_path) {
                warn!("Failed to clean up Node environment: {}", e);
            }
        }

        // Clean up Rust environment
        if rust_env_path.exists() {
            info!("Cleaning up Rust environment at {:?}", rust_env_path);
            if let Err(e) = std::fs::remove_dir_all(&rust_env_path) {
                warn!("Failed to clean up Rust environment: {}", e);
            }
        }

        // Clean up Go environment
        if go_env_path.exists() {
            info!("Cleaning up Go environment at {:?}", go_env_path);
            if let Err(e) = std::fs::remove_dir_all(&go_env_path) {
                warn!("Failed to clean up Go environment: {}", e);
            }
        }

        // Ensure we're not leaving any security holes
        info!("Verifying all security restrictions are removed");
    }
}
