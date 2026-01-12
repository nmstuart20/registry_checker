# Registry Checker

A simple CLI tool to find missing dependencies for offline Cargo registries.

## What it does

This tool uses `cargo tree` to get the exact dependencies needed for your project, then compares them against your offline registry to identify missing crates.

**Key behavior:** The tool uses `cargo tree --edges normal` to get only the dependencies actually needed for building your project (excludes dev and build dependencies). It then checks if your offline registry has the exact versions needed.

When dependencies are missing from the offline registry, the tool categorizes them:

- **Minor/patch version upgrades** (e.g., 1.2.0 → 1.3.0 or 1.2.0 → 1.2.1) can be added without approval
- **Major version upgrades** (e.g., 1.x.x → 2.0.0) require approval
- **Version downgrades** (e.g., 2.0.0 → 1.9.0) require approval
- **New dependencies** require approval

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

#### When offline registry has all dependencies:
```
Scanning project dependencies...
Reading existing registry file: "my-registry.txt"
All dependencies from cargo tree are in the offline registry.
```

#### When some dependencies are missing:
```
Scanning project dependencies...
Reading existing registry file: "my-registry.txt"
Found 4 dependencies missing from the offline registry:
  serde-1.0.210.crate [registry has 1.0.195; minor/patch upgrade needed]
  tokio-2.0.0.crate [WARNING: registry has 1.41.0; MAJOR upgrade needed, requires approval]
  old-lib-0.9.0.crate [WARNING: registry has 1.0.0; DOWNGRADE needed, requires approval]
  new-crate-0.1.0.crate [WARNING: NEW dependency, requires approval]

 3 crate(s) require approval:
   (major version upgrades, downgrades, or new dependencies)

 1 crate(s) are minor/patch upgrades (no approval needed)

========================================
CRATES REQUIRING APPROVAL:
========================================
  - tokio-2.0.0.crate (major upgrade from 1.41.0)
  - old-lib-0.9.0.crate (downgrade needed (has 1.0.0, requires ^0.9))
  - new-crate-0.1.0.crate (new dependency)
========================================

(Run with --write to add these and sort the file)
```
