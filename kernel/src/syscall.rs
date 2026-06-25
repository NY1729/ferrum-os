#![allow(dead_code)]
use crate::fs::vfs::Vfs;
use crate::paging::PageAllocator;
use crate::process::ProcessState;
use core::arch::asm;

#[no_mangle]
pub static mut KERNEL_GS_SCRATCH: [u64; 2] = [0u64; 2];

pub fn update_kernel_rsp(kstack_top: u64) {
    unsafe {
        KERNEL_GS_SCRATCH[0] = kstack_top;
    }
}

pub fn init() {
    unsafe {
        enable_syscall_msr();
        let gs_base = &raw const KERNEL_GS_SCRATCH as u64;
        asm!("mov gs, {z:x}", z = in(reg) 0u16, options(nostack));
        asm!("wrmsr", in("ecx") 0xC000_0101u32,
             in("eax") gs_base as u32, in("edx") (gs_base >> 32) as u32, options(nostack));
        asm!("wrmsr", in("ecx") 0xC000_0102u32,
             in("eax") gs_base as u32, in("edx") (gs_base >> 32) as u32, options(nostack));
        crate::serial_println!("[syscall] GS.base={:#x}", gs_base);
    }
}

unsafe fn enable_syscall_msr() {
    let eax: u32;
    let edx: u32;
    asm!("rdmsr", in("ecx") 0xC000_0080u32, out("eax") eax, out("edx") edx, options(nostack));
    let efer = ((edx as u64) << 32) | (eax as u64) | 1;
    asm!("wrmsr", in("ecx") 0xC000_0080u32,
         in("eax") efer as u32, in("edx") (efer >> 32) as u32, options(nostack));
    crate::serial_println!("[syscall] EFER={:#x} (SCE)", efer);

    let star: u64 = (0x0008u64 << 32) | (0x0020u64 << 48);
    asm!("wrmsr", in("ecx") 0xC000_0081u32,
         in("eax") star as u32, in("edx") (star >> 32) as u32);

    let entry = syscall_entry as *const () as u64;
    crate::serial_println!("[syscall] LSTAR={:#x}", entry);
    asm!("wrmsr", in("ecx") 0xC000_0082u32,
         in("eax") entry as u32, in("edx") (entry >> 32) as u32, options(nostack));

    let fmask: u64 = (1 << 9) | (1 << 10);
    asm!("wrmsr", in("ecx") 0xC000_0084u32,
         in("eax") fmask as u32, in("edx") 0u32, options(nostack));
}

// ── SyscallFrame ──────────────────────────────────────────────────
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SyscallFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r9: u64,
    pub r8: u64,
    pub r10: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rax: u64,
    pub user_rsp: u64,
    pub r11: u64,
    pub rcx: u64,
}

#[unsafe(naked)]
pub unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        "swapgs",
        "mov gs:[8], rsp",
        "mov rsp, gs:[0]",

        "push rcx",
        "push r11",
        "push qword ptr gs:[8]",
        "push rax",
        "push rdi",
        "push rsi",
        "push rdx",
        "push r10",
        "push r8",
        "push r9",
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov rbp, rsp",

        "and rsp, -16",
        "mov rdi, rbp",
        "call {handler}",

        // rbp = フレーム先頭(r15位置), raxフィールドは +0x60
        "mov [rbp + 0x60], rax",

        "mov rsp, rbp",
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        "pop r9",
        "pop r8",
        "pop r10",
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "pop rax",

        "mov rcx, [rsp + 16]",
        "mov r11, [rsp + 8]",
        "mov rsp, [rsp]",

        "swapgs",
        "sysretq",

        handler = sym syscall_handler,
    );
}

// ── Linux x86-64 syscall 番号 ─────────────────────────────────────
pub mod nr {
    pub const READ: u64 = 0;
    pub const WRITE: u64 = 1;
    pub const OPEN: u64 = 2;
    pub const CLOSE: u64 = 3;
    pub const STAT: u64 = 4;
    pub const FSTAT: u64 = 5;
    pub const LSTAT: u64 = 6;
    pub const LSEEK: u64 = 8;
    pub const MMAP: u64 = 9;
    pub const MPROTECT: u64 = 10;
    pub const MUNMAP: u64 = 11;
    pub const BRK: u64 = 12;
    pub const RT_SIGACTION: u64 = 13;
    pub const RT_SIGPROCMASK: u64 = 14;
    pub const IOCTL: u64 = 16;
    pub const PREAD64: u64 = 17;
    pub const WRITEV: u64 = 20;
    pub const ACCESS: u64 = 21;
    pub const YIELD: u64 = 24;
    pub const DUP: u64 = 32;
    pub const DUP2: u64 = 33;
    pub const NANOSLEEP: u64 = 35;
    pub const GETPID: u64 = 39;
    pub const SENDFILE: u64 = 40;
    pub const CLONE: u64 = 56;
    pub const FORK: u64 = 57;
    pub const EXECVE: u64 = 59;
    pub const EXIT: u64 = 60;
    pub const WAIT4: u64 = 61;
    pub const UNAME: u64 = 63;
    pub const FCNTL: u64 = 72;
    pub const GETCWD: u64 = 79;
    pub const CHDIR: u64 = 80;
    pub const RENAME: u64 = 82;
    pub const MKDIR: u64 = 83;
    pub const UNLINK: u64 = 87;
    pub const GETUID: u64 = 102;
    pub const GETGID: u64 = 104;
    pub const SETUID: u64 = 105;
    pub const SETGID: u64 = 106;
    pub const GETEUID: u64 = 107;
    pub const GETEGID: u64 = 108;
    pub const GETPPID: u64 = 110;
    pub const ARCH_PRCTL: u64 = 158;
    pub const GETTID: u64 = 186;
    pub const TIME: u64 = 201;
    pub const FUTEX: u64 = 202;
    pub const GETDENTS64: u64 = 217;
    pub const SET_TID_ADDRESS: u64 = 218;
    pub const CLOCK_GETTIME: u64 = 228;
    pub const CLOCK_GETRES: u64 = 229;
    pub const CLOCK_NANOSLEEP: u64 = 230;
    pub const EXIT_GROUP: u64 = 231;
    pub const OPENAT: u64 = 257;
    pub const NEWFSTATAT: u64 = 262;
    pub const UNLINKAT: u64 = 263;
    pub const READLINKAT: u64 = 267;
    pub const FACCESSAT: u64 = 269;
    pub const SET_ROBUST_LIST: u64 = 273;
    pub const PRLIMIT64: u64 = 302;
    pub const GETRANDOM: u64 = 318;
    pub const STATX: u64 = 332;
    pub const RSEQ: u64 = 334;

    pub const ARCH_SET_GS: u64 = 0x1001;
    pub const ARCH_SET_FS: u64 = 0x1002;
    pub const ARCH_GET_FS: u64 = 0x1003;
    pub const ARCH_GET_GS: u64 = 0x1004;
}

