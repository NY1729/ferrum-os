#!/bin/bash
set -e

EFI=$1
OVMF=/usr/share/ovmf/OVMF.fd
USER_ELF=${2:-binaries/user.elf}   # 親玉プロセス (init)
HELLO_ELF=${3:-binaries/hello_musl}  
CAT_ELF=${4:-binaries/cat.elf}  
ECHO_ELF=${5:-binaries/echo.elf}  
MMAP_ELF=${6:-binaries/mmap_test.elf}  
DEV_ELF=${7:-binaries/dev_test.elf}  
BUSY_BOX=${8:-binaries/busybox}  

mkdir -p target/esp/EFI/BOOT
cp "$EFI" target/esp/EFI/BOOT/BOOTX64.EFI

cp "$USER_ELF" target/esp/user.elf
cp "$HELLO_ELF" target/esp/hello.elf 
cp "$CAT_ELF" target/esp/cat.elf 
cp "$ECHO_ELF" target/esp/echo.elf 
cp "$MMAP_ELF" target/esp/mmap_test.elf 
cp "$DEV_ELF" target/esp/dev_test.elf 
cp "$BUSY_BOX" target/esp/busybox

qemu-system-x86_64 \
    -bios $OVMF \
    -drive format=raw,file=fat:rw:target/esp \
    -serial stdio \
    -display none \
    -m 4G \
    -no-reboot \
    -no-shutdown \
    -d int,guest_errors -D qemu_debug.log