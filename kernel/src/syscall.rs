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
        "add rsp, 8",

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
    pub const LSTAT: u64 = 6;
    pub const MMAP: u64 = 9;
    pub const MUNMAP: u64 = 11;
    pub const BRK: u64 = 12;
    pub const RT_SIGACTION: u64 = 13;
    pub const RT_SIGPROCMASK: u64 = 14;
    pub const IOCTL: u64 = 16;
    pub const WRITEV: u64 = 20;
    pub const YIELD: u64 = 24;
    pub const GETPID: u64 = 39;
    pub const CLONE: u64 = 56;
    pub const FORK: u64 = 57;
    pub const EXECVE: u64 = 59;
    pub const EXIT: u64 = 60;
    pub const WAIT4: u64 = 61;
    pub const FCNTL: u64 = 72;
    pub const GETCWD: u64 = 79;
    pub const GETEUID: u64 = 107;
    pub const GETPPID: u64 = 110;
    pub const ARCH_PRCTL: u64 = 158;
    pub const GETDENTS64: u64 = 217;
    pub const SET_TID_ADDRESS: u64 = 218;
    pub const EXIT_GROUP: u64 = 231;

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
        nr::ARCH_PRCTL => syscall_arch_prctl(frame),
        nr::WRITEV => syscall_writev(frame),
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

    match kind {
        crate::fd::FdKind::Serial => {
            for &b in user_buf {
                crate::serial_print!("{}", b as char);
            }
            len as u64
        }
        crate::fd::FdKind::File => {
            let mut desc = entry.lock();
            let offset = desc.offset;
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
    }
}

mod open_flags {
    pub const O_ACCMODE: u64 = 0x3;
    pub const O_WRONLY: u64 = 0x1;
    pub const O_RDWR: u64 = 0x2;
    pub const O_CREAT: u64 = 0o100;
    pub const O_TRUNC: u64 = 0o1000;
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
        let file = crate::fd::OpenFile::file(node, true, false);
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
            if rootfs.write_file(&path, b"").is_err() {
                return (-5i64) as u64;
            }
            node = rootfs.open(&path);
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

    let file = crate::fd::OpenFile::file(node, readable, writable);
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
    let cmd = frame.rsi;
    const F_DUPFD: u64 = 0;
    const F_GETFD: u64 = 1;
    const F_SETFD: u64 = 2;
    const F_GETFL: u64 = 3;
    const F_SETFL: u64 = 4;

    match cmd {
        F_GETFD => 0,
        F_SETFD => 0,
        F_GETFL => 0,
        F_SETFL => 0,
        F_DUPFD => (-38i64) as u64,
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

    crate::serial_println!(
        "[syscall] execve: '{}' argc={} envc={}",
        path,
        argv.len(),
        envp.len()
    );

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
    let ustack_pages = 8usize;
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
                ustack_virt,
                crate::vma::VMAFlags::rw(),
                crate::vma::VMAKind::Stack,
            ))
            .ok();
    }

    let new_rsp = build_argv_envp_stack(ustack_virt, &argv, &envp);

    frame.rcx = entry;
    frame.user_rsp = new_rsp;
    frame.r11 = 0x202;

    0
}