pub extern "sysv64" fn syscall_handler(frame: &mut SyscallFrame) -> u64 {
    let nr = frame.rax;

    match nr {
        nr::READ => syscall_read(frame),
        nr::WRITE => syscall_write(frame),
        nr::OPEN => syscall_open(frame),
        nr::CLOSE => syscall_close(frame),
        nr::STAT | nr::LSTAT => syscall_stat(frame),
        nr::FSTAT => syscall_fstat(frame),
        nr::NEWFSTATAT => syscall_newfstatat(frame),
        nr::FCNTL => syscall_fcntl(frame),
        nr::GETPID => unsafe {
            (&raw mut crate::SCHEDULER)
                .as_mut()
                .unwrap()
                .current_mut()
                .map(|p| p.pid)
                .unwrap_or(0)
        },
        nr::GETPPID => unsafe {
            (&raw mut crate::SCHEDULER)
                .as_mut()
                .unwrap()
                .current_mut()
                .map(|p| p.parent_pid)
                .unwrap_or(0)
        },
        nr::GETEUID => 0, // ルートユーザーとして振る舞う
        nr::GETCWD => syscall_getcwd(frame),
        nr::FORK | nr::CLONE => syscall_fork(frame),
        nr::WAIT4 => syscall_wait4(frame),
        nr::CHDIR => syscall_chdir(frame),
        nr::MKDIR => syscall_mkdir(frame),
        nr::RENAME => syscall_rename(frame),
        nr::UNLINK => syscall_unlink(frame),
        nr::UNLINKAT => syscall_unlinkat(frame),
        nr::SENDFILE => {
            // sendfile(out_fd, in_fd, offset, count)
            // 未実装: EINVAL を返して呼び出し元に read+write へのフォールバックを促す
            (-22i64) as u64
        }
        nr::EXECVE => syscall_execve(frame),
        nr::EXIT | nr::EXIT_GROUP => syscall_exit(frame),
        nr::YIELD => {
            x86_64::instructions::interrupts::enable();
            crate::process::schedule(&raw mut crate::SCHEDULER);
            x86_64::instructions::interrupts::disable();
            0
        }
        nr::MMAP => syscall_mmap(frame),
        nr::MUNMAP => syscall_munmap(frame),
        nr::MPROTECT => 0,
        nr::BRK => syscall_brk(frame),
        nr::IOCTL => syscall_ioctl(frame),
        nr::RT_SIGACTION | nr::RT_SIGPROCMASK => 0, // 暫定的に常に成功を返す
        nr::GETDENTS64 => syscall_getdents64(frame),
        nr::SET_TID_ADDRESS => unsafe {
            (&raw mut crate::SCHEDULER)
                .as_mut()
                .unwrap()
                .current_mut()
                .map(|p| p.pid)
                .unwrap_or(0)
        },
        nr::TIME => {
            let tloc = frame.rdi as *mut u64;
            let ticks = crate::interrupts::timer_ticks();
            let sec = ticks / 100; // 100Hz
            if !tloc.is_null() {
                let sched = unsafe { &*&raw const crate::SCHEDULER };
                write_to_user(sched, tloc as u64, &sec.to_ne_bytes());
            }
            sec
        }
        nr::ARCH_PRCTL => syscall_arch_prctl(frame),
        nr::WRITEV => syscall_writev(frame),
        nr::PREAD64 => syscall_pread64(frame),
        nr::ACCESS => syscall_access(frame),
        nr::DUP => syscall_dup(frame),
        nr::DUP2 => syscall_dup2(frame),
        nr::NANOSLEEP | nr::CLOCK_NANOSLEEP => 0,
        nr::GETUID | nr::GETGID | nr::GETEGID => 0,
        nr::SETUID | nr::SETGID => 0,
        nr::GETTID => unsafe {
            (&raw mut crate::SCHEDULER)
                .as_mut()
                .unwrap()
                .current_mut()
                .map(|p| p.pid)
                .unwrap_or(1)
        },
        nr::UNAME => syscall_uname(frame),
        nr::FUTEX => syscall_futex(frame),
        nr::CLOCK_GETTIME => syscall_clock_gettime(frame),
        nr::CLOCK_GETRES => {
            let tp = frame.rsi as *mut i64;
            if !tp.is_null() {
                let sched = unsafe { &*&raw const crate::SCHEDULER };
                let data: [i64; 2] = [0, 10_000_000]; // sec=0, nsec=10ms
                let bytes = unsafe { core::slice::from_raw_parts(data.as_ptr() as *const u8, 16) };
                write_to_user(sched, tp as u64, bytes);
            }
            0
        }
        nr::LSEEK => syscall_lseek(frame),
        nr::OPENAT => syscall_openat(frame),
        nr::READLINKAT => (-2i64) as u64, // ENOENT
        nr::FACCESSAT => syscall_faccessat(frame),
        nr::SET_ROBUST_LIST => 0,
        nr::PRLIMIT64 => 0,
        nr::GETRANDOM => syscall_getrandom(frame),
        nr::RSEQ => (-38i64) as u64,
        nr::STATX => (-38i64) as u64, // ENOSYS: vfs では未実装
        _ => {
            crate::serial_println!("[syscall] unimplemented syscall nr={}", nr);
            (-38i64) as u64 // ENOSYS
        }
    }
}

// ── Syscall Implementations ───────────────────────────────────────

fn syscall_read(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let buf = frame.rsi as *mut u8;
    let count = frame.rdx as usize;

    if count == 0 {
        return 0;
    }

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let (kind, readable) = {
        let desc = entry.lock();
        (desc.kind, desc.readable)
    };

    if matches!(kind, crate::fd::FdKind::Directory) {
        return (-21i64) as u64; // EISDIR
    }

    if !readable {
        return (-9i64) as u64;
    }

    match kind {
        crate::fd::FdKind::Serial => {
            x86_64::instructions::interrupts::enable();
            let user_buf = unsafe { core::slice::from_raw_parts_mut(buf, count) };
            let mut n = 0usize;
            while n < count {
                let b = crate::driver::serial::read_byte_blocking();
                if b == b'\r' || b == b'\n' {
                    crate::serial_print!("\r\n");
                } else if b == 0x08 || b == 0x7F {
                    // BS or DEL
                    if n > 0 {
                        crate::serial_print!("\x08 \x08");
                        n -= 1;
                    }
                    continue;
                } else {
                    crate::serial_print!("{}", b as char);
                }

                let mapped_byte = if b == b'\r' { b'\n' } else { b };
                user_buf[n] = mapped_byte;
                n += 1;

                if mapped_byte == b'\n' {
                    break;
                }
            }
            n as u64
        }
        crate::fd::FdKind::File => {
            let mut desc = entry.lock();
            let offset = desc.offset;
            let read_result = {
                let node = desc.node.as_ref().expect("File fd must have node");
                let user_buf = unsafe { core::slice::from_raw_parts_mut(buf, count) };
                node.read(offset, user_buf)
            };
            match read_result {
                Ok(n) => {
                    desc.offset += n;
                    n as u64
                }
                Err(_) => (-5i64) as u64, // EIO
            }
        }
        crate::fd::FdKind::Directory => (-21i64) as u64,
        crate::fd::FdKind::DevNull => 0, // EOF
        crate::fd::FdKind::DevZero => {
            let user_buf = unsafe { core::slice::from_raw_parts_mut(buf, count) };
            user_buf.fill(0);
            count as u64
        }
    }
}

