#![no_std]
#![allow(dead_code)]
core::arch::global_asm!(
    ".global _start",
    "_start:",
    "mov rdi, [rsp]",     // argc
    "lea rsi, [rsp + 8]", // argv
    "and rsp, -16",       // 16バイト境界に再アライン
    "call user_main",
    "mov rax, 60",
    "mov rdi, 1",
    "syscall",
);

unsafe extern "C" {
    fn user_main(argc: i64, argv: *const *const u8) -> !;
}

pub mod syscall {
    #[inline(always)]
    pub fn read(fd: u64, buf: &mut [u8]) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 0i64 => ret,
                in("rdi") fd,
                in("rsi") buf.as_mut_ptr() as u64,
                in("rdx") buf.len() as u64,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn write(fd: u64, buf: &[u8]) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 1i64 => ret,  // SYS_write
                in("rdi") fd,
                in("rsi") buf.as_ptr() as u64,
                in("rdx") buf.len() as u64,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    pub const O_RDONLY: u64 = 0;
    pub const O_WRONLY: u64 = 1;
    pub const O_RDWR: u64 = 2;
    pub const O_CREAT: u64 = 0o100;
    pub const O_TRUNC: u64 = 0o1000;

    #[inline(always)]
    pub fn open(path: &[u8], flags: u64, mode: u64) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 2i64 => ret,
                in("rdi") path.as_ptr() as u64,
                in("rsi") flags,
                in("rdx") mode,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn close(fd: u64) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 3i64 => ret,
                in("rdi") fd,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn getpid() -> u64 {
        let ret: u64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 39u64 => ret,  // SYS_getpid
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn exit(code: i32) -> ! {
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") 60u64,  // SYS_exit
                in("rdi") code as u64,
                options(noreturn),
            );
        }
    }

    #[inline(always)]
    pub fn sched_yield() {
        unsafe {
            core::arch::asm!(
                "syscall",
                in("rax") 24u64,  // SYS_sched_yield
                out("rcx") _,
                out("r11") _,
            );
        }
    }

    #[inline(always)]
    pub fn mmap(addr: u64, size: usize, prot: u32, flags: u32) -> *mut u8 {
        let ret: u64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 9u64 => ret,
                in("rdi") addr,
                in("rsi") size as u64,
                in("rdx") prot as u64,
                in("r10") flags as u64,
                in("r8")  (-1i64) as u64,
                in("r9")  0u64,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret as *mut u8
    }

    #[inline(always)]
    pub fn munmap(addr: u64, len: usize) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 11i64 => ret, // SYS_munmap
                in("rdi") addr,
                in("rsi") len as u64,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn fork() -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 57i64 => ret,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn wait4(pid: i64, status: *mut i32) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 61i64 => ret,
                in("rdi") pid,
                in("rsi") status as u64,
                in("rdx") 0u64,
                in("r10") 0u64,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn execve(path: &[u8], argv: &[&[u8]]) -> i64 {
        const MAX_ARGV: usize = 16;
        let mut ptrs: [*const u8; MAX_ARGV] = [core::ptr::null(); MAX_ARGV];
        for (i, a) in argv.iter().enumerate().take(MAX_ARGV - 1) {
            ptrs[i] = a.as_ptr();
        }

        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 59i64 => ret,
                in("rdi") path.as_ptr() as u64,
                in("rsi") ptrs.as_ptr() as u64,
                in("rdx") 0u64,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    #[inline(always)]
    pub fn ioctl(fd: u64, request: u64, argp: u64) -> i64 {
        let ret: i64;
        unsafe {
            core::arch::asm!(
                "syscall",
                inout("rax") 16i64 => ret, // SYS_ioctl
                in("rdi") fd,
                in("rsi") request,
                in("rdx") argp,
                out("rcx") _,
                out("r11") _,
            );
        }
        ret
    }

    pub const TIOCGWINSZ: u64 = 0x5413;
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    syscall::exit(1)
}
