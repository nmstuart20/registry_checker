use anyhow::{Context, Result};
use cargo_metadata::MetadataCommand;
use clap::Parser;
use semver::{Version, VersionReq};
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

    // Build a map of dependency names to their version requirements
    // We collect all version requirements across all packages for each dependency
    let mut dep_requirements: HashMap<String, Vec<VersionReq>> = HashMap::new();
    for package in &metadata.packages {
        for dep in &package.dependencies {
            dep_requirements
                .entry(dep.name.clone())
                .or_default()
                .push(dep.req.clone());
        }
    }

    // Get workspace members (the root packages we're actually building)
    let workspace_members: HashSet<_> = metadata.workspace_members.iter().collect();

    // Get the resolve graph - this shows which dependencies are actually used
    let resolve = metadata
        .resolve
        .as_ref()
        .context("No dependency resolution information available")?;

    // Build a map of package ID to its dependencies for efficient traversal
    let mut node_deps_map: HashMap<_, Vec<_>> = HashMap::new();
    for node in &resolve.nodes {
        node_deps_map.insert(
            &node.id,
            node.deps.iter().map(|d| &d.pkg).collect(),
        );
    }

    // Traverse from workspace members to find all actually used packages
    let mut actually_used_packages = HashSet::new();
    let mut to_visit: Vec<_> = workspace_members.iter().copied().collect();

    while let Some(pkg_id) = to_visit.pop() {
        if actually_used_packages.insert(pkg_id) {
            // If this is a new package, add its dependencies to visit
            if let Some(deps) = node_deps_map.get(pkg_id) {
                for dep_id in deps {
                    if !actually_used_packages.contains(dep_id) {
                        to_visit.push(dep_id);
                    }
                }
            }
        }
    }

    // Now only include external dependencies that are actually used
    let project_deps: HashMap<String, Version> = metadata
        .packages
        .iter()
        .filter(|p| p.source.is_some() && actually_used_packages.contains(&p.id))
        .map(|p| (p.name.clone(), p.version.clone()))
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

    // Find what is truly missing: dependencies where the offline registry
    // doesn't have ANY version that satisfies the version requirement
    let mut missing_deps: HashMap<String, Version> = HashMap::new();

    for (dep_name, lock_version) in &project_deps {
        // Check if the offline registry has any version that satisfies the requirement
        let has_satisfying_version = if let Some(requirements) = dep_requirements.get(dep_name) {
            if let Some(available_versions) = registry_versions.get(dep_name) {
                // Check if any available version satisfies any of the requirements
                available_versions.iter().any(|available_version| {
                    requirements
                        .iter()
                        .any(|req| req.matches(available_version))
                })
            } else {
                // No versions of this crate in the registry at all
                false
            }
        } else {
            // No version requirement found (shouldn't happen, but be safe)
            // Fall back to checking for exact version
            registry_versions
                .get(dep_name)
                .map(|versions| versions.contains(lock_version))
                .unwrap_or(false)
        };

        if !has_satisfying_version {
            missing_deps.insert(dep_name.clone(), lock_version.clone());
        }
    }

    if missing_deps.is_empty() {
        println!("All dependencies can be satisfied by the offline registry.");
        println!("(The offline registry has compatible versions for all requirements)");
        return Ok(());
    }

    // Display what we found with version analysis
    let mut missing_sorted: Vec<_> = missing_deps.iter().collect();
    missing_sorted.sort_by_key(|(name, _)| *name);

    println!(
        "Found {} dependencies that cannot be satisfied by the offline registry:",
        missing_deps.len()
    );

    let mut needs_approval: Vec<(String, String)> = Vec::new(); // (crate, reason)
    let mut minor_upgrades: Vec<String> = Vec::new();

    for (dep_name, needed_version) in &missing_sorted {
        let crate_file = format!("{}-{}.crate", dep_name, needed_version);

        if let Some(existing_versions) = registry_versions.get(*dep_name) {
            // Find the latest existing version
            let latest_existing = existing_versions.iter().max().unwrap();

            // Get the version requirements for this dependency
            let requirements = dep_requirements.get(*dep_name);
            let req_str = requirements
                .map(|reqs| {
                    reqs.iter()
                        .map(|r| r.to_string())
                        .collect::<Vec<_>>()
                        .join(" or ")
                })
                .unwrap_or_else(|| "unknown".to_string());

            if **needed_version < *latest_existing {
                // Version downgrade - needs approval
                println!(
                    "  {} [WARNING: requires {}, but registry has {}; DOWNGRADE needed, requires approval]",
                    crate_file, req_str, latest_existing
                );
                needs_approval.push((
                    crate_file.clone(),
                    format!(
                        "downgrade needed (has {}, requires {})",
                        latest_existing, req_str
                    ),
                ));
            } else if needed_version.major != latest_existing.major {
                // Major version upgrade - needs approval
                println!(
                    "  {} [WARNING: requires {}, but registry only has {}; MAJOR upgrade needed, requires approval]",
                    crate_file, req_str, latest_existing
                );
                needs_approval.push((
                    crate_file.clone(),
                    format!("major upgrade from {}", latest_existing),
                ));
            } else if needed_version.minor != latest_existing.minor
                || needed_version.patch != latest_existing.patch
            {
                // Minor or patch version upgrade - OK to add
                println!(
                    "  {} [requires {}, registry has {}; minor/patch upgrade needed]",
                    crate_file, req_str, latest_existing
                );
                minor_upgrades.push(crate_file.clone());
            } else {
                // Same version? Shouldn't happen based on our logic, but just in case
                println!("  {} [requires {}]", crate_file, req_str);
            }
        } else {
            // New dependency - needs approval
            let req_str = dep_requirements
                .get(*dep_name)
                .map(|reqs| {
                    reqs.iter()
                        .map(|r| r.to_string())
                        .collect::<Vec<_>>()
                        .join(" or ")
                })
                .unwrap_or_else(|| "unknown".to_string());
            println!(
                "  {} [WARNING: requires {}; NEW dependency, requires approval]",
                crate_file, req_str
            );
            needs_approval.push((crate_file.clone(), "new dependency".to_string()));
        }
    }

    // Summary of what needs approval
    if !needs_approval.is_empty() {
        println!("\n {} crate(s) require approval:", needs_approval.len());
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
        println!("\nMerging and sorting registry file...");

        // Convert missing deps to crate file format
        let missing_crate_files: HashSet<String> = missing_deps
            .iter()
            .map(|(name, version)| format!("{}-{}.crate", name, version))
            .collect();

        // 1. Combine existing and missing
        let mut full_list: Vec<String> = existing_registry
            .union(&missing_crate_files)
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
