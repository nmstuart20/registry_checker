use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use semver::Version;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
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

    /// Add missing crates and sort the file
    #[arg(short, long)]
    write: bool,
}

/// Parse a crate filename (e.g., "serde-1.0.0.crate") into (name, version)
fn parse_crate_name_version(crate_file: &str) -> Option<(String, Version)> {
    // Remove the .crate extension
    let without_ext = crate_file.strip_suffix(".crate")?;

    // Find the last dash that separates name from version
    let last_dash = without_ext.rfind('-')?;
    let name = &without_ext[..last_dash];
    let version_str = &without_ext[last_dash + 1..];

    // Parse the version
    let version = Version::parse(version_str).ok()?;

    Some((name.to_string(), version))
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
    let file_content =
        fs::read_to_string(&args.registry_file).context("Could not read registry file")?;

    let existing_registry: HashSet<String> = file_content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();

    // Build a map of crate names to their versions in the registry
    let mut registry_versions: HashMap<String, Vec<Version>> = HashMap::new();
    for crate_file in &existing_registry {
        if let Some((name, version)) = parse_crate_name_version(crate_file) {
            registry_versions.entry(name).or_default().push(version);
        }
    }

    // Find what is missing
    let missing_crates: Vec<String> = project_deps
        .difference(&existing_registry)
        .cloned()
        .collect();

    if missing_crates.is_empty() {
        println!("All dependencies are already present in the registry file.");
        return Ok(());
    }

    // Display what we found with version analysis
    let mut missing_sorted = missing_crates.clone();
    missing_sorted.sort();

    println!("Found {} missing crates:", missing_crates.len());

    let mut needs_approval: Vec<(String, String)> = Vec::new(); // (crate, reason)
    let mut minor_upgrades: Vec<String> = Vec::new();

    for krate in &missing_sorted {
        if let Some((name, new_version)) = parse_crate_name_version(krate) {
            if let Some(existing_versions) = registry_versions.get(&name) {
                // Find the latest existing version
                let latest_existing = existing_versions.iter().max().unwrap();

                if new_version < *latest_existing {
                    // Version downgrade - needs approval
                    println!(
                        "  {} [WARNING: DOWNGRADE from {}, requires approval]",
                        krate, latest_existing
                    );
                    needs_approval.push((
                        krate.clone(),
                        format!("downgrade from {}", latest_existing),
                    ));
                } else if new_version.major != latest_existing.major {
                    // Major version upgrade - needs approval
                    println!(
                        "  {} [WARNING: MAJOR version upgrade from {}, requires approval]",
                        krate, latest_existing
                    );
                    needs_approval.push((
                        krate.clone(),
                        format!("major upgrade from {}", latest_existing),
                    ));
                } else if new_version.minor != latest_existing.minor
                    || new_version.patch != latest_existing.patch
                {
                    // Minor or patch version upgrade - OK
                    println!(
                        "  {} [minor/patch upgrade from {}]",
                        krate, latest_existing
                    );
                    minor_upgrades.push(krate.clone());
                } else {
                    // Same version? Shouldn't happen, but just in case
                    println!("  {}", krate);
                }
            } else {
                // New dependency - needs approval
                println!("  {} [WARNING: NEW dependency, requires approval]", krate);
                needs_approval.push((krate.clone(), "new dependency".to_string()));
            }
        } else {
            // Couldn't parse version
            println!("  {} [WARNING: Could not parse version]", krate);
            needs_approval.push((krate.clone(), "unable to parse version".to_string()));
        }
    }

    // Summary of what needs approval
    if !needs_approval.is_empty() {
        println!(
            "\n {} crate(s) require approval:",
            needs_approval.len()
        );
        println!("   (major version upgrades, downgrades, or new dependencies)");
    }

    if !minor_upgrades.is_empty() {
        println!(
            "\n {} crate(s) are minor/patch upgrades (no approval needed)",
            minor_upgrades.len()
        );
    }

    // Detailed list of crates requiring approval
    if !needs_approval.is_empty() {
        println!("\n========================================");
        println!("CRATES REQUIRING APPROVAL:");
        println!("========================================");
        for (crate_name, reason) in &needs_approval {
            println!("  - {} ({})", crate_name, reason);
        }
        println!("========================================");
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
