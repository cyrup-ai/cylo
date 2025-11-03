# Linux-Only Implementation Details

This document describes the Linux-specific implementation details of the cylo service.

## Isolation Mechanisms

cylo uses multiple isolation mechanisms to create a secure execution environment:

1. **User Namespace**: Creates a new user namespace where the current user appears as root, allowing privileged operations within the namespace without requiring actual root privileges.

2. **Mount Namespace**: Creates a new mount namespace to isolate filesystem mounts from the host system.

3. **Sandboxed Language Environments**: Each language runtime operates in an isolated environment with:
   - Dedicated directories for dependencies
   - Restricted environment variables
   - Wrapper scripts that enforce security boundaries

4. **Ramdisk Isolation**: All execution happens within a temporary ramdisk that is isolated from the host filesystem.

## Ramdisk Implementation

The Linux ramdisk implementation uses tmpfs mounted in the isolated namespace:

```
User Space
┌─────────────────────────────────────┐
│                                     │
│  ┌─────────────────────────────┐    │
│  │ Isolated Namespace          │    │
│  │                             │    │
│  │  ┌─────────────────────┐    │    │
│  │  │ tmpfs Ramdisk       │    │    │
│  │  │                     │    │    │
│  │  │  ┌─────────────┐    │    │    │
│  │  │  │ watched_dir │    │    │    │
│  │  │  └─────────────┘    │    │    │
│  │  └─────────────────────┘    │    │
│  └─────────────────────────────┘    │
└─────────────────────────────────────┘
Kernel Space
```

## Landlock Integration

Landlock is a Linux security module that provides unprivileged access control. cylo uses Landlock to restrict file access to only the watched directory, even if the namespace isolation is compromised.

## System Requirements

### Kernel Version

- Linux kernel 5.11 or newer is recommended for full namespace and Landlock support
- Minimum kernel version: 4.14 (with limited functionality)

### User Namespace Configuration

User namespaces must be enabled:

```bash
# Check current setting
sysctl kernel.unprivileged_userns_clone

# Enable temporarily
sudo sysctl -w kernel.unprivileged_userns_clone=1

# Enable permanently
echo 'kernel.unprivileged_userns_clone=1' | sudo tee /etc/sysctl.d/00-local-userns.conf
sudo sysctl --system
```

### AppArmor Configuration

If AppArmor is active, you may need to set the profile to complain mode:

```bash
# Check if AppArmor is active
aa-status

# Set profile to complain mode
sudo aa-complain /usr/bin/cargo
```

## Security Guarantees

Cylo takes a strict approach to security with no fallbacks to less secure mechanisms:

1. **Mandatory Ramdisk**: A ramdisk is required for execution. If it cannot be created, execution will fail.
2. **Mandatory Sandboxed Environments**: Each language requires a properly sandboxed environment. If this cannot be created, execution will fail.
3. **No Fallbacks to Insecure Execution**: Unlike previous versions, this implementation does not fall back to less secure execution methods when proper security cannot be established.

This approach ensures consistent security guarantees across all executions.

## Troubleshooting

### Common Issues

1. **"Operation not permitted" when creating namespaces**
   - User namespaces are disabled
   - Solution: Enable user namespaces as described above

2. **"Permission denied" when mounting tmpfs**
   - AppArmor or SELinux is blocking the operation
   - Solution: Adjust AppArmor profile or temporarily disable SELinux

3. **"Invalid argument" when using namespaces**
   - Running in a container that doesn't support nested namespaces
   - Solution: Run directly on the host or use a privileged container

4. **"Failed to create secure environment" errors**
   - Ramdisk couldn't be mounted or sandboxed environment couldn't be created
   - Solution: Ensure /ephemeral/cylo is writable or run with sudo
   - Don't attempt to bypass this error - it's a security feature!

### Diagnostic Commands

```bash
# Check kernel version
uname -r

# Check namespace support
ls -l /proc/self/ns/

# Check if running in a container
grep -q docker /proc/1/cgroup && echo "In Docker"

# Check AppArmor status
aa-status
```
