# cylo - Secure Multi-Language Code Execution Service for Linux

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

cylo (Iron Executor) is a secure service for executing code snippets in multiple programming languages. It provides isolation through ramdisk-based execution environments and supports Go, Rust, Python, and JavaScript. This service is designed specifically for Linux systems.

## Features

- **Multi-language Support**: Execute code in Bash, Go, Rust, Python, and JavaScript
- **Secure Execution**: Isolate code execution using ramdisk and Landlock file system restrictions
- **Enhanced Security**: Multilayered security approach with namespace isolation
- **Linux-optimized**: Designed specifically for Linux systems
- **Parallel Execution**: Run multiple code snippets concurrently
- **File Watching**: Monitor directories for changes to trigger executions
- **CLI Interface**: Simple command-line interface for code execution

## Installation

### Prerequisites

- Linux system (kernel 5.11+ recommended)
- Rust toolchain (1.56.0 or newer)
- Language runtimes for languages you want to execute:
  - Go (for Go code execution)
  - rust-script (for Rust code execution)
  - Python 3 (for Python code execution)
  - Node.js (for JavaScript execution)

### Linux System Requirements

- Kernel 5.11 or newer recommended
- The system will automatically prompt for sudo access if needed to:
  - Enable user namespaces (`sudo sysctl -w kernel.unprivileged_userns_clone=1`)
  - Configure AppArmor (`sudo aa-complain /usr/bin/cargo`)
  - Mount ramdisks directly if other methods fail

- If you prefer to configure your system manually instead of using sudo prompts:
  ```bash
  # Enable user namespaces
  sudo sysctl -w kernel.unprivileged_userns_clone=1

  # For permanent change:
  echo 'kernel.unprivileged_userns_clone=1' | sudo tee /etc/sysctl.d/00-local-userns.conf

  # If using AppArmor, set the profile to complain mode:
  sudo aa-complain /usr/bin/cargo
  ```

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/cylo.git
cd cylo

# Build the project
cargo build --release

# The binary will be available at target/release/ironexec
```

## Usage

### Basic Usage

Execute code in a specific language:

```bash
# Execute Go code
ironexec exec --lang go "package main; import \"fmt\"; func main() { fmt.Println(\"Hello from Go!\") }"

# Execute Rust code
ironexec exec --lang rust "fn main() { println!(\"Hello from Rust!\"); }"

# Execute Python code
ironexec exec --lang python "print('Hello from Python!')"

# Execute JavaScript code
ironexec exec --lang js "console.log('Hello from JavaScript!')"
```

### Debug Mode

Enable debug logging for more detailed output:

```bash
ironexec --debug exec --lang python "print('Running with debug output')"
```

### File Watching

The service can watch a directory for file changes:

```bash
# Place files in the watched_dir directory to trigger executions
mkdir -p watched_dir
touch watched_dir/trigger.txt
```

## Security Features

cylo provides a multi-layered approach to secure code execution:

1. **Namespace Isolation**: Uses Linux user and mount namespaces to create an isolated execution environment.

2. **Ramdisk Isolation**: All code executes in a dedicated ramdisk environment, isolating file operations from the host system.

3. **Sandboxed Language Environments**: Each language runtime operates in a sandboxed environment with:
   - Isolated directories for dependencies and modules
   - Environment variables configured for containment
   - Wrapper scripts that enforce security boundaries
   - No access to user home directories or system libraries

4. **Landlock File System Restrictions**: Uses the Landlock security module to provide additional kernel-level file system access control, restricting which directories and files can be modified by executed code.

5. **File Monitoring**: The watchexec integration monitors file access and modifications, logging any attempts to modify protected files.

6. **Secure by Default**: Security is non-negotiable - if proper security cannot be established, execution will fail rather than fall back to less secure methods. This ensures consistent security guarantees.

Note: Landlock restrictions and sandbox environments are mandatory for security and cannot be disabled.

## Docker Support

You can run cylo in a Docker container:

```bash
# Build the Docker image
docker build -t cylo .

# Run the container
docker run -it --rm cylo exec --lang python "print('Hello from Docker!')"
```

## Architecture

For detailed information about the project's architecture, please see [ARCHITECTURE.md](./ARCHITECTURE.md).

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.
