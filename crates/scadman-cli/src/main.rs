//! The `scadman` command-line interface.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use scadman_core::{Manifest, manifest};

#[derive(Parser)]
#[command(
    name = "scadman",
    version,
    about = "Project-oriented OpenSCAD dependency manager"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a scadman.toml in the current directory.
    Init {
        /// Project name (defaults to the current directory name).
        #[arg(long)]
        name: Option<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Init { name } => init(name),
    }
}

fn init(name: Option<String>) -> Result<()> {
    let path = Path::new(manifest::MANIFEST_FILE);
    if path.exists() {
        bail!("{} already exists", manifest::MANIFEST_FILE);
    }

    let name = match name {
        Some(name) => name,
        None => current_dir_name().unwrap_or_else(|| "project".to_string()),
    };

    let manifest = Manifest::new(name);
    let text = manifest.to_toml().context("serialize manifest")?;
    fs::write(path, text).with_context(|| format!("write {}", manifest::MANIFEST_FILE))?;
    println!("Created {}", manifest::MANIFEST_FILE);
    Ok(())
}

fn current_dir_name() -> Option<String> {
    std::env::current_dir()
        .ok()?
        .file_name()?
        .to_str()
        .map(str::to_string)
}
