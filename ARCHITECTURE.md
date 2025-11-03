# cylo Architecture

This document describes the architecture of the cylo (Iron Executor) project, a secure multi-language code execution service.

## System Overview

cylo is designed as a secure service for executing code snippets in multiple programming languages. It leverages Firecracker microVMs or ramdisk-based isolation for security and provides a simple CLI interface.

```
                   ┌─────────────────┐
                   │  CLI Interface  │
                   └────────┬────────┘
                            │
                   ┌────────▼────────┐
                   │  Execution Flow │
                   │  State Machine  │
                   └────────┬────────┘
                            │
         ┌─────────────────┼─────────────────┐
         │                 │                 │
┌────────▼────────┐ ┌──────▼───────┐ ┌──────▼───────┐
│ Isolation Layer │ │ Task Manager │ │ File Watcher │
└────────┬────────┘ └──────┬───────┘ └──────────────┘
         │                 │
┌────────▼────────┐ ┌──────▼───────┐
│ Firecracker VM  │ │ Language     │
│ or Ramdisk      │ │ Executors    │
└─────────────────┘ └──────────────┘
```

## Core Components

### 1. CLI Interface (`cli.rs`)

The CLI interface is built using the `clap` crate and provides:
- Command-line argument parsing
- Debug logging options
- Subcommand structure with the primary `exec` command

### 2. Execution Flow State Machine (`state.rs`)

The state machine manages the execution flow through various states:
- `Init`: Initial state, ready to start execution
- `MountRamdisk`: Creating and mounting the ramdisk
- `PrepareExecution`: Setting up execution environment
- `Processing`: Processing execution tasks
- `Done`: All operations completed successfully
- `Failed`: An error occurred during execution

The state machine processes events via the `PipelineEvent` enum, which includes events like:
- `StepSuccess`: Indicates successful completion of the current step
- `StepError`: Indicates an error occurred in the current step
- `FileChanged`: Indicates a file change was detected in the watched directory
- `ExecuteCode`: Request to execute code in a specific language

### 3. Isolation Management

#### Firecracker VM (`firecracker.rs`)
Provides hardware virtualization-based isolation using Firecracker microVMs:
- `create_firecracker_environment`: Creates a Firecracker VM with the specified configuration
- `is_firecracker_available`: Checks if Firecracker is available on the system
- `FirecrackerVM`: Manages a Firecracker VM instance for code execution

#### Ramdisk Management (`ramdisk.rs`)
Provides a unified API for ramdisk operations as a fallback isolation mechanism:
- `create_ramdisk`: Creates a ramdisk with the specified configuration
- `remove_ramdisk`: Removes a ramdisk at the specified mount point
- `is_mounted`: Checks if a path is mounted as a ramdisk

#### Platform Interface (`platform.rs`)
Defines the `RamdiskPlatform` trait that platform-specific implementations must implement:
- `new`: Create a new instance of the platform implementation
- `is_mounted`: Check if a path is mounted as a ramdisk
- `create`: Create a new ramdisk with the given configuration
- `remove`: Remove an existing ramdisk at the given mount point

#### Platform-Specific Implementations
- `linux.rs`: Linux-specific implementation using tmpfs
- `macos.rs`: macOS-specific implementation

### 4. Task Execution (`task.rs`)

Manages code execution tasks with:
- `ExecutionTask`: Represents a code execution task
- `ExecutionOutcome`: Represents the result of a code execution task
- `ExecutionPool`: Manages a pool of worker threads for code execution

### 5. Language Executors and Sandboxed Environments

#### Language Executors (`exec.rs`)

Contains functions to execute code in different languages:
- `exec_go`: Executes Go code in a sandboxed environment
- `exec_rust`: Executes Rust code in a sandboxed environment
- `exec_python`: Executes Python code in a sandboxed environment
- `exec_js`: Executes JavaScript code in a sandboxed environment

Each executor:
1. Creates a temporary file with the provided code
2. Creates a sandboxed environment for the language runtime
3. Executes the code within the sandboxed environment
4. Cleans up after execution
5. Returns the execution result

#### Sandboxed Environments (`sandbox.rs`)

Provides a security layer for language runtime isolation:

- `SandboxedEnvironment`: Represents an isolated environment for a specific language runtime
  - Provides environment variables for isolation
  - Contains wrapper scripts that enforce security boundaries
  - Manages execution within the environment

- `SandboxManager`: Creates and manages sandboxed environments
  - `create_python_environment`: Creates an isolated Python virtual environment
  - `create_node_environment`: Creates an isolated Node.js environment
  - `create_rust_environment`: Creates an isolated Rust environment
  - `create_go_environment`: Creates an isolated Go environment

Each sandboxed environment:
1. Creates a dedicated directory structure
2. Sets up environment variables for isolation
3. Creates wrapper scripts for language runtimes
4. Ensures proper cleanup after use

### 6. File Watching (`watcher.rs`)

Monitors a directory for file changes using the `notify` crate and sends events to the state machine.

### 7. Configuration (`config.rs`)

Defines `RamdiskConfig` for configuring ramdisk parameters:
- Size in GB
- Mount point
- Volume name
- Filesystem type (for macOS)

### 8. Error Handling (`error.rs`)

Defines error types using `thiserror`:
- `ExecError`: For code execution errors
- `StorageError`: For ramdisk and storage-related errors

## Execution Flow

1. User submits code via CLI with language specification
2. CLI parses arguments and creates an `ExecuteCode` event
3. System checks if Firecracker is available
4. If Firecracker is available:
   a. State machine transitions from `Init` to `MountRamdisk`
   b. Firecracker VM is created and configured
   c. State machine transitions to `PrepareExecution`
   d. Code is executed in the Firecracker VM
   e. Results are returned to the user
   f. State machine transitions to `Done`
   g. Firecracker VM is cleaned up after execution
5. If Firecracker is not available (fallback):
   a. State machine transitions from `Init` to `MountRamdisk`
   b. Ramdisk is created and mounted (if needed)
   c. State machine transitions to `PrepareExecution`
   d. Execution environment is prepared
   e. State machine transitions to `Processing`
   f. Code is written to a temporary file in the secure environment
   g. Appropriate language interpreter/compiler executes the code
   h. Results are returned to the user
   i. State machine transitions to `Done`
   j. Ramdisk is cleaned up after execution

## Security Model

cylo uses several techniques to ensure secure code execution:

1. **Hardware Virtualization**: When available, code is executed in Firecracker microVMs, providing strong isolation through hardware virtualization
2. **Ramdisk Isolation**: Code is executed in a ramdisk environment, isolated from the host system
3. **Sandboxed Language Environments**: Each language runtime operates in a sandboxed environment with:
   - Isolated directories for dependencies and modules
   - Environment variables configured for containment
   - Wrapper scripts that enforce security boundaries
4. **Landlock File System Restrictions**: Uses the Landlock security module to provide additional kernel-level file system access control
5. **Privilege Checking**: The system checks for appropriate privileges for operations that require them
6. **No Security Fallbacks**: Security is non-negotiable - if proper security cannot be established, execution will fail rather than fall back to less secure methods
7. **Resource Cleanup**: Temporary files, ramdisks, VMs, and sandboxed environments are cleaned up after execution
8. **Error Handling**: Comprehensive error handling prevents security issues from unexpected states

## Dependencies

- **clap**: Command-line argument parsing
- **tracing**: Logging and diagnostics
- **tempfile**: Temporary file creation
- **notify**: File system notifications
- **nix**: Unix system call bindings (Linux-specific)
- **thiserror/anyhow**: Error handling
- **statig**: State machine implementation
- **chrono**: Date and time functionality
