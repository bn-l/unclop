# unclop task runner. `just` lists the recipes.

# List the recipes
default:
    @just --list --unsorted

# Build the debug binary
build:
    cargo build

# Run every test
test:
    cargo test

# Run only the extraction snapshot tests
test-extract:
    cargo test --test extract

# Run only the end-to-end CLI tests
test-cli:
    cargo test --test cli

# Re-record the extraction snapshots after a query or fixture change, then show what moved
snapshots:
    INSTA_UPDATE=always cargo test --test extract
    git status --short tests/snapshots

# Format the code
fmt:
    cargo fmt

# Clippy on every target with warnings as errors
lint:
    cargo clippy --all-targets -- -D warnings

# What CI runs: formatting, clippy, tests
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# Install the binary into ~/.cargo/bin
install:
    cargo install --path . --locked

# Print the full help as an agent sees it
help: build
    ./target/debug/unclop --help

# Print the tree-sitter parse of one file, for writing or fixing queries
tree file: build
    ./target/debug/unclop debug-tree {{file}}

# Try the tool on a copy of DIR with a throwaway config: prints the counts, the first chunk and the status
try dir: build
    #!/usr/bin/env bash
    set -euo pipefail
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    cp -R "{{dir}}" "$tmp/proj"
    rm -f "$tmp/proj/.unclop.jsonl"
    export UNCLOP_CONFIG="$tmp/config.yaml"
    ./target/debug/unclop -C "$tmp/proj" init
    echo
    ./target/debug/unclop -C "$tmp/proj" next | head -40
    echo
    ./target/debug/unclop -C "$tmp/proj" status | tail -6 || true