fn syscall_write(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let buf = frame.rsi as *const u8;
    let len = frame.rdx as usize;
    if len == 0 {
        return 0;
    }

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let (kind, writable) = {
        let desc = entry.lock();
        (desc.kind, desc.writable)
    };
    if !writable {
        return (-9i64) as u64;
    }

    let user_buf = unsafe { core::slice::from_raw_parts(buf, len) };

    let result = match kind {
        crate::fd::FdKind::Serial => {
            for &b in user_buf {
                crate::serial_print!("{}", b as char);
            }
            len as u64
        }
        crate::fd::FdKind::File => {
            let mut desc = entry.lock();
            let offset = if desc.append {
                desc.node.as_ref().map(|n| n.stat().size).unwrap_or(0)
            } else {
                desc.offset
            };
            let write_result = {
                let node = desc.node.as_mut().expect("File fd must have node");
                node.write(offset, user_buf)
            };
            match write_result {
                Ok(n) => {
                    desc.offset += n;
                    n as u64
                }
                Err(_) => (-5i64) as u64,
            }
        }
        crate::fd::FdKind::Directory => (-21i64) as u64,
        crate::fd::FdKind::DevNull => len as u64,
        crate::fd::FdKind::DevZero => len as u64,
    };
    result
}

mod open_flags {
    pub const O_ACCMODE: u64 = 0x3;
    pub const O_WRONLY: u64 = 0x1;
    pub const O_RDWR: u64 = 0x2;
    pub const O_CREAT: u64 = 0o100;
    pub const O_TRUNC: u64 = 0o1000;
    pub const O_APPEND: u64 = 0o2000;
}

fn syscall_open(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let cwd = unsafe {
        (&raw mut crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);

    let flags = frame.rsi;
    const O_DIRECTORY: u64 = 0o200000;

    if flags & O_DIRECTORY != 0 || path == "/" {
        let names = crate::ROOTFS.lock().list_dir(&path);
        let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
        let p = match sched.current_mut() {
            Some(p) => p,
            None => return (-1i64) as u64,
        };
        let file = crate::fd::OpenFile::directory(names);
        return match p.fd_table.alloc(file) {
            Some(fd) => fd as u64,
            None => (-24i64) as u64,
        };
    }

    let special_kind = match path.as_str() {
        "/dev/null" => Some(crate::fd::FdKind::DevNull),
        "/dev/zero" => Some(crate::fd::FdKind::DevZero),
        "/dev/tty" => Some(crate::fd::FdKind::Serial),
        _ => None,
    };

    if let Some(kind) = special_kind {
        let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
        let p = match sched.current_mut() {
            Some(p) => p,
            None => return (-1i64) as u64,
        };
        let file = crate::fd::OpenFile::device(kind);
        return match p.fd_table.alloc(file) {
            Some(fd) => fd as u64,
            None => (-24i64) as u64,
        };
    }

    if path == "/proc/self/maps" || path == "/proc/self/status" {
        let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
        let p = match sched.current_mut() {
            Some(p) => p,
            None => return (-1i64) as u64,
        };
        let content = if path == "/proc/self/maps" {
            crate::fs::procfs::format_maps(p)
        } else {
            crate::fs::procfs::format_status(p)
        };
        let node: alloc::boxed::Box<dyn crate::fs::vfs::VfsNode> =
            alloc::boxed::Box::new(crate::fs::procfs::ProcfsNode::new(content.into_bytes()));
        let file = crate::fd::OpenFile::file(node, true, false, false);
        return match p.fd_table.alloc(file) {
            Some(fd) => fd as u64,
            None => (-24i64) as u64,
        };
    }

    let acc = flags & open_flags::O_ACCMODE;
    let readable = acc != open_flags::O_WRONLY;
    let writable = acc == open_flags::O_WRONLY || acc == open_flags::O_RDWR;

    let mut rootfs = crate::ROOTFS.lock();
    let mut node = rootfs.open(&path);

    if node.is_none() {
        if flags & open_flags::O_CREAT != 0 {
            crate::serial_println!("[open] O_CREAT: creating '{}'", path);
            match rootfs.write_file(&path, b"") {
                Ok(_) => crate::serial_println!("[open] O_CREAT: write_file OK"),
                Err(e) => {
                    crate::serial_println!("[open] O_CREAT: write_file failed: {}", e);
                    return (-5i64) as u64;
                }
            }
            node = rootfs.open(&path);
            crate::serial_println!("[open] O_CREAT: open after create = {}", node.is_some());
        }
    } else if flags & open_flags::O_TRUNC != 0 && writable {
        let _ = rootfs.write_file(&path, b"");
        node = rootfs.open(&path);
    }
    drop(rootfs);

    let node = match node {
        Some(n) => n,
        None => return (-2i64) as u64,
    };

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    let append = flags & open_flags::O_APPEND != 0;
    let file = crate::fd::OpenFile::file(node, readable, writable, append);
    match p.fd_table.alloc(file) {
        Some(fd) => fd as u64,
        None => (-24i64) as u64,
    }
}

fn syscall_close(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    if p.fd_table.close(fd) {
        0
    } else {
        (-9i64) as u64
    }
}

#[repr(C)]
struct LinuxStat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    __pad0: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    __unused: [i64; 3],
}

const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;

fn fill_stat(file_type: crate::fs::vfs::FileType, size: usize) -> LinuxStat {
    let mut st: LinuxStat = unsafe { core::mem::zeroed() };
    st.st_mode = match file_type {
        crate::fs::vfs::FileType::Directory => S_IFDIR | 0o755,
        crate::fs::vfs::FileType::Regular => S_IFREG | 0o644,
    };
    st.st_nlink = 1;
    st.st_size = size as i64;
    st.st_blksize = 4096;
    st.st_blocks = (size as i64 + 511) / 512;
    st
}

