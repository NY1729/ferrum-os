#!/bin/bash
set -e
cd "$(dirname "$0")"

USERLD="$(realpath ../binaries/user.ld)"
export RUSTFLAGS="-C link-arg=-T${USERLD} -C link-arg=--entry=_start -C relocation-model=static"

echo "Building all userland binaries..."

cargo build --release --target x86_64-unknown-none \
    --bin init \
    --bin cat \
    --bin echo \
    --bin mmap_test \
    --bin dev_test

# コピー
cp ../target/x86_64-unknown-none/release/init ../binaries/user.elf
cp ../target/x86_64-unknown-none/release/cat ../binaries/cat.elf
cp ../target/x86_64-unknown-none/release/echo ../binaries/echo.elf
cp ../target/x86_64-unknown-none/release/mmap_test ../binaries/mmap_test.elf
cp ../target/x86_64-unknown-none/release/dev_test ../binaries/dev_test.elf

echo "✅ Userland build completed."