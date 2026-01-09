# Registry Checker

A simple CLI tool to find missing dependencies for offline Cargo registries.

## What it does

This tool scans your Rust project's dependencies and compares them against your offline registry file to identify which crates are missing. It also checks version differences and warns you when a dependency requires approval:

- **Minor/patch version upgrades** (e.g., 1.2.0 → 1.3.0 or 1.2.0 → 1.2.1) can be added without approval
- **Major version upgrades** (e.g., 1.x.x → 2.0.0) require approval
- **New dependencies** require approval

## Installation

```bash
cargo build --release
```

The binary will be available at `target/release/registry_checker`.

## Usage

### Check for missing crates

```bash
registry_checker --registry-file <path-to-registry.txt>
```

### Check a specific project

```bash
registry_checker --manifest-path /path/to/Cargo.toml --registry-file <path-to-registry.txt>
```

### Add missing crates and sort the registry file

```bash
registry_checker --registry-file <path-to-registry.txt> --write
```

## Options

- `-m, --manifest-path <PATH>` - Path to the Cargo.toml of the project (default: ./Cargo.toml)
- `-r, --registry-file <PATH>` - Path to the text file listing your offline registry crates (required)
- `-w, --write` - Add missing crates to the registry file and sort it

## Example

```bash
# Check what's missing
registry_checker -r my-registry.txt

# Update the registry file with missing crates
registry_checker -r my-registry.txt --write
```

### Example Output

```
Scanning project dependencies...
Reading existing registry file: "my-registry.txt"
Found 5 missing crates:
  + serde-1.0.210.crate [minor/patch upgrade from 1.0.195]
  + tokio-2.0.0.crate [WARNING: MAJOR version upgrade from 1.41.0, requires approval]
  + new-crate-0.1.0.crate [WARNING: NEW dependency, requires approval]

⚠️  2 crate(s) require approval:
   (major version upgrades or new dependencies)

✓  1 crate(s) are minor/patch upgrades (no approval needed)

(Run with --write to add these and sort the file)
```
