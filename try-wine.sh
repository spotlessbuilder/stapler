#!/bin/sh
set -ex
cargo build --release --target=x86_64-pc-windows-gnu
wine target/x86_64-pc-windows-gnu/release/stapler.exe

