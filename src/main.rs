use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about = "Finds missing dependencies for offline registry")]
struct Args {
    /// Path to the Cargo.toml of the project you want to check
    #[arg(short, long, default_value = "./Cargo.toml")]
    manifest_path: PathBuf,

    /// Path to the text file listing your current offline registry crates
    #[arg(short, long)]
    registry_file: PathBuf,

    /// Add missing crates and sort the file
    #[arg(short, long)]
    write: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Scanning project dependencies...");
    let metadata = MetadataCommand::new()
        .manifest_path(&args.manifest_path)
        .exec()
        .context("Failed to run cargo metadata. Is this a valid Rust project?")?;

    // Format dependencies as 'name-version.crate'
    let project_deps: HashSet<String> = metadata
        .packages
        .into_iter()
        .filter(|p| p.source.is_some())
        .map(|p| format!("{}-{}.crate", p.name, p.version))
        .collect();

    println!("Reading existing registry file: {:?}", args.registry_file);
    let file_content = fs::read_to_string(&args.registry_file)
        .context("Could not read registry file")?;

    let existing_registry: HashSet<String> = file_content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    // Find what is missing
    let missing_crates: Vec<String> = project_deps
        .difference(&existing_registry)
        .cloned()
        .collect();

    if missing_crates.is_empty() {
        println!("All dependencies are already present in the registry file.");
        
        // Optional: You might still want to sort the file even if nothing is missing?
        // If so, you could move the write logic outside this check.
        return Ok(());
    }

    // Display what we found
    let mut missing_sorted = missing_crates.clone();
    missing_sorted.sort();
    
    println!("Found {} missing crates:", missing_crates.len());
    for krate in &missing_sorted {
        println!("  + {}", krate);
    }

    if args.write {
        println!("Merging and sorting registry file...");

        // 1. Combine existing and missing
        let mut full_list: Vec<String> = existing_registry
            .union(&project_deps) // Union handles duplicates automatically
            .cloned()
            .collect();

        // 2. Sort the full list
        full_list.sort();

        // 3. Overwrite the file with the sorted content
        let file = File::create(&args.registry_file)
            .context("Failed to open registry file for writing")?;
        let mut writer = BufWriter::new(file);

        for line in full_list {
            writeln!(writer, "{}", line)?;
        }

        println!("Successfully updated and sorted {:?}", args.registry_file);
    } else {
        println!("\n(Run with --write to add these and sort the file)");
    }

    Ok(())
}