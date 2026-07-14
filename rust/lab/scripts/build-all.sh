#!/usr/bin/env bash
set -e

rustup target add thumbv8m.main-none-eabihf
cargo build --release
BOARD="fire-40-a" cargo build --release
BOARD="fire-32-a" cargo build --release
BOARD="fire-28-a" cargo build --release
BOARD="fire-28-c" cargo build --release
BOARD="fire-24-e" cargo build --release