fn syscall_wait4(frame: &mut SyscallFrame) -> u64 {
    let target_pid = frame.rdi as i64;
    let status_ptr = frame.rsi as *mut i32;
    let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
    let caller_pid = match sched.current_mut() {
        Some(p) => p.pid,
        None => return (-1i64) as u64,
    };

    loop {
        let zombie_idx = sched.processes.iter().enumerate().find_map(|(i, slot)| {
            slot.as_ref()
                .filter(|p| {
                    p.parent_pid == caller_pid
                        && p.state == crate::process::ProcessState::Dead
                        && (target_pid == -1 || (p.pid as i64) == target_pid)
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
                unsafe {
                    *status_ptr = code;
                }
            }
            return pid;
        }

        let has_child = sched
            .processes
            .iter()
            .flatten()
            .any(|p| p.parent_pid == caller_pid);
        if !has_child {
            return (-10i64) as u64; // ECHILD
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
    crate::process::schedule(&raw mut crate::SCHEDULER);
    0
}

fn syscall_mmap(frame: &mut SyscallFrame) -> u64 {
    let addr = frame.rdi;
    let len = frame.rsi as usize;
    let prot = frame.rdx as u32;

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

    let result = if addr != 0 {
        let mut alloc = crate::ALLOCATOR.lock();
        p.address_space.mmap_fixed(
            addr,
            len,
            vma_flags,
            crate::vma::VMAKind::Anonymous,
            &mut alloc,
        )
    } else {
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
    let alloc = crate::ALLOCATOR.lock();
    match p.address_space.extend_heap(p.brk_start, aligned) {
        Ok(()) => {
            drop(alloc);
            p.brk_current = aligned;
            p.brk_current
        }
        Err(_) => p.brk_current,
    }
}

fn syscall_getcwd(frame: &mut SyscallFrame) -> u64 {
    let buf = frame.rdi as *mut u8;
    let size = frame.rsi as usize;
    let cwd = b"/\0";

    if size < cwd.len() {
        return (-34i64) as u64;
    } // ERANGE
    unsafe {
        core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf, cwd.len());
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
            unsafe {
                core::ptr::write(argp as *mut Winsize, ws);
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
            unsafe {
                *(frame.rsi as *mut u64) = p.fs_base;
            }
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
                let offset = desc.offset;
                let r = {
                    let node = desc.node.as_mut().expect("File fd must have node");
                    node.write(offset, buf)
                };
                match r {
                    Ok(n) => {
                        desc.offset += n;
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

// ── Helpers ───────────────────────────────────────────────────────

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

pub(crate) fn build_argv_envp_stack(
    ustack_top: u64,
    argv: &[alloc::string::String],
    envp: &[alloc::string::String],
) -> u64 {
    let mut addr = ustack_top;
    let mut argv_ptrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    let mut envp_ptrs: alloc::vec::Vec<u64> = alloc::vec::Vec::new();

    for s in envp {
        let bytes = s.as_bytes();
        addr -= (bytes.len() + 1) as u64;
        unsafe {
            let dst = addr as *mut u8;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            *dst.add(bytes.len()) = 0;
        }
        envp_ptrs.push(addr);
    }

    for s in argv {
        let bytes = s.as_bytes();
        addr -= (bytes.len() + 1) as u64;
        unsafe {
            let dst = addr as *mut u8;
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            *dst.add(bytes.len()) = 0;
        }
        argv_ptrs.push(addr);
    }

    let at_random_bytes: [u8; 16] = {
        let tsc: u64;
        unsafe {
            let lo: u32;
            let hi: u32;
            core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
            tsc = ((hi as u64) << 32) | (lo as u64);
        }
        let mut b = [0u8; 16];
        b[0..8].copy_from_slice(&tsc.to_le_bytes());
        b[8..16].copy_from_slice(&(!tsc).to_le_bytes());
        b
    };
    addr -= 16;
    unsafe {
        core::ptr::copy_nonoverlapping(at_random_bytes.as_ptr(), addr as *mut u8, 16);
    }
    let at_random_ptr = addr;

    addr &= !0xF;

    let total_words = argv.len() + envp.len() + 7;
    if total_words % 2 == 0 {
        addr -= 8;
    }

    addr -= 8;
    unsafe {
        *(addr as *mut u64) = 0;
    }
    addr -= 8;
    unsafe {
        *(addr as *mut u64) = 0;
    }

    addr -= 8;
    unsafe {
        *(addr as *mut u64) = at_random_ptr;
    }
    addr -= 8;
    unsafe {
        *(addr as *mut u64) = 25;
    }

    addr -= 8;
    unsafe {
        *(addr as *mut u64) = 0;
    }
    for &ptr in envp_ptrs.iter().rev() {
        addr -= 8;
        unsafe {
            *(addr as *mut u64) = ptr;
        }
    }

    addr -= 8;
    unsafe {
        *(addr as *mut u64) = 0;
    }
    for &ptr in argv_ptrs.iter().rev() {
        addr -= 8;
        unsafe {
            *(addr as *mut u64) = ptr;
        }
    }

    addr -= 8;
    unsafe {
        *(addr as *mut u64) = argv.len() as u64;
    }

    addr
}
