# Registry Checker

A simple CLI tool to find missing dependencies for offline Cargo registries.

## What it does

This tool scans your Rust project's dependencies and compares them against your offline registry file to identify which crates are missing.

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