fn syscall_stat(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let cwd = unsafe {
        (&raw mut crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };

    // ★ 本格的なパス解決を実行
    let path = resolve_path(&cwd, &raw_path);
    let statbuf_ptr = frame.rsi as *mut LinuxStat;

    if path == "/" || !crate::ROOTFS.lock().list_dir(&path).is_empty() {
        let st = fill_stat(crate::fs::vfs::FileType::Directory, 0);
        unsafe { core::ptr::write_unaligned(statbuf_ptr, st) };
        return 0;
    }

    let node = {
        let rootfs = crate::ROOTFS.lock();
        rootfs.open(&path)
    };
    match node {
        Some(n) => {
            let s = n.stat();
            let st = fill_stat(s.file_type, s.size);
            unsafe { core::ptr::write_unaligned(statbuf_ptr, st) };
            0
        }
        None => (-2i64) as u64,
    }
}

fn syscall_fcntl(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let cmd = frame.rsi;
    let arg = frame.rdx as usize;

    const F_DUPFD: u64 = 0;
    const F_GETFD: u64 = 1;
    const F_SETFD: u64 = 2;
    const F_GETFL: u64 = 3;
    const F_SETFL: u64 = 4;
    const F_DUPFD_CLOEXEC: u64 = 1030;

    match cmd {
        F_DUPFD | F_DUPFD_CLOEXEC => {
            let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
            let p = match sched.current_mut() {
                Some(p) => p,
                None => return (-1i64) as u64,
            };
            let entry = match p.fd_table.get(fd) {
                Some(e) => e,
                None => return (-9i64) as u64,
            };
            match p.fd_table.alloc_from(entry, arg) {
                Some(newfd) => newfd as u64,
                None => (-24i64) as u64,
            }
        }
        F_GETFD => 0,
        F_SETFD => 0,
        F_GETFL => 0,
        F_SETFL => 0,
        _ => 0,
    }
}

fn syscall_fork(frame: &mut SyscallFrame) -> u64 {
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let new_pid = sched.alloc_pid();
    let parent_idx = sched.current_idx();
    let child = if let Some(Some(parent)) = sched.processes.get_mut(parent_idx) {
        parent.fork(new_pid, frame)
    } else {
        None
    };

    match child {
        Some(child) => {
            let child_pid = child.pid;
            let add_result = sched.add_process(child);
            add_result.expect("fork: no scheduler slot");
            child_pid as u64
        }
        None => (-12i64) as u64, // ENOMEM
    }
}

fn syscall_execve(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };

    let cwd = unsafe {
        (&raw mut crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);

    let argv = read_user_cstr_array(frame.rsi as *const u64, 32);
    let mut envp = read_user_cstr_array(frame.rdx as *const u64, 64);

    if envp.is_empty() {
        envp.push(alloc::string::String::from("PATH=/"));
    }

    let node = {
        let rootfs = crate::ROOTFS.lock();
        rootfs.open(&path)
    };
    let node = match node {
        Some(n) => n,
        None => {
            crate::serial_println!("[syscall] execve: '{}' not found", path);
            return (-2i64) as u64;
        }
    };

    let elf_data: alloc::vec::Vec<u8> = node.read_all();

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    {
        let mut alloc = crate::ALLOCATOR.lock();
        p.address_space.clear_user(&mut *alloc);
    }

    let entry = match p
        .address_space
        .load_elf(&elf_data, &mut *crate::ALLOCATOR.lock())
    {
        Ok(e) => e,
        Err(e) => {
            crate::serial_println!("[syscall] execve: load failed: {}", e);
            return (-8i64) as u64; // ENOEXEC
        }
    };

    let (elf_phdr_vaddr, elf_phent, elf_phnum) = {
        #[repr(C, packed)]
        struct Ehdr {
            _ident: [u8; 16],
            _type: u16,
            _machine: u16,
            _version: u32,
            e_entry: u64,
            e_phoff: u64,
            _shoff: u64,
            _flags: u32,
            _ehsize: u16,
            e_phentsize: u16,
            e_phnum: u16,
            _rest: [u16; 3],
        }
        let ehdr = unsafe { &*(elf_data.as_ptr() as *const Ehdr) };
        let phoff = { ehdr.e_phoff };
        let phentsize = { ehdr.e_phentsize } as u64;
        let phnum = { ehdr.e_phnum } as u64;
        // busybox は ET_EXEC で 0x400000 固定ロードのため PT_PHDR がない。
        // AT_PHDR は auxv から除外済み（0 渡し）なので phdr_vaddr は実質未使用だが
        // 将来的には PT_PHDR セグメントか PT_LOAD の p_vaddr + phoff で計算すべき。
        (0x400000u64 + phoff, phentsize, phnum)
    };

    {
        let mut highest_end = 0u64;
        for vma in p.address_space.vmas.iter() {
            if vma.kind != crate::vma::VMAKind::Stack && vma.end > highest_end {
                highest_end = vma.end;
            }
        }
        let brk_start = (highest_end + 0xFFF) & !0xFFF;
        p.brk_start = brk_start;
        p.brk_current = brk_start;
    }

    let ustack_virt = 0x0000_7fff_0000u64;
    let ustack_pages = 128usize;
    {
        let ustack_base = ustack_virt - (ustack_pages as u64) * 0x1000;
        let flags = crate::paging::PageFlags::PRESENT
            | crate::paging::PageFlags::WRITABLE
            | crate::paging::PageFlags::USER
            | crate::paging::PageFlags::NO_EXEC;
        let mut alloc = crate::ALLOCATOR.lock();
        for i in 0..ustack_pages {
            let addr = ustack_base + (i as u64) * 0x1000;
            let (_, phys) = alloc.alloc_page().expect("execve: ustack alloc");
            p.address_space
                .page_table
                .map(addr, phys, flags, &mut *alloc)
                .expect("execve: ustack map");
        }
        p.address_space
            .add_vma(crate::vma::VMA::new(
                ustack_base,
                ustack_virt + 0x1000,
                crate::vma::VMAFlags::rw(),
                crate::vma::VMAKind::Stack,
            ))
            .ok();
    }

    let new_rsp = build_argv_envp_stack(
        ustack_virt,
        &p.address_space.page_table,
        &argv,
        &envp,
        entry,
        elf_phdr_vaddr,
        elf_phent,
        elf_phnum,
    );

    p.fs_base = 0;
    unsafe {
        crate::syscall::write_fs_base(0);
    }

    frame.rax = 0;
    frame.rbx = 0;
    frame.rcx = entry; // RIP
    frame.rdx = 0; // atexit pointer (0 = 登録なし)
    frame.rsi = 0;
    frame.rdi = 0;
    frame.rbp = 0;
    frame.r8 = 0;
    frame.r9 = 0;
    frame.r10 = 0;
    frame.r11 = 0x202; // RFLAGS (割り込み許可)
    frame.r12 = 0;
    frame.r13 = 0;
    frame.r14 = 0;
    frame.r15 = 0;
    frame.user_rsp = new_rsp;

    0
}

fn syscall_wait4(frame: &mut SyscallFrame) -> u64 {
    let target_pid = frame.rdi as i32 as i64;
    let status_ptr = frame.rsi as *mut i32;
    let options = frame.rdx; // 第3引数: options (WNOHANG など)
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let caller_pid = match sched.current_mut() {
        Some(p) => p.pid,
        None => return (-1i64) as u64,
    };

    loop {
        let zombie_idx = sched.processes.iter().enumerate().find_map(|(i, slot)| {
            slot.as_ref()
                .filter(|p| {
                    if p.parent_pid != caller_pid || p.state != crate::process::ProcessState::Dead {
                        return false;
                    }
                    target_pid == -1
                        || target_pid == 0
                        || target_pid < -1
                        || (p.pid as i64) == target_pid
                })
                .map(|_| i)
        });

        if let Some(idx) = zombie_idx {
            let mut zombie = sched.processes[idx].take().unwrap();
            let pid = zombie.pid;
            let code = zombie.exit_code;
            {
                let mut alloc = crate::ALLOCATOR.lock();
                zombie.address_space.destroy(&mut alloc);
                if let Some((kphys, order)) = zombie.kernel_stack_alloc {
                    alloc.free(kphys, order);
                }
            }
            if !status_ptr.is_null() {
                // Unix の仕様: 正常終了コードは上位 8bit に格納
                let status_val: i32 = (code & 0xff) << 8;
                let sched_ref = unsafe { &*&raw const crate::SCHEDULER };
                write_to_user(sched_ref, status_ptr as u64, &status_val.to_ne_bytes());
            }
            return pid as u64;
        }

        let has_child = sched.processes.iter().flatten().any(|p| {
            p.parent_pid == caller_pid && (target_pid <= 0 || (p.pid as i64) == target_pid)
        });

        if !has_child {
            return (-10i64) as u64; // ECHILD
        }

        // WNOHANG (1) が指定されている場合、ブロックせずに 0 を返す
        if options & 1 != 0 {
            return 0;
        }

        x86_64::instructions::interrupts::enable();
        crate::process::schedule(&raw mut crate::SCHEDULER);
        x86_64::instructions::interrupts::disable();
    }
}

fn syscall_exit(frame: &mut SyscallFrame) -> u64 {
    let code = frame.rdi as i32;
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    if let Some(p) = sched.current_mut() {
        p.exit_code = code;
        p.state = ProcessState::Dead;
    }
    x86_64::instructions::interrupts::enable();
    crate::process::schedule(&raw mut crate::SCHEDULER);

    loop {
        x86_64::instructions::hlt();
    }
}

fn syscall_mmap(frame: &mut SyscallFrame) -> u64 {
    let addr = frame.rdi;
    let len = frame.rsi as usize;
    let prot = frame.rdx as u32;
    // r10 = flags (MAP_FIXED = 0x10)
    let flags = frame.r10 as u32;

    let vma_flags = crate::vma::VMAFlags {
        read: prot & 0x1 != 0,
        write: prot & 0x2 != 0,
        exec: prot & 0x4 != 0,
    };

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    const MAP_FIXED: u32 = 0x10;

    let result = if addr != 0 && (flags & MAP_FIXED != 0) {
        // MAP_FIXED: 指定アドレスに強制マップ
        let mut alloc = crate::ALLOCATOR.lock();
        p.address_space.mmap_fixed(
            addr,
            len,
            vma_flags,
            crate::vma::VMAKind::Anonymous,
            &mut alloc,
        )
    } else {
        // addr=0 または MAP_FIXED なし: 空き領域を探して割り当て
        p.address_space
            .mmap(0, len, vma_flags, crate::vma::VMAKind::Anonymous)
    };

    match result {
        Ok(mapped) => mapped,
        Err(_) => (-12i64) as u64,
    }
}

fn syscall_munmap(frame: &mut SyscallFrame) -> u64 {
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    if let Some(p) = sched.current_mut() {
        let mut alloc = crate::ALLOCATOR.lock();
        let _ = p
            .address_space
            .munmap(frame.rdi, frame.rsi as usize, &mut alloc);
    }
    0
}

fn syscall_brk(frame: &mut SyscallFrame) -> u64 {
    let requested = frame.rdi;
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return 0,
    };

    if requested == 0 || requested <= p.brk_current {
        return p.brk_current;
    }

    let aligned = (requested + 0xFFF) & !0xFFF;
    match p.address_space.extend_heap(p.brk_start, aligned) {
        Ok(()) => {
            p.brk_current = aligned;
            p.brk_current
        }
        Err(_) => p.brk_current,
    }
}

