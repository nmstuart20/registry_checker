use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Finds missing dependencies for offline registry"
)]
struct Args {
    /// Path to the Cargo.toml of the project you want to check
    #[arg(short, long, default_value = "./Cargo.toml")]
    manifest_path: PathBuf,

    /// Path to the text file listing your current offline registry crates
    #[arg(short, long)]
    registry_file: PathBuf,

    /// Append missing crates directly to the file instead of just printing them
    #[arg(short, long)]
    write: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // 1. Parse the project dependencies using cargo_metadata
    // This looks at Cargo.lock to get the EXACT full tree (transitive deps included)
    println!("Scanning project dependencies...");
    let metadata = MetadataCommand::new()
        .manifest_path(&args.manifest_path)
        .exec()
        .context("Failed to run cargo metadata. Is this a valid Rust project?")?;

    // Collect all project dependencies into a set of "name:version" strings
    let project_deps: HashSet<String> = metadata
        .packages
        .into_iter()
        .filter(|p| p.source.is_some()) // Filter out local path dependencies (your own workspaces)
        .map(|p| format!("{}:{}", p.name, p.version))
        .collect();

    // 2. Read the existing registry file
    println!("Reading existing registry file: {:?}", args.registry_file);
    let file_content =
        fs::read_to_string(&args.registry_file).context("Could not read registry file")?;

    // Store existing entries in a HashSet for O(1) lookups
    // We trim whitespace to handle different formatting
    let existing_registry: HashSet<String> = file_content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    // 3. Find missing crates
    let mut missing_crates: Vec<String> = project_deps
        .difference(&existing_registry)
        .cloned()
        .collect();

    if missing_crates.is_empty() {
        println!("All dependencies are already in the registry file.");
        return Ok(());
    }

    // Sort for readability
    missing_crates.sort();

    println!("Found {} missing crates:", missing_crates.len());
    for krate in &missing_crates {
        println!("  + {}", krate);
    }

    // 4. Update the file if requested
    if args.write {
        let mut file = OpenOptions::new()
            .append(true)
            .open(&args.registry_file)
            .context("Failed to open registry file for writing")?;

        for krate in missing_crates {
            writeln!(file, "{}", krate)?;
        }
        println!(
            "Successfully appended missing crates to {:?}",
            args.registry_file
        );
    } else {
        println!("\n(Run with --write to append these automatically)");
    }

    Ok(())
}
