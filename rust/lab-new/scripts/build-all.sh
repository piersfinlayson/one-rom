#!/usr/bin/env bash
set -e

rustup target add thumbv8m.main-none-eabihf
BOARD="fire-40-a" CHIP_TYPE=27c400 cargo build --release
BOARD="fire-32-a" CHIP_TYPE=27c010 cargo build --release
BOARD="fire-28-a" CHIP_TYPE=27c512 cargo build --release
BOARD="fire-28-a" CHIP_TYPE=23128 CS1=0 CS2=0 CS3=1 cargo build --release
BOARD="fire-24-e" CHIP_TYPE=2364 CS1=0 cargo build --release
