use anyhow::{Context, Result};
use clap::Parser;
use semver::{Version, VersionReq};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::Command;
use toml::Value;

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

/// Parse version requirements from a Cargo.toml file
/// Returns a map of crate names to their version requirements
fn parse_cargo_toml_requirements(manifest_path: &PathBuf) -> Result<HashMap<String, VersionReq>> {
    let content = fs::read_to_string(manifest_path).context("Could not read Cargo.toml")?;

    let toml_value: Value = content
        .parse()
        .context("Could not parse Cargo.toml as TOML")?;

    let mut requirements: HashMap<String, VersionReq> = HashMap::new();

    // Check all dependency sections
    let dep_sections = ["dependencies", "dev-dependencies", "build-dependencies"];

    for section in dep_sections {
        if let Some(deps) = toml_value.get(section).and_then(|v| v.as_table()) {
            for (name, value) in deps {
                let version_str = match value {
                    Value::String(s) => s.clone(),
                    Value::Table(t) => {
                        if let Some(Value::String(v)) = t.get("version") {
                            v.clone()
                        } else {
                            continue; // Skip deps without version (git, path, etc.)
                        }
                    }
                    _ => continue,
                };

                if let Ok(req) = VersionReq::parse(&version_str) {
                    requirements.insert(name.clone(), req);
                }
            }
        }
    }

    Ok(requirements)
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

/// Parse a line from cargo tree output to extract crate name and version
/// Example: "serde v1.0.228" -> Some(("serde", Version(1.0.228)))
/// Returns None for dependencies from non-crates.io registries
fn parse_cargo_tree_line(line: &str) -> Option<(String, Version)> {
    // Remove tree characters and whitespace
    let cleaned = line.trim().trim_start_matches(['├', '│', '└', '─', ' ']);

    // Check if this dependency is from a non-crates.io registry
    // Alternative registries show as: "crate v1.0.0 (registry `my-registry`)"
    // or "crate v1.0.0 (registry+https://my-registry.com/...)"
    // crates.io dependencies either have no suffix or show as:
    // "crate v1.0.0" or "crate v1.0.0 (registry+https://github.com/rust-lang/crates.io-index)"
    if cleaned.contains("(registry") {
        // Check if it's NOT the crates.io registry
        if !cleaned.contains("crates.io-index") {
            // This is from a different registry, skip it
            return None;
        }
    }

    // Split by space and look for "name vX.Y.Z" pattern
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let name = parts[0];
    let version_str = parts[1].trim_start_matches('v');

    // Parse version (stop at additional info like "(*)" or "(proc-macro)")
    let version_clean = version_str.split_whitespace().next()?;
    let version = Version::parse(version_clean).ok()?;

    Some((name.to_string(), version))
}

fn main() -> Result<()> {
    let args = Args::parse();

    println!("Scanning project dependencies...");

    // Run cargo tree to get the actual dependency tree
    let output = Command::new("cargo")
        .arg("tree")
        .arg("--manifest-path")
        .arg(&args.manifest_path)
        .arg("--edges")
        .arg("normal") // Only normal dependencies (not dev or build)
        .arg("--prefix")
        .arg("none") // Simpler output format
        .output()
        .context("Failed to run cargo tree. Is cargo installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("cargo tree failed: {}", stderr);
    }

    let tree_output =
        String::from_utf8(output.stdout).context("cargo tree output was not valid UTF-8")?;

    // Parse cargo tree output to get all dependencies
    let mut project_deps: HashMap<String, Version> = HashMap::new();

    for line in tree_output.lines() {
        if let Some((name, version)) = parse_cargo_tree_line(line) {
            // cargo tree includes all crates, but we only want external dependencies
            // We'll use a simple heuristic: if it appears multiple times or has a version,
            // it's likely an external dependency. Workspace crates typically appear once at the root.
            project_deps.insert(name, version);
        }
    }

    // Remove the first entry which is usually the workspace root
    // cargo tree shows "workspace_name v0.1.0 (path)" as the first line
    let first_line = tree_output.lines().next().unwrap_or("");
    if let Some((root_name, _)) = parse_cargo_tree_line(first_line) {
        project_deps.remove(&root_name);
    }

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

    // Parse Cargo.toml to get version requirements for direct dependencies
    println!("Parsing Cargo.toml version requirements...");
    let cargo_requirements = parse_cargo_toml_requirements(&args.manifest_path)?;

    // Find missing dependencies: crates from cargo tree where no approved version satisfies the requirement
    let mut missing_deps: HashMap<String, Version> = HashMap::new();

    for (dep_name, needed_version) in &project_deps {
        // First check if there's a version requirement from Cargo.toml (direct dependency)
        // For transitive deps, create a requirement based on the resolved version
        let version_req = cargo_requirements
            .get(dep_name)
            .cloned()
            .unwrap_or_else(|| {
                // For transitive deps, create a caret requirement from the resolved version
                // e.g., if cargo tree shows 1.0.95, create ^1.0.95
                VersionReq::parse(&format!("^{}", needed_version)).unwrap_or(VersionReq::STAR)
            });

        // Check if any version in the registry satisfies the requirement
        let has_compatible_version = registry_versions
            .get(dep_name)
            .map(|versions| versions.iter().any(|v| version_req.matches(v)))
            .unwrap_or(false);

        if !has_compatible_version {
            missing_deps.insert(dep_name.clone(), needed_version.clone());
        }
    }

    if missing_deps.is_empty() {
        println!("All dependencies from cargo tree are in the offline registry.");
        return Ok(());
    }

    // Display what we found with version analysis
    let mut missing_sorted: Vec<_> = missing_deps.iter().collect();
    missing_sorted.sort_by_key(|(name, _)| *name);

    println!(
        "Found {} dependencies missing from the offline registry:",
        missing_deps.len()
    );

    let mut needs_approval: Vec<(String, String)> = Vec::new(); // (crate, reason)

    for (dep_name, needed_version) in &missing_sorted {
        let crate_file = format!("{}-{}.crate", dep_name, needed_version);

        // Get the version requirement for display
        let version_req = cargo_requirements
            .get(*dep_name)
            .map(|r| r.to_string())
            .unwrap_or_else(|| format!("^{}", needed_version));

        if let Some(existing_versions) = registry_versions.get(*dep_name) {
            // Registry has this crate but no version satisfies the requirement
            let versions_str: Vec<String> =
                existing_versions.iter().map(|v| v.to_string()).collect();
            println!(
                "  {} [requirement: \"{}\", registry has: {}; no compatible version]",
                crate_file,
                version_req,
                versions_str.join(", ")
            );
            needs_approval.push((
                crate_file.clone(),
                format!(
                    "requirement \"{}\" not satisfied by registry versions [{}]",
                    version_req,
                    versions_str.join(", ")
                ),
            ));
        } else {
            // New dependency - needs approval
            println!(
                "  {} [WARNING: NEW dependency, requires approval]",
                crate_file
            );
            needs_approval.push((crate_file.clone(), "new dependency".to_string()));
        }
    }

    // Summary of what needs approval
    if !needs_approval.is_empty() {
        println!("\n {} crate(s) require approval:", needs_approval.len());
        println!("   (no compatible version found in registry)");
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
