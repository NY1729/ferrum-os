#!/bin/bash
set -e

EFI=$1
OVMF=/usr/share/ovmf/OVMF.fd

mkdir -p target/esp/EFI/BOOT
cp "$EFI" target/esp/EFI/BOOT/BOOTX64.EFI

qemu-system-x86_64 \
    -bios $OVMF \
    -drive format=raw,file=fat:rw:target/esp \
    -serial stdio \
    -display none \
    -m 4G \
    -no-reboot \
    -no-shutdown \
    -d int,guest_errors -D qemu_debug.log