fn syscall_getcwd(frame: &mut SyscallFrame) -> u64 {
    let buf = frame.rdi as *mut u8;
    let size = frame.rsi as usize;

    let sched = unsafe { &*&raw const crate::SCHEDULER };
    let cwd = sched
        .current()
        .map(|p| p.cwd.clone())
        .unwrap_or_else(|| alloc::string::String::from("/"));

    let bytes = cwd.as_bytes();
    if size < bytes.len() + 1 {
        return (-34i64) as u64; // ERANGE
    }

    // NUL 終端を含めてユーザー空間に書き込む
    let mut data = alloc::vec::Vec::with_capacity(bytes.len() + 1);
    data.extend_from_slice(bytes);
    data.push(0);
    if !write_to_user(sched, buf as u64, &data) {
        return (-14i64) as u64; // EFAULT
    }
    buf as u64
}

const TIOCGWINSZ: u64 = 0x5413;
#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

fn syscall_ioctl(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let request = frame.rsi;
    let argp = frame.rdx;

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let kind = entry.lock().kind;

    if request == TIOCGWINSZ {
        if matches!(kind, crate::fd::FdKind::Serial) {
            let ws = Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let sched = unsafe { &*&raw const crate::SCHEDULER };
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    &ws as *const Winsize as *const u8,
                    core::mem::size_of::<Winsize>(),
                )
            };
            if !write_to_user(sched, argp, bytes) {
                return (-14i64) as u64; // EFAULT
            }
            return 0;
        } else {
            return (-25i64) as u64;
        }
    }
    (-25i64) as u64
}

fn syscall_getdents64(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let buf_ptr = frame.rsi as *mut u8;
    let count = frame.rdx as usize;

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let mut desc = entry.lock();
    if !matches!(desc.kind, crate::fd::FdKind::Directory) {
        return (-20i64) as u64;
    }

    let names = match &desc.dir_entries {
        Some(n) => n.clone(),
        None => return 0,
    };

    let start_idx = desc.offset;
    let mut out_off = 0usize;
    let mut consumed = 0usize;

    for (i, name) in names.iter().enumerate().skip(start_idx) {
        let name_bytes = name.as_bytes();
        let raw_reclen = 19 + name_bytes.len() + 1;
        let reclen = (raw_reclen + 7) & !7;

        if out_off + reclen > count {
            break;
        }

        unsafe {
            let p = buf_ptr.add(out_off);
            core::ptr::write_unaligned(p as *mut u64, (i as u64) + 1);
            core::ptr::write_unaligned(p.add(8) as *mut i64, (i as i64) + 1);
            core::ptr::write_unaligned(p.add(16) as *mut u16, reclen as u16);
            *p.add(18) = 8u8;
            core::ptr::copy_nonoverlapping(name_bytes.as_ptr(), p.add(19), name_bytes.len());
            *p.add(19 + name_bytes.len()) = 0;
        }
        out_off += reclen;
        consumed += 1;
    }
    desc.offset = start_idx + consumed;
    out_off as u64
}

#[repr(C)]
struct UtsName {
    sysname: [u8; 65],
    nodename: [u8; 65],
    release: [u8; 65],
    version: [u8; 65],
    machine: [u8; 65],
    domainname: [u8; 65],
}

