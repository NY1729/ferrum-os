.PHONY: all build run run-debug clean userland

all: build

userland:
	cd userland && ./build.sh

build: userland
	cargo build -p kernel --release

run: build
	cargo build -p kernel
	@mkdir -p target/esp/EFI/BOOT
	./scripts/launch.sh target/x86_64-unknown-uefi/debug/kernel.efi

clean:
	cargo clean
	rm -f *.elf *.log qemu_trace.log
	rm -rf target/esp/EFI target/esp/*.elf

userland-only:
	cd userland && ./build.sh