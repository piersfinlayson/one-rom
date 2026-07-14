#!/usr/bin/env bash
set -e

cd rust
echo "Running tests for Rust crates..."

echo "Testing onerom-app..."
cargo test -p onerom-app
cargo test -p onerom-app -- --ignored

echo "Testing onerom-cli..."
cargo test -p onerom-cli

echo "Testing onerom-config..."
cargo test -p onerom-config

echo "Testing onerom-database..."
cargo test -p onerom-database

echo "Testing onerom-fw..."
cargo test -p onerom-fw

echo "Testing fw-config-gen..."
cargo test -p fw-config-gen

echo "Testing onerom-fw-parser..."
cargo test -p onerom-fw-parser
cargo test -p onerom-fw-parser --no-default-features

echo "Testing onerom-gen..."
cargo test -p onerom-gen

echo "Testing onerom-metadata..."
cargo test -p onerom-protocol

echo "Testing onerom-protocol..."
cargo test -p onerom-protocol

echo "Testing schema-gen..."
cargo test -p schema-gen

