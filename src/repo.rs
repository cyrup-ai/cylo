use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Result};
use log::info;

/// Configuration for repository initialization
#[derive(Debug, Clone)]
pub struct RepoConfig {
    /// Path to the repository
    pub path: PathBuf,
    /// Whether to initialize git if not already initialized
    pub init_git: bool,
    /// Whether to set up git filters
    pub setup_filters: bool}

impl Default for RepoConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("."),
            init_git: true,
            setup_filters: true}
    }
}

/// Initializes a repository with metadata and git filters
pub fn init_repository(config: &RepoConfig) -> Result<()> {
    info!("Initializing repository at {:?}", config.path);

    // Create .parallm directory if it doesn't exist
    let parallm_dir = config.path.join(".parallm");
    fs::create_dir_all(&parallm_dir)?;

    // Create git directory if needed
    let git_dir = parallm_dir.join("git");
    fs::create_dir_all(&git_dir)?;

    // Copy filter scripts if they don't exist
    copy_filter_scripts(&git_dir)?;

    if config.init_git {
        init_git(&config.path)?;
    }

    if config.setup_filters {
        setup_git_filters(&config.path)?;
    }

    info!("Repository initialization complete");
    Ok(())
}

/// Copies clean and smudge filter scripts to the repository
fn copy_filter_scripts(git_dir: &Path) -> Result<()> {
    // Create the directory if it doesn't exist
    if !git_dir.exists() {
        fs::create_dir_all(git_dir)?;
    }

    let clean_script = include_str!("../.parallm/git/clean.rs");
    let smudge_script = include_str!("../.parallm/git/smudge.rs");

    fs::write(git_dir.join("clean.rs"), clean_script)?;
    fs::write(git_dir.join("smudge.rs"), smudge_script)?;

    // Make scripts executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = 0o755;
        fs::set_permissions(git_dir.join("clean.rs"), fs::Permissions::from_mode(mode))?;
        fs::set_permissions(git_dir.join("smudge.rs"), fs::Permissions::from_mode(mode))?;
    }

    Ok(())
}

/// Initializes git repository if not already initialized
fn init_git(repo_path: &Path) -> Result<()> {
    if !repo_path.join(".git").exists() {
        info!("Initializing git repository");
        let output = Command::new("git")
            .arg("init")
            .current_dir(repo_path)
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to initialize git repository: {}", error));
        }
    }
    Ok(())
}

/// Sets up git filters for the repository
fn setup_git_filters(repo_path: &Path) -> Result<()> {
    info!("Setting up git filters");

    // Create .gitattributes if it doesn't exist
    let gitattributes = repo_path.join(".gitattributes");
    if !gitattributes.exists() {
        fs::write(&gitattributes, "*.txt filter=parallm_filter\n")?;
    }

    // Configure git filters
    let filter_commands = [
        [
            "config",
            "filter.parallm_filter.clean",
            ".parallm/git/clean.rs",
        ],
        [
            "config",
            "filter.parallm_filter.smudge",
            ".parallm/git/smudge.rs",
        ],
    ];

    for args in filter_commands.iter() {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Failed to configure git filter: {}", error));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_repository_initialization() {
        let temp_dir = tempdir()
            .expect("test should successfully create temporary directory for repository initialization");
        let config = RepoConfig {
            path: temp_dir.path().to_path_buf(),
            init_git: true,
            setup_filters: true};

        assert!(init_repository(&config).is_ok());
        assert!(temp_dir.path().join(".parallm").exists());
        assert!(temp_dir.path().join(".parallm/git").exists());
        assert!(temp_dir.path().join(".parallm/git/clean.rs").exists());
        assert!(temp_dir.path().join(".parallm/git/smudge.rs").exists());
        assert!(temp_dir.path().join(".git").exists());
        assert!(temp_dir.path().join(".gitattributes").exists());
    }
}
