use clap::{Args, Parser, Subcommand};
use log::info;

use crate::{config::RamdiskConfig, error::ExecError, exec};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// [DEPRECATED] Landlock file system restrictions are now mandatory
    #[arg(long, default_value_t = false, hide = true)]
    disable_landlock: bool,

    #[command(subcommand)]
    command: Commands}

#[derive(Subcommand)]
pub enum Commands {
    /// Execute code in various languages
    Exec(ExecArgs)}

#[derive(Args)]
pub struct ExecArgs {
    /// Language to execute (go, rust, python, js, bash)
    #[arg(short, long)]
    lang: String,

    /// Code to execute
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
    code: Vec<String>}

impl ExecArgs {
    pub fn lang(&self) -> &str {
        &self.lang
    }

    pub fn code(&self) -> String {
        self.code.join(" ")
    }
}

impl Cli {
    pub fn is_debug(&self) -> bool {
        self.debug
    }

    pub fn is_landlock_enabled(&self) -> bool {
        !self.disable_landlock
    }

    pub fn get_exec_args(&self) -> Option<&ExecArgs> {
        match &self.command {
            Commands::Exec(args) => Some(args)}
    }

    pub fn execute(&self) -> Result<(), ExecError> {
        match &self.command {
            Commands::Exec(args) => {
                info!("Executing {} code", args.lang());
                // Create a default RamdiskConfig for the execution
                let config = RamdiskConfig::default();

                match args.lang() {
                    "go" => exec::exec_go(&args.code(), &config)?,
                    "rust" => exec::exec_rust(&args.code(), &config)?,
                    "python" => exec::exec_python(&args.code(), &config)?,
                    "js" => exec::exec_js(&args.code(), &config)?,
                    "bash" => exec::exec_bash(&args.code(), &config)?,
                    _ => return Err(ExecError::UnsupportedLanguage(args.lang().to_string()))}
                info!("{} code executed successfully", args.lang());
            }
        }
        Ok(())
    }
}