fn syscall_uname(frame: &mut SyscallFrame) -> u64 {
    let ptr = frame.rdi as *mut UtsName;
    if ptr.is_null() {
        return (-14i64) as u64;
    }

    let mut uts: UtsName = unsafe { core::mem::zeroed() };
    let copy = |dst: &mut [u8; 65], s: &[u8]| {
        let n = s.len().min(64);
        dst[..n].copy_from_slice(&s[..n]);
    };

    copy(&mut uts.sysname, b"Linux");
    copy(&mut uts.nodename, b"ferrum");
    copy(&mut uts.release, b"6.1.0");
    copy(&mut uts.version, b"#1 Ferrum OS");
    copy(&mut uts.machine, b"x86_64");

    let sched = unsafe { &*&raw const crate::SCHEDULER };
    let bytes = unsafe {
        core::slice::from_raw_parts(
            &uts as *const UtsName as *const u8,
            core::mem::size_of::<UtsName>(),
        )
    };
    if !write_to_user(sched, ptr as u64, bytes) {
        return (-14i64) as u64; // EFAULT
    }
    0
}

fn syscall_chdir(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    let new_cwd = resolve_path(&p.cwd.clone(), &raw_path);

    // パスの存在確認
    let rootfs = crate::ROOTFS.lock();
    let exists =
        new_cwd == "/" || rootfs.open(&new_cwd).is_some() || !rootfs.list_dir(&new_cwd).is_empty();
    drop(rootfs);

    if !exists {
        return (-2i64) as u64; // ENOENT
    }

    p.cwd = new_cwd;
    0
}

fn syscall_mkdir(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let cwd = unsafe {
        (&raw const crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);

    let mut rootfs = crate::ROOTFS.lock();

    // 既に存在する場合は EEXIST
    if path == "/" || !rootfs.list_dir(&path).is_empty() || rootfs.open(&path).is_some() {
        return (-17i64) as u64; // EEXIST
    }

    match rootfs.mkdir(&path) {
        Ok(_) => 0,
        Err(_) => (-1i64) as u64,
    }
}

fn syscall_rename(frame: &mut SyscallFrame) -> u64 {
    let old = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let new = match read_user_cstr(frame.rsi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let sched = unsafe { &*&raw const crate::SCHEDULER };
    let cwd = sched
        .current()
        .map(|p| p.cwd.clone())
        .unwrap_or_else(|| alloc::string::String::from("/"));
    let old_path = resolve_path(&cwd, &old);
    let new_path = resolve_path(&cwd, &new);
    match crate::ROOTFS.lock().rename(&old_path, &new_path) {
        Ok(_) => 0,
        Err(_) => (-2i64) as u64,
    }
}

fn syscall_arch_prctl(frame: &mut SyscallFrame) -> u64 {
    let code = frame.rdi;
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };

    match code {
        nr::ARCH_SET_FS => {
            let val = frame.rsi;
            p.fs_base = val;
            unsafe {
                write_fs_base(val);
            }
            0
        }
        nr::ARCH_GET_FS => {
            let sched_ref = unsafe { &*&raw const crate::SCHEDULER };
            write_to_user(sched_ref, frame.rsi, &p.fs_base.to_ne_bytes());
            0
        }
        _ => (-22i64) as u64,
    }
}

pub(crate) unsafe fn write_fs_base(val: u64) {
    unsafe {
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0xC000_0100u32,
            in("eax") val as u32,
            in("edx") (val >> 32) as u32,
        );
    }
}

#[repr(C)]
struct IoVec {
    base: u64,
    len: u64,
}

fn syscall_writev(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let iov_ptr = frame.rsi as *const IoVec;
    let iovcnt = frame.rdx as usize;

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let (kind, writable) = {
        let d = entry.lock();
        (d.kind, d.writable)
    };
    if !writable {
        return (-9i64) as u64;
    }

    let mut total = 0usize;
    for i in 0..iovcnt {
        let iov = unsafe { &*iov_ptr.add(i) };
        if iov.len == 0 {
            continue;
        }
        let buf = unsafe { core::slice::from_raw_parts(iov.base as *const u8, iov.len as usize) };

        match kind {
            crate::fd::FdKind::Serial => {
                for &b in buf {
                    crate::serial_print!("{}", b as char);
                }
                total += buf.len();
            }
            crate::fd::FdKind::File => {
                let mut desc = entry.lock();
                let offset = if desc.append {
                    desc.node.as_ref().map(|n| n.stat().size).unwrap_or(0)
                } else {
                    desc.offset
                };
                let r = {
                    let node = desc.node.as_mut().expect("File fd must have node");
                    node.write(offset, buf)
                };
                match r {
                    Ok(n) => {
                        if !desc.append {
                            desc.offset += n;
                        }
                        total += n;
                    }
                    Err(_) => return (-5i64) as u64,
                }
            }
            crate::fd::FdKind::Directory => return (-21i64) as u64,
            crate::fd::FdKind::DevNull | crate::fd::FdKind::DevZero => total += buf.len(),
        }
    }
    total as u64
}

