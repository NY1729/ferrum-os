#![no_std]
#![no_main]

use libuser::syscall;

const PROT_READ: u32 = 1;
const PROT_WRITE: u32 = 2;
const MAP_PRIVATE: u32 = 0x02;
const MAP_ANONYMOUS: u32 = 0x20;

#[unsafe(no_mangle)]
pub extern "C" fn user_main(_argc: i64, _argv: *const *const u8) -> ! {
    syscall::write(1, b"mmap_test: start\n");

    let size = 3 * 4096; // 3ページ分、demand pagingが複数ページにまたがるか確認
    let ptr = syscall::mmap(0, size, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS);
    if ptr.is_null() || (ptr as i64) == -1 {
        syscall::write(1, b"mmap_test: mmap failed\n");
        syscall::exit(1);
    }
    syscall::write(1, b"mmap_test: mapped ok, writing...\n");

    // 1〜3ページ目それぞれに書き込み(各ページで個別にページフォルトが発生するはず)
    unsafe {
        *ptr = 0xAA;
        *ptr.add(4096) = 0xBB;
        *ptr.add(8192) = 0xCC;
    }
    syscall::write(1, b"mmap_test: write ok, reading back...\n");

    let ok = unsafe { *ptr == 0xAA && *ptr.add(4096) == 0xBB && *ptr.add(8192) == 0xCC };
    if ok {
        syscall::write(1, b"mmap_test: read back OK\n");
    } else {
        syscall::write(1, b"mmap_test: read back MISMATCH\n");
    }

    let r = syscall::munmap(ptr as u64, size);
    if r == 0 {
        syscall::write(1, b"mmap_test: munmap OK\n");
    } else {
        syscall::write(1, b"mmap_test: munmap FAILED\n");
    }

    syscall::write(1, b"mmap_test: testing two separate mmap(NULL) calls...\n");
    let ptr_a = syscall::mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS);
    let ptr_b = syscall::mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS);
    if ptr_a != ptr_b && !ptr_a.is_null() && !ptr_b.is_null() {
        syscall::write(1, b"mmap_test: two mmaps got DIFFERENT addresses: OK\n");
    } else {
        syscall::write(1, b"mmap_test: two mmaps COLLIDED: BUG\n");
    }
    unsafe {
        *ptr_a = 0x11;
        *ptr_b = 0x22;
    }
    let ok2 = unsafe { *ptr_a == 0x11 && *ptr_b == 0x22 };
    if ok2 {
        syscall::write(1, b"mmap_test: both regions independently writable: OK\n");
    } else {
        syscall::write(1, b"mmap_test: regions corrupted each other: BUG\n");
    }
    syscall::munmap(ptr_a as u64, 4096);
    syscall::munmap(ptr_b as u64, 4096);

    syscall::write(1, b"mmap_test: done\n");
    syscall::exit(0)
}
