use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use log::{error, info};

/// Metadata keys used for source code files
pub const XATTR_NAMESPACE: &str = "user.ironexec";
pub const XATTR_LANGUAGE: &str = "user.ironexec.language";
pub const XATTR_LAST_EXECUTED: &str = "user.ironexec.last_executed";
pub const XATTR_EXECUTION_COUNT: &str = "user.ironexec.execution_count";

/// Manages source code metadata using extended attributes
pub struct MetadataManager {
    /// Base directory for the repository
    repo_path: PathBuf,
}

impl MetadataManager {
    /// Creates a new metadata manager for the specified repository path
    pub fn new<P: AsRef<Path>>(repo_path: P) -> Self {
        Self {
            repo_path: repo_path.as_ref().to_path_buf(),
        }
    }

    /// Sets up git filters for the repository
    pub fn setup_git_filters(&self) -> io::Result<()> {
        // Create .gitattributes if it doesn't exist
        let gitattributes_path = self.repo_path.join(".gitattributes");
        if !gitattributes_path.exists() {
            let mut file = File::create(&gitattributes_path)?;
            writeln!(file, "*.txt filter=ironexec")?;
            writeln!(file, "*.rs filter=ironexec")?;
            writeln!(file, "*.py filter=ironexec")?;
            writeln!(file, "*.js filter=ironexec")?;
            writeln!(file, "*.go filter=ironexec")?;
        }

        // Create .parallm directory if it doesn't exist
        let parallm_dir = self.repo_path.join(".parallm");
        if !parallm_dir.exists() {
            std::fs::create_dir_all(&parallm_dir)?;
        }

        // Create git filter scripts
        self.create_clean_script()?;
        self.create_smudge_script()?;

        Ok(())
    }

    /// Creates the clean script that runs when files are staged
    fn create_clean_script(&self) -> io::Result<()> {
        let script_path = self.repo_path.join(".parallm/git/clean.rs");
        if let Some(parent) = script_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = File::create(&script_path)?;
        write!(
            file,
            r#"#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! xattr = "1.0"
//! ```

use std::io::{{self, Read, Write}};
use xattr::FileExt;

fn main() -> io::Result<()> {{
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    
    // Clean operation: Pass through the content unchanged
    // The xattrs are preserved separately
    io::stdout().write_all(input.as_bytes())?;
    Ok(())
}}
"#
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        Ok(())
    }

    /// Creates the smudge script that runs when files are checked out
    fn create_smudge_script(&self) -> io::Result<()> {
        let script_path = self.repo_path.join(".parallm/git/smudge.rs");
        if let Some(parent) = script_path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = File::create(&script_path)?;
        write!(
            file,
            r#"#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! xattr = "1.0"
//! ```

use std::io::{{self, Read, Write}};
use xattr::FileExt;

fn main() -> io::Result<()> {{
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    
    // Smudge operation: Pass through the content unchanged
    // The xattrs will be restored from the git notes
    io::stdout().write_all(input.as_bytes())?;
    Ok(())
}}
"#
        )?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&script_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms)?;
        }

        Ok(())
    }

    /// Updates metadata for a source code file
    #[cfg(target_os = "linux")]
    pub fn update_metadata<P: AsRef<Path>>(&self, path: P, language: &str) -> io::Result<()> {
        use xattr::FileExt;
        let path = path.as_ref();

        match File::open(path) {
            Ok(file) => {
                if let Err(e) = file.set_xattr(XATTR_LANGUAGE, language.as_bytes()) {
                    error!("Failed to set language xattr: {}", e);
                    return Ok(());
                }

                let timestamp = chrono::Utc::now().to_rfc3339();
                if let Err(e) = file.set_xattr(XATTR_LAST_EXECUTED, timestamp.as_bytes()) {
                    error!("Failed to set timestamp xattr: {}", e);
                    return Ok(());
                }

                let count = match file.get_xattr(XATTR_EXECUTION_COUNT) {
                    Ok(Some(bytes)) => {
                        String::from_utf8_lossy(&bytes).parse::<u32>().unwrap_or(0) + 1
                    }
                    _ => 1,
                };

                if let Err(e) = file.set_xattr(XATTR_EXECUTION_COUNT, count.to_string().as_bytes())
                {
                    error!("Failed to set execution count xattr: {}", e);
                    return Ok(());
                }

                info!(
                    "Updated metadata for {}: lang={} count={} last={}",
                    path.display(),
                    language,
                    count,
                    timestamp
                );
            }
            Err(e) => {
                error!("Failed to open file for metadata update: {}", e);
            }
        }

        Ok(())
    }

    /// Updates metadata for a source code file (no-op on non-Linux platforms)
    #[cfg(not(target_os = "linux"))]
    pub fn update_metadata<P: AsRef<Path>>(&self, _path: P, _language: &str) -> io::Result<()> {
        // Extended attributes not supported on this platform
        Ok(())
    }

    /// Gets metadata for a source code file
    #[cfg(target_os = "linux")]
    pub fn get_metadata<P: AsRef<Path>>(&self, path: P) -> io::Result<Option<FileMetadata>> {
        use xattr::FileExt;
        let path = path.as_ref();
        let file = File::open(path)?;

        let language = match file.get_xattr(XATTR_LANGUAGE) {
            Ok(Some(bytes)) => String::from_utf8_lossy(&bytes).to_string(),
            _ => return Ok(None),
        };

        let last_executed = file
            .get_xattr(XATTR_LAST_EXECUTED)
            .ok()
            .flatten()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string());

        let execution_count = file
            .get_xattr(XATTR_EXECUTION_COUNT)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8_lossy(&bytes).parse::<u32>().ok())
            .unwrap_or(0);

        Ok(Some(FileMetadata {
            language,
            last_executed,
            execution_count,
        }))
    }

    /// Gets metadata for a source code file (returns None on non-Linux platforms)
    #[cfg(not(target_os = "linux"))]
    pub fn get_metadata<P: AsRef<Path>>(&self, _path: P) -> io::Result<Option<FileMetadata>> {
        // Extended attributes not supported on this platform
        Ok(None)
    }
}

/// Metadata associated with a source code file
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// Programming language of the file
    pub language: String,
    /// When the file was last executed
    pub last_executed: Option<String>,
    /// Number of times the file has been executed
    pub execution_count: u32,
}