fn syscall_access(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let cwd = unsafe {
        (&raw mut crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);
    let rootfs = crate::ROOTFS.lock();
    if rootfs.open(&path).is_some() {
        0
    } else {
        (-2i64) as u64
    }
}

fn syscall_pread64(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let buf = frame.rsi as *mut u8;
    let count = frame.rdx as usize;
    let offset = frame.r10 as usize; // pread では rcx ではなく r10

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let mut desc = entry.lock();
    let node = match desc.node.as_ref() {
        Some(n) => n,
        None => return (-9i64) as u64,
    };
    let user_buf = unsafe { core::slice::from_raw_parts_mut(buf, count) };
    match node.read(offset, user_buf) {
        Ok(n) => n as u64,
        Err(_) => (-5i64) as u64,
    }
}

fn syscall_unlink(frame: &mut SyscallFrame) -> u64 {
    let raw_path = match read_user_cstr(frame.rdi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let cwd = unsafe {
        (&raw const crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);
    match crate::ROOTFS.lock().unlink(&path) {
        Ok(_) => 0,
        Err(_) => (-2i64) as u64,
    }
}

fn syscall_unlinkat(frame: &mut SyscallFrame) -> u64 {
    // dirfd(rdi) は AT_FDCWD として扱う
    let mut frame_copy = *frame;
    frame_copy.rdi = frame.rsi; // path
    syscall_unlink(&mut frame_copy)
}

fn syscall_dup(frame: &mut SyscallFrame) -> u64 {
    let oldfd = frame.rdi as usize;
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(oldfd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };
    match p.fd_table.alloc_entry(entry) {
        Some(fd) => fd as u64,
        None => (-24i64) as u64,
    }
}

fn syscall_dup2(frame: &mut SyscallFrame) -> u64 {
    let oldfd = frame.rdi as usize;
    let newfd = frame.rsi as usize;

    if newfd >= crate::fd::FD_MAX {
        return (-9i64) as u64;
    }
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(oldfd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };
    if oldfd == newfd {
        return newfd as u64;
    }
    p.fd_table.close(newfd);
    p.fd_table.put(newfd, entry);
    newfd as u64
}

fn syscall_lseek(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let offset = frame.rsi as i64;
    let whence = frame.rdx as i32;

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };
    let mut desc = entry.lock();

    let size = desc.node.as_ref().map(|n| n.stat().size).unwrap_or(0) as i64;
    let new_offset = match whence {
        0 => offset,                      // SEEK_SET
        1 => desc.offset as i64 + offset, // SEEK_CUR
        2 => size + offset,               // SEEK_END
        _ => return (-22i64) as u64,
    };
    if new_offset < 0 {
        return (-22i64) as u64;
    }
    desc.offset = new_offset as usize;
    new_offset as u64
}

fn syscall_fstat(frame: &mut SyscallFrame) -> u64 {
    let fd = frame.rdi as usize;
    let statbuf_ptr = frame.rsi as *mut LinuxStat;

    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let p = match sched.current_mut() {
        Some(p) => p,
        None => return (-1i64) as u64,
    };
    let entry = match p.fd_table.get(fd) {
        Some(e) => e,
        None => return (-9i64) as u64,
    };

    let desc = entry.lock();
    let (file_type, size) = match desc.kind {
        crate::fd::FdKind::Serial => (crate::fs::vfs::FileType::Regular, 0),
        crate::fd::FdKind::Directory => (crate::fs::vfs::FileType::Directory, 0),
        crate::fd::FdKind::File => {
            let s = desc
                .node
                .as_ref()
                .map(|n| n.stat())
                .unwrap_or(crate::fs::vfs::Stat {
                    file_type: crate::fs::vfs::FileType::Regular,
                    size: 0,
                });
            (s.file_type, s.size)
        }
        _ => (crate::fs::vfs::FileType::Regular, 0),
    };

    let st = fill_stat(file_type, size);
    unsafe { core::ptr::write_unaligned(statbuf_ptr, st) };
    0
}

fn syscall_newfstatat(frame: &mut SyscallFrame) -> u64 {
    // dirfd(rdi), path(rsi), statbuf(rdx), flags(r10)
    let raw_path = match read_user_cstr(frame.rsi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let statbuf_ptr = frame.rdx as *mut LinuxStat;

    let cwd = unsafe {
        (&raw mut crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);

    // ディレクトリチェック
    if path == "/" || !crate::ROOTFS.lock().list_dir(&path).is_empty() {
        let st = fill_stat(crate::fs::vfs::FileType::Directory, 0);
        unsafe { core::ptr::write_unaligned(statbuf_ptr, st) };
        return 0;
    }

    let node = {
        let rootfs = crate::ROOTFS.lock();
        rootfs.open(&path)
    };
    match node {
        Some(n) => {
            let s = n.stat();
            let st = fill_stat(s.file_type, s.size);
            unsafe { core::ptr::write_unaligned(statbuf_ptr, st) };
            0
        }
        None => (-2i64) as u64,
    }
}

fn syscall_openat(frame: &mut SyscallFrame) -> u64 {
    // dirfd(rdi): AT_FDCWD(-100) のときは cwd を使う。
    // それ以外の dirfd は今は未サポートで AT_FDCWD と同様に扱う。
    // rsi=path, rdx=flags, r10=mode
    let mut frame_copy = *frame;
    frame_copy.rdi = frame.rsi; // path
    frame_copy.rsi = frame.rdx; // flags
    syscall_open(&mut frame_copy)
}

fn syscall_faccessat(frame: &mut SyscallFrame) -> u64 {
    // dirfd(rdi) は AT_FDCWD として扱う、path(rsi)
    let raw_path = match read_user_cstr(frame.rsi as *const u8, 256) {
        Some(p) => p,
        None => return (-14i64) as u64,
    };
    let cwd = unsafe {
        (&raw const crate::SCHEDULER)
            .as_ref()
            .unwrap()
            .current()
            .map(|p| p.cwd.clone())
            .unwrap_or_else(|| alloc::string::String::from("/"))
    };
    let path = resolve_path(&cwd, &raw_path);
    let rootfs = crate::ROOTFS.lock();
    if path == "/" || rootfs.open(&path).is_some() || !rootfs.list_dir(&path).is_empty() {
        0
    } else {
        (-2i64) as u64
    }
}

fn syscall_getrandom(frame: &mut SyscallFrame) -> u64 {
    let buf = frame.rdi as *mut u8;
    let buflen = frame.rsi as usize;
    if buf.is_null() || buflen == 0 {
        return 0;
    }
    let tsc: u64;
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
        tsc = ((hi as u64) << 32) | (lo as u64);
    }
    let seed = tsc.to_le_bytes();
    let mut out_buf: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(buflen);
    for i in 0..buflen {
        out_buf.push(seed[i % 8].wrapping_add(i as u8).wrapping_mul(0x6d));
    }
    let sched = unsafe { &*&raw const crate::SCHEDULER };
    if !write_to_user(sched, buf as u64, &out_buf) {
        return (-14i64) as u64; // EFAULT
    }
    buflen as u64
}

fn syscall_clock_gettime(frame: &mut SyscallFrame) -> u64 {
    let _clkid = frame.rdi;
    let tp = frame.rsi as *mut i64;
    if tp.is_null() {
        return (-14i64) as u64;
    }

    let ticks = crate::interrupts::timer_ticks(); // 100Hz カウンタ
    let ms = ticks * 10; // tick → ミリ秒
    let data: [i64; 2] = [(ms / 1_000) as i64, ((ms % 1_000) * 1_000_000) as i64];
    let bytes = unsafe { core::slice::from_raw_parts(data.as_ptr() as *const u8, 16) };
    let sched = unsafe { &*&raw const crate::SCHEDULER };
    if !write_to_user(sched, tp as u64, bytes) {
        return (-14i64) as u64;
    }
    0
}

fn syscall_futex(frame: &mut SyscallFrame) -> u64 {
    let op = (frame.rsi as u32) & 0x7f;
    match op {
        0 => {
            // FUTEX_WAIT: 一度スケジュールして戻る（ビジーウェイト軽減）
            x86_64::instructions::interrupts::enable();
            crate::process::schedule(&raw mut crate::SCHEDULER);
            x86_64::instructions::interrupts::disable();
            (-11i64) as u64 // EAGAIN
        }
        1 => 0, // FUTEX_WAKE
        _ => (-38i64) as u64,
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// 現在プロセスのページテーブル経由でユーザー空間にバイト列を書き込む。
/// ページをまたぐ場合は2分割して書き込む。
fn write_to_user(sched: &crate::process::Scheduler, vaddr: u64, data: &[u8]) -> bool {
    let p = match sched.current() {
        Some(p) => p,
        None => return false,
    };
    let pt = &p.address_space.page_table;
    write_to_user_via_pt(pt, vaddr, data)
}

fn write_to_user_via_pt(pt: &crate::paging::PageTableManager, vaddr: u64, data: &[u8]) -> bool {
    if data.is_empty() {
        return true;
    }
    let page = vaddr & !0xFFF;
    let off = (vaddr & 0xFFF) as usize;
    let phys = match pt.translate(page) {
        Some(p) => p,
        None => return false,
    };
    let hh = crate::paging::phys_to_virt(phys.as_u64());
    let avail = (0x1000 - off).min(data.len());
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), (hh + off as u64) as *mut u8, avail);
    }
    if avail < data.len() {
        write_to_user_via_pt(pt, vaddr + avail as u64, &data[avail..])
    } else {
        true
    }
}

fn resolve_path(cwd: &str, raw_path: &str) -> alloc::string::String {
    let mut stack: alloc::vec::Vec<&str> = alloc::vec::Vec::new();

    if !raw_path.starts_with('/') {
        for comp in cwd.split('/') {
            if !comp.is_empty() {
                stack.push(comp);
            }
        }
    }

    for comp in raw_path.split('/') {
        match comp {
            "" | "." => {
                // 連続するスラッシュ（//）やカレントディレクトリ（.）は無視
            }
            ".." => {
                // 親ディレクトリへ（ルートにいる場合は pop しても何もしないのがUNIXの仕様）
                stack.pop();
            }
            _ => {
                // 通常のディレクトリ/ファイル名
                stack.push(comp);
            }
        }
    }

    let mut resolved = alloc::string::String::from("/");
    resolved.push_str(&stack.join("/"));
    resolved
}

fn read_user_cstr(ptr: *const u8, max: usize) -> Option<alloc::string::String> {
    if ptr.is_null() {
        return None;
    }
    let mut s = alloc::string::String::new();
    for i in 0..max {
        let b = unsafe { *ptr.add(i) };
        if b == 0 {
            return Some(s);
        }
        s.push(b as char);
    }
    None
}

fn read_user_cstr_array(
    ptr: *const u64,
    max_items: usize,
) -> alloc::vec::Vec<alloc::string::String> {
    let mut vec = alloc::vec::Vec::new();
    if ptr.is_null() {
        return vec;
    }
    for i in 0..max_items {
        let entry_ptr = unsafe { *ptr.add(i) };
        if entry_ptr == 0 {
            break;
        }
        if let Some(s) = read_user_cstr(entry_ptr as *const u8, 256) {
            vec.push(s);
        } else {
            break;
        }
    }
    vec
}

// AT_* 定数
const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_SECURE: u64 = 23;
const AT_RANDOM: u64 = 25;
const AT_EXECFN: u64 = 31;
const AT_SYSINFO_EHDR: u64 = 33;

pub(crate) fn build_argv_envp_stack(
    ustack_top: u64,
    pt: &crate::paging::PageTableManager, // ← 追加
    argv: &[alloc::string::String],
    envp: &[alloc::string::String],
    elf_entry: u64,
    elf_phdr_vaddr: u64,
    elf_phent: u64,
    elf_phnum: u64,
) -> u64 {
    // ユーザー仮想アドレスへの書き込みを物理アドレス経由で行うヘルパー
    let write_u64 = |vaddr: u64, val: u64| {
        let phys = pt.translate(vaddr & !0xFFF).expect("stack not mapped");

        let hh = crate::paging::phys_to_virt(phys.as_u64());
        unsafe {
            *((hh + (vaddr & 0xFFF)) as *mut u64) = val;
        }
    };

    // バイト列の書き込み（ページをまたぐ場合は再帰）
    fn write_bytes_via_pt(pt: &crate::paging::PageTableManager, vaddr: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let page = vaddr & !0xFFF;
        let off = (vaddr & 0xFFF) as usize;
        let phys = pt.translate(page).expect("stack not mapped");
        let hh = crate::paging::phys_to_virt(phys.as_u64());
        let avail = (0x1000 - off).min(data.len());
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), (hh + off as u64) as *mut u8, avail);
        }
        if avail < data.len() {
            write_bytes_via_pt(pt, vaddr + avail as u64, &data[avail..]);
        }
    }

    let mut addr = ustack_top;
    let mut argv_ptrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    let mut envp_ptrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();

    for s in envp {
        let bytes = s.as_bytes();
        addr -= (bytes.len() + 1) as u64;
        write_bytes_via_pt(pt, addr, bytes);
        write_bytes_via_pt(pt, addr + bytes.len() as u64, &[0u8]);
        envp_ptrs.push(addr);
    }

    for s in argv {
        let bytes = s.as_bytes();
        addr -= (bytes.len() + 1) as u64;
        write_bytes_via_pt(pt, addr, bytes);
        write_bytes_via_pt(pt, addr + bytes.len() as u64, &[0u8]);
        argv_ptrs.push(addr);
    }

    // AT_RANDOM: TSC 2サンプル + カウンタでエントロピーを改善
    let at_random_bytes: [u8; 16] = {
        let mut b = [0u8; 16];
        let tsc1: u64;
        let tsc2: u64;
        unsafe {
            let lo1: u32;
            let hi1: u32;
            let lo2: u32;
            let hi2: u32;
            core::arch::asm!(
                "rdtsc",
                "mov {lo1:e}, eax",
                "mov {hi1:e}, edx",
                "lfence",
                "rdtsc",
                lo1 = out(reg) lo1, hi1 = out(reg) hi1,
                out("eax") lo2, out("edx") hi2,
            );
            tsc1 = ((hi1 as u64) << 32) | (lo1 as u64);
            tsc2 = ((hi2 as u64) << 32) | (lo2 as u64);
        }
        // xorshift で拡散させてエントロピーを改善
        let mut x = tsc1 ^ (tsc2 << 17) ^ (tsc2 >> 13);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mut y = tsc2 ^ (tsc1 << 31) ^ (tsc1 >> 5);
        y ^= y << 13;
        y ^= y >> 7;
        y ^= y << 17;
        b[0..8].copy_from_slice(&x.to_le_bytes());
        b[8..16].copy_from_slice(&y.to_le_bytes());
        b
    };
    // build_argv_envp_stack 内
    addr -= 16;
    write_bytes_via_pt(pt, addr, &at_random_bytes);
    let at_random_ptr = addr;

    let auxv: &[(u64, u64)] = &[
        (AT_PHDR, elf_phdr_vaddr),
        (AT_PHENT, elf_phent),
        (AT_PHNUM, elf_phnum),
        (AT_PAGESZ, 4096),
        (AT_BASE, 0),
        (AT_FLAGS, 0),
        (AT_ENTRY, elf_entry),
        (AT_UID, 0),
        (AT_EUID, 0),
        (AT_GID, 0),
        (AT_EGID, 0),
        (AT_SECURE, 0),
        (AT_RANDOM, at_random_ptr),
        (AT_SYSINFO_EHDR, 0),
        (AT_NULL, 0),
    ];

    addr &= !0xF;
    let total_words = 1 + argv.len() + 1 + envp.len() + 1 + auxv.len() * 2;
    if total_words % 2 != 0 {
        addr -= 8;
    }

    for &(tag, val) in auxv.iter().rev() {
        addr -= 8;
        write_u64(addr, val);
        addr -= 8;
        write_u64(addr, tag);
    }

    addr -= 8;
    write_u64(addr, 0);
    for &ptr in envp_ptrs.iter().rev() {
        addr -= 8;
        write_u64(addr, ptr);
    }

    addr -= 8;
    write_u64(addr, 0);
    for &ptr in argv_ptrs.iter().rev() {
        addr -= 8;
        write_u64(addr, ptr);
    }

    addr -= 8;
    write_u64(addr, argv.len() as u64);

    addr
}
