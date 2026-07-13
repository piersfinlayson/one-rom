#!/usr/bin/env bash
set -e

cd rust
echo "Generating documentation for Rust crates..."

echo "Generating documentation for onerom-app..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-app

echo "Generating documentation for onerom-cli..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-cli

echo "Generating documentation for onerom-config..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-config

echo "Generating documentation for onerom-database..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-database

echo "Generating documentation for onerom-fw..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw

echo "Generating documentation for fw-config-gen..."
RUSTDOCFLAGS="-D warnings" cargo doc -p fw-config-gen

echo "Generating documentation for onerom-fw-parser..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-fw-parser

echo "Generating documentation for onerom-gen..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-gen

echo "Generating documentation for onerom-metadata..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-metadata

echo "Generating documentation for onerom-protocol..."
RUSTDOCFLAGS="-D warnings" cargo doc -p onerom-protocol

echo "Generating documentation for schema-gen..."
RUSTDOCFLAGS="-D warnings" cargo doc -p schema-gen
