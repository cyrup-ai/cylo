use std::fmt::Formatter;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use log::{error, info};

use crate::config::RamdiskConfig;
use crate::error::ExecError;
use crate::exec::{exec_go, exec_js, exec_python, exec_rust};
use crate::exec_bash;

/// Represents a code execution task
#[derive(Debug)]
pub struct ExecutionTask {
    /// Unique identifier for the task
    pub id: usize,
    /// Programming language to use
    pub language: String,
    /// Code to execute
    pub code: String,
}

impl std::fmt::Display for ExecutionTask {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Executing {} in language {}", self.code, self.language)
    }
}

/// Result of a code execution task
#[derive(Debug)]
pub struct ExecutionOutcome {
    /// ID of the completed task
    pub task_id: usize,
    /// Whether the execution was successful
    pub success: bool,
    /// Error message if execution failed
    pub error: Option<String>,
}

/// Manages a pool of worker threads for code execution
pub struct ExecutionPool {
    task_sender: mpsc::Sender<ExecutionTask>,
    result_receiver: mpsc::Receiver<ExecutionOutcome>,
    worker_count: usize,
}

impl ExecutionPool {
    /// Creates a new execution pool with the specified number of workers
    pub fn new(worker_count: usize, config: RamdiskConfig) -> Self {
        let (task_tx, task_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let shared_rx = Arc::new(Mutex::new(task_rx));

        // Spawn worker threads
        for i in 0..worker_count {
            let thread_rx = Arc::clone(&shared_rx);
            let thread_result_tx = result_tx.clone();
            let thread_config = config.clone();
            thread::spawn(move || {
                Self::worker_loop(i, thread_rx, thread_result_tx, thread_config);
            });
        }

        Self {
            task_sender: task_tx,
            result_receiver: result_rx,
            worker_count,
        }
    }

    /// Submits a new task for execution
    pub fn submit_task(&self, task: ExecutionTask) -> Result<(), ExecError> {
        self.task_sender
            .send(task)
            .map_err(|e| ExecError::RuntimeError(format!("Failed to submit task: {e}")))
    }

    /// Receives the next completed task outcome, non-blocking
    pub fn receive_outcome(&self) -> Result<ExecutionOutcome, ExecError> {
        self.result_receiver
            .try_recv()
            .map_err(|e| ExecError::RuntimeError(format!("Failed to receive outcome: {e}")))
    }

    /// Returns the number of worker threads
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    /// Worker thread main loop
    fn worker_loop(
        worker_id: usize,
        rx: Arc<Mutex<mpsc::Receiver<ExecutionTask>>>,
        result_tx: mpsc::Sender<ExecutionOutcome>,
        config: RamdiskConfig,
    ) {
        info!("Worker {} started", worker_id);

        loop {
            info!("Worker {} waiting for next task", worker_id);
            let next_task = {
                let lock = match rx.lock() {
                    Ok(l) => l,
                    Err(poisoned) => {
                        error!("Worker {} mutex poisoned, recovering", worker_id);
                        poisoned.into_inner()
                    }
                };
                info!("Worker {} acquired lock", worker_id);
                let task = lock.recv();
                info!(
                    "Worker {} received task result: {:?}",
                    worker_id,
                    task.is_ok()
                );
                task
            };

            let task = match next_task {
                Ok(t) => t,
                Err(_) => {
                    info!("Worker {} channel closed, exiting", worker_id);
                    break;
                }
            };

            info!(
                "Worker {} processing task {} with language {}",
                worker_id, task.id, task.language
            );

            let outcome = match task.language.as_str() {
                "go" => exec_go(&task.code, &config),
                "rust" => exec_rust(&task.code, &config),
                "python" => {
                    info!("Worker {} executing Python code", worker_id);
                    let result = exec_python(&task.code, &config);
                    info!(
                        "Worker {} Python execution result: {:?}",
                        worker_id,
                        result.is_ok()
                    );
                    result
                }
                "js" => exec_js(&task.code, &config),
                "bash" => exec_bash(&task.code, &config),
                _ => Err(ExecError::UnsupportedLanguage(task.language.clone())),
            };

            info!("Worker {} sending outcome for task {}", worker_id, task.id);
            let execution_outcome = ExecutionOutcome {
                task_id: task.id,
                success: outcome.is_ok(),
                error: outcome.err().map(|e| e.to_string()),
            };

            if let Err(e) = result_tx.send(execution_outcome) {
                error!("Worker {} failed to send outcome: {}", worker_id, e);
                break;
            }
            info!("Worker {} successfully sent outcome", worker_id);
        }

        info!("Worker {} finished", worker_id);
    }
}
