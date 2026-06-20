#![allow(dead_code)]

use crate::allocator::PhysAddr;
use crate::fd::FdTable;
use crate::paging::{PageAllocator, PageFlags, PageTableManager};
use crate::vma::AddressSpace;
use alloc::vec::Vec;
use core::arch::naked_asm;
use core::sync::atomic::Ordering;

pub type Pid = u64;

// do_switch が保存・復元する callee-saved レジスタと RSP。
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Context {
    pub rbx: u64, // +0x00
    pub rbp: u64, // +0x08
    pub r12: u64, // +0x10
    pub r13: u64, // +0x18
    pub r14: u64, // +0x20
    pub r15: u64, // +0x28
    pub rsp: u64, // +0x30
}

impl Context {
    pub const fn zero() -> Self {
        Self {
            rbx: 0,
            rbp: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rsp: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Ready,
    Running,
    Blocked,
    Dead,
}

pub struct Process {
    pub pid: Pid,
    pub parent_pid: Pid,
    pub state: ProcessState,
    pub exit_code: i32,
    pub context: Context,
    pub kernel_stack_alloc: Option<(PhysAddr, usize)>,
    pub kernel_stack_top: u64,
    pub address_space: AddressSpace,
    pub fd_table: FdTable,
    pub fs_base: u64,
    pub brk_start: u64,
    pub brk_current: u64,
    pub cwd: alloc::string::String,
}

impl Process {
    pub fn new_kernel(
        pid: Pid,
        entry_point: u64,
        kernel_pml4: PhysAddr,
        stack: *mut u8,
        stack_len: usize,
    ) -> Self {
        let stack_top = (stack as u64) + (stack_len as u64);
        let virt_entry = crate::paging::ensure_virt(entry_point);

        unsafe {
            core::ptr::write_bytes(stack, 0, stack_len);
            *((stack_top - 8) as *mut u64) = virt_entry;
        }

        let ctx = Context {
            rsp: stack_top - 8,
            ..Context::zero()
        };

        crate::serial_println!(
            "[process] new_kernel pid={} entry={:#x} kstack=[{:#x},{:#x})",
            pid,
            virt_entry,
            stack as u64,
            stack_top
        );

        Self {
            pid,
            parent_pid: 0,
            state: ProcessState::Ready,
            exit_code: 0,
            context: ctx,
            kernel_stack_alloc: None,
            kernel_stack_top: stack_top,
            address_space: AddressSpace::new(PageTableManager::from_phys(kernel_pml4)),
            fd_table: FdTable::new_stdio(),
            fs_base: 0,
            brk_start: 0,
            brk_current: 0,
            cwd: alloc::string::String::from("/"),
        }
    }

    pub fn new_user(
        pid: Pid,
        entry_point: u64,
        kernel_pml4: PhysAddr,
        kstack: *mut u8,
        kstack_len: usize,
        ustack_virt: u64,
        ustack_pages: usize,
    ) -> Option<Self> {
        let mut pt = {
            let mut alloc = crate::ALLOCATOR.lock();
            PageTableManager::new_user(kernel_pml4, &mut *alloc)?
        };

        {
            let flags =
                PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER | PageFlags::NO_EXEC;
            let ustack_base = ustack_virt.checked_sub((ustack_pages as u64) * 0x1000)?;
            let mut alloc = crate::ALLOCATOR.lock();
            for i in 0..ustack_pages {
                let page_virt = ustack_base + (i as u64) * 0x1000;
                let (_, phys) = alloc.alloc_page()?;
                pt.map(page_virt, phys, flags, &mut *alloc)?;
            }
        }

        let kstack_top = (kstack as u64) + (kstack_len as u64);
        unsafe {
            core::ptr::write_bytes(kstack, 0, kstack_len);
            let (user_cs, user_ss) = crate::gdt::ring3_selectors();
            let frame = kstack_top as *mut u64;
            *frame.sub(1) = user_ss as u64;
            *frame.sub(2) = ustack_virt;
            *frame.sub(3) = 0x202; // RFLAGS: IF=1
            *frame.sub(4) = user_cs as u64;
            *frame.sub(5) = entry_point;
            *frame.sub(6) = iretq_trampoline as *const () as u64;
        }

        let ctx = Context {
            rsp: kstack_top - 6 * 8,
            ..Context::zero()
        };

        crate::serial_println!(
            "[process] new_user pid={} entry={:#x} kstack=[{:#x},{:#x}) ustack_top={:#x} pml4={:#x}",
            pid,
            entry_point,
            kstack as u64,
            kstack_top,
            ustack_virt,
            pt.pml4_phys().as_u64()
        );

        Some(Self {
            pid,
            parent_pid: 0,
            state: ProcessState::Ready,
            exit_code: 0,
            context: ctx,
            kernel_stack_alloc: None,
            kernel_stack_top: kstack_top,
            address_space: AddressSpace::new(pt),
            fd_table: FdTable::new_stdio(),
            fs_base: 0,
            brk_start: 0,
            brk_current: 0,
            cwd: alloc::string::String::from("/"),
        })
    }

    // fork / exec 用に使う既存 AddressSpace をそのまま受け取る。
    // ユーザースタックはここで割り当てる。
    pub fn new_user_with_address_space(
        pid: Pid,
        entry_point: u64,
        kstack: *mut u8,
        kstack_len: usize,
        kernel_stack_alloc: Option<(PhysAddr, usize)>,
        ustack_virt: u64,
        ustack_pages: usize,
        argv: &[alloc::string::String],
        envp: &[alloc::string::String],
        mut address_space: AddressSpace,
    ) -> Option<Self> {
        {
            let flags =
                PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER | PageFlags::NO_EXEC;
            let ustack_base = ustack_virt.checked_sub((ustack_pages as u64) * 0x1000)?;
            let mut alloc = crate::ALLOCATOR.lock();
            for i in 0..ustack_pages {
                let page_virt = ustack_base + (i as u64) * 0x1000;
                let (_, phys) = alloc.alloc_page()?;
                address_space
                    .page_table
                    .map(page_virt, phys, flags, &mut *alloc)?;
            }
            address_space
                .add_vma(crate::vma::VMA::new(
                    ustack_base,
                    ustack_virt,
                    crate::vma::VMAFlags::rw(),
                    crate::vma::VMAKind::Stack,
                ))
                .ok()?;
        }

        let initial_rsp = crate::syscall::build_argv_envp_stack(ustack_virt, argv, envp);

        let kstack_top = (kstack as u64) + (kstack_len as u64);
        unsafe {
            core::ptr::write_bytes(kstack, 0, kstack_len);
            let (user_cs, user_ss) = crate::gdt::ring3_selectors();
            let frame = kstack_top as *mut u64;
            *frame.sub(1) = user_ss as u64;
            *frame.sub(2) = initial_rsp;
            *frame.sub(3) = 0x202;
            *frame.sub(4) = user_cs as u64;
            *frame.sub(5) = entry_point;
            *frame.sub(6) = iretq_trampoline as *const () as u64;
        }

        let ctx = Context {
            rsp: kstack_top - 6 * 8,
            ..Context::zero()
        };

        crate::serial_println!(
            "[process] new_user_with_address_space pid={} entry={:#x} \
             kstack=[{:#x},{:#x}) ustack_top={:#x} pml4={:#x}",
            pid,
            entry_point,
            kstack as u64,
            kstack_top,
            ustack_virt,
            address_space.page_table.pml4_phys().as_u64()
        );

        Some(Self {
            pid,
            parent_pid: 0,
            state: ProcessState::Ready,
            exit_code: 0,
            context: ctx,
            kernel_stack_alloc,
            kernel_stack_top: kstack_top,
            address_space,
            fd_table: FdTable::new_stdio(),
            fs_base: 0,
            brk_start: 0,
            brk_current: 0,
            cwd: alloc::string::String::from("/"),
        })
    }

    pub fn pml4_phys(&self) -> PhysAddr {
        self.address_space.page_table.pml4_phys()
    }

    pub fn new_with_static_stack(
        pid: Pid,
        entry_point: u64,
        kernel_pml4: PhysAddr,
        stack: *mut u8,
        stack_len: usize,
    ) -> Self {
        Self::new_kernel(pid, entry_point, kernel_pml4, stack, stack_len)
    }

    pub fn fork(&mut self, new_pid: Pid, frame: &crate::syscall::SyscallFrame) -> Option<Self> {
        use crate::paging::{phys_to_virt, PageTableManager, KERNEL_IMAGE_SIZE, KERNEL_PHYS_BASE};

        // 子のカーネルスタックを buddy から確保
        let kstack_phys = {
            let mut alloc = crate::ALLOCATOR.lock();
            alloc.alloc(3)?
        };
        let kstack_virt = phys_to_virt(kstack_phys.as_u64()) as *mut u8;
        let kstack_len = 4096 * 8;
        let kstack_top = (kstack_virt as u64) + (kstack_len as u64);
        unsafe {
            core::ptr::write_bytes(kstack_virt, 0, kstack_len);
        }

        // 子の kernel stack に SyscallFrame をコピー（rax=0 のみ上書き）
        let frame_size = core::mem::size_of::<crate::syscall::SyscallFrame>() as u64;
        let frame_base = kstack_top - frame_size; // = kstack_top - 128
        let trampoline_slot = frame_base - 8; // = kstack_top - 136

        unsafe {
            core::ptr::write(
                frame_base as *mut crate::syscall::SyscallFrame,
                crate::syscall::SyscallFrame { rax: 0, ..*frame },
            );
            *(trampoline_slot as *mut u64) = fork_child_trampoline as *const () as u64;
        }

        // 子の Context を設定
        //    do_switch が rbp=frame_base, rsp=trampoline_slot をロードし
        //    ret → fork_child_trampoline に飛ぶ
        let child_ctx = Context {
            rbp: frame_base,
            rsp: trampoline_slot,
            ..Context::zero()
        };

        // 子のアドレス空間を作成（カーネル higher-half 込みの新規 PML4）
        let kernel_pml4 = crate::ADDRESS_SPACE
            .lock()
            .as_ref()
            .unwrap()
            .page_table
            .pml4_phys();

        let mut child_pt = {
            let mut alloc = crate::ALLOCATOR.lock();
            PageTableManager::new_user(kernel_pml4, &mut *alloc)?
        };

        let mut child_as = AddressSpace::new(PageTableManager::from_phys(child_pt.pml4_phys()));

        // 親のユーザーページをコピー
        {
            let mut alloc = crate::ALLOCATOR.lock();
            for vma in self.address_space.vmas.iter().flatten().copied() {
                child_as.add_vma(vma).ok()?;
                let mut addr = vma.start;
                while addr < vma.end {
                    if let Some(src_phys) = self.address_space.page_table.translate(addr) {
                        crate::serial_println!(
                            "[fork] copying virt={:#x} src_phys={:#x}",
                            addr,
                            src_phys.as_u64()
                        );

                        let (_, dst_phys) = alloc.alloc_page().expect("fork: OOM during copy");

                        if src_phys.as_u64() == dst_phys.as_u64() {
                            panic!(
                                "[fork] CRITICAL: dst_phys == src_phys {:#x}!",
                                dst_phys.as_u64()
                            );
                        }

                        let src = phys_to_virt(src_phys.as_u64()) as *const u8;
                        let dst = phys_to_virt(dst_phys.as_u64()) as *mut u8;
                        unsafe {
                            core::ptr::copy_nonoverlapping(src, dst, 4096);
                        }
                        child_pt
                            .map(addr, dst_phys, vma.flags.to_page_flags(), &mut *alloc)
                            .expect("fork: child_pt.map failed");

                        crate::serial_println!(
                            "[fork] mapped virt={:#x} -> dst_phys={:#x}",
                            addr,
                            dst_phys.as_u64()
                        );
                    }
                    addr += 0x1000;
                }
            }
        }

        // カーネルイメージを子の PML4 にもマップ（syscall/do_switch アクセス用）
        unsafe {
            let parent_table = crate::paging::phys_to_virt(self.pml4_phys().as_u64())
                as *const crate::paging::PageTable;
            let child_table = crate::paging::phys_to_virt(child_as.page_table.pml4_phys().as_u64())
                as *mut crate::paging::PageTable;

            // カーネル領域 (Higher Half: 256-511) を直接コピー
            for i in 256..512 {
                (*child_table).entries[i] = (*parent_table).entries[i];
            }
        }
        let img_base = KERNEL_PHYS_BASE.load(Ordering::Relaxed);
        let img_size = KERNEL_IMAGE_SIZE.load(Ordering::Relaxed);
        crate::paging::map_kernel_image(&mut child_as.page_table, img_base, img_size);

        // fd テーブルをクローン（fork では親子で同じ OpenFile を共有）
        let fd_table = self.fd_table.fork_clone();

        crate::ALLOCATOR
            .lock()
            .check_metadata_integrity("after_elf_process_registered");

        crate::serial_println!(
            "[process] fork: parent_pid={} -> child_pid={} child_pml4={:#x}",
            self.pid,
            new_pid,
            child_as.page_table.pml4_phys().as_u64()
        );

        Some(Process {
            pid: new_pid,
            parent_pid: self.pid,
            state: ProcessState::Ready,
            exit_code: 0,
            context: child_ctx,
            kernel_stack_alloc: Some((kstack_phys, 3)),
            kernel_stack_top: kstack_top,
            address_space: child_as,
            fd_table,
            fs_base: self.fs_base,
            brk_start: self.brk_start,
            brk_current: self.brk_current,
            cwd: alloc::string::String::from("/"),
        })
    }
}

// new_user / new_user_with_address_space の初回起動時に
// do_switch の ret からここに飛び、リング3へ切り替える。
#[unsafe(naked)]
pub unsafe extern "C" fn iretq_trampoline() -> ! {
    naked_asm!(
        "mov ax, 0x2B",
        "mov ds, ax",
        "mov es, ax",
        "mov fs, ax",
        "iretq"
    );
}

#[unsafe(naked)]
pub unsafe extern "C" fn fork_child_trampoline() -> ! {
    naked_asm!(
        "pop r15", // [frame+0x00]
        "pop r14", // [frame+0x08]
        "pop r13", // [frame+0x10]
        "pop r12", // [frame+0x18]
        "pop rbp", // [frame+0x20] user rbp
        "pop rbx", // [frame+0x28]
        "pop r9",  // [frame+0x30]
        "pop r8",  // [frame+0x38]
        "pop r10", // [frame+0x40]
        "pop rdx", // [frame+0x48]
        "pop rsi", // [frame+0x50]
        "pop rdi", // [frame+0x58]
        "pop rax", // [frame+0x60] rax=0 → 子の fork() 戻り値として実レジスタにロード
        // rsp = frame+0x68 = [user_rsp]
        "mov rcx, [rsp + 16]", // user_rip  → rcx (frame+0x78)
        "mov r11, [rsp + 8]",  // user_rflags → r11 (frame+0x70)
        "mov rsp, [rsp]",      // user_rsp  → rsp
        "swapgs",
        "sysretq" // ユーザー空間で fork() == 0 が返る (RAX=0)
    );
}

pub struct Scheduler {
    pub processes: Vec<Option<Process>>,
    current_idx: usize,
    initialized: bool,
    /// 最初の schedule() 呼び出し元のコンテキストを保存する。
    /// 全プロセスが Dead になったとき do_switch でここに戻る。
    pub idle_context: Context,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            processes: Vec::new(),
            current_idx: 0,
            initialized: false,
            idle_context: Context::zero(),
        }
    }

    pub fn add_process(&mut self, p: Process) -> Option<()> {
        crate::serial_println!("[sched] add_process: called for pid={}", p.pid);
        for (i, slot) in self.processes.iter_mut().enumerate() {
            let is_free = match slot {
                None => true,
                Some(existing) => existing.state == ProcessState::Dead,
            };
            if is_free {
                if let Some(mut old) = slot.take() {
                    if old.state == ProcessState::Dead {
                        let mut alloc = crate::ALLOCATOR.lock();
                        old.address_space.destroy(&mut alloc);
                        if let Some((kphys, order)) = old.kernel_stack_alloc {
                            alloc.free(kphys, order);
                        }
                    }
                }
                crate::serial_println!("[sched] add: pid={} slot[{}]", p.pid, i);
                *slot = Some(p);
                return Some(());
            }
        }

        let i = self.processes.len();
        crate::serial_println!("[sched] add: pid={} slot[{}] (grown)", p.pid, i);
        self.processes.push(Some(p));
        Some(())
    }

    pub fn next_process(&mut self) -> Option<&mut Process> {
        let len = self.processes.len();
        let start = if self.initialized {
            (self.current_idx + 1) % len
        } else {
            0
        };

        let idx = (0..len).map(|i| (start + i) % len).find(|&i| {
            self.processes[i]
                .as_ref()
                .is_some_and(|p| p.state == ProcessState::Ready)
        });

        match idx {
            Some(idx) => {
                self.current_idx = idx;
                self.initialized = true;
                Some(self.processes[idx].as_mut().unwrap())
            }
            None => None,
        }
    }

    pub fn current_mut(&mut self) -> Option<&mut Process> {
        if !self.initialized {
            return None;
        }
        self.processes[self.current_idx].as_mut()
    }

    pub fn alloc_pid(&self) -> Pid {
        self.processes
            .iter()
            .flatten()
            .map(|p| p.pid)
            .max()
            .unwrap_or(0)
            + 1
    }

    pub fn current_idx(&self) -> usize {
        self.current_idx
    }

    pub fn dump_state(&self) {
        for (i, slot) in self.processes.iter().enumerate() {
            if let Some(p) = slot {
                crate::serial_println!(
                    "[sched]   [{}] pid={} {:?} rsp={:#x}",
                    i,
                    p.pid,
                    p.state,
                    p.context.rsp
                );
            }
        }
    }

    pub fn current(&self) -> Option<&Process> {
        let idx = self.current_idx();
        if idx < self.processes.len() {
            self.processes[idx].as_ref()
        } else {
            None
        }
    }
}

// callee-saved レジスタ（rbx, rbp, r12-r15）と RSP を保存・復元してコンテキストを切り替える。
// 戻り先は [next.rsp] に積まれたアドレス。
#[unsafe(naked)]
pub unsafe extern "C" fn do_switch(prev: *mut Context, next: *const Context) {
    naked_asm!(
        "mov [rdi + 0x00], rbx",
        "mov [rdi + 0x08], rbp",
        "mov [rdi + 0x10], r12",
        "mov [rdi + 0x18], r13",
        "mov [rdi + 0x20], r14",
        "mov [rdi + 0x28], r15",
        "mov [rdi + 0x30], rsp",
        "mov rsp, [rsi + 0x30]",
        "mov r15, [rsi + 0x28]",
        "mov r14, [rsi + 0x20]",
        "mov r13, [rsi + 0x18]",
        "mov r12, [rsi + 0x10]",
        "mov rbp, [rsi + 0x08]",
        "mov rbx, [rsi + 0x00]",
        "ret"
    );
}

pub fn schedule(scheduler: *mut Scheduler) {
    x86_64::instructions::interrupts::disable();

    let sched = unsafe { &mut *scheduler };

    // Ready プロセスが存在するか（Running 中のものは含まない）
    let has_ready = sched
        .processes
        .iter()
        .any(|p| p.as_ref().is_some_and(|p| p.state == ProcessState::Ready));

    // 現在 Running のプロセスを Ready に戻す（タイマー割り込みによるプリエンプション）
    if sched.initialized {
        let idx = sched.current_idx;
        if let Some(Some(p)) = sched.processes.get_mut(idx) {
            if p.state == ProcessState::Running {
                p.state = ProcessState::Ready;
            }
        }
    }

    // 現在のコンテキストポインタと Dead フラグを取得
    let (prev, is_current_dead): (*mut Context, bool) = if sched.initialized {
        let idx = sched.current_idx;
        if let Some(Some(p)) = sched.processes.get_mut(idx) {
            let dead = p.state == ProcessState::Dead;
            (&mut p.context as *mut Context, dead)
        } else {
            (&mut sched.idle_context as *mut Context, false)
        }
    } else {
        (&mut sched.idle_context as *mut Context, false)
    };

    if !has_ready {
        if is_current_dead {
            crate::interrupts::PREEMPT_SCHED_PTR
                .store(core::ptr::null_mut(), core::sync::atomic::Ordering::Relaxed);

            let next = &sched.idle_context as *const Context;
            let next_pml4 = crate::ADDRESS_SPACE
                .lock()
                .as_ref()
                .unwrap()
                .page_table
                .pml4_phys()
                .as_u64();
            unsafe {
                core::arch::asm!(
                    "mov cr3, {cr3}",
                    "call {f}",
                    "sti",
                    cr3          = in(reg) next_pml4,
                    inout("rdi") prev     => _,
                    inout("rsi") next     => _,
                    f            = sym do_switch,
                    lateout("rax") _,
                    lateout("rcx") _,
                    lateout("rdx") _,
                    lateout("r8")  _,
                    lateout("r9")  _,
                    lateout("r10") _,
                    lateout("r11") _,
                );
            }
        } else {
            // Ready なし・Dead でもない（全プロセスが Running 中 or 未登録）
            x86_64::instructions::interrupts::enable();
        }
        return;
    }
    let (next, next_pml4, next_kstack_top) = match sched.next_process() {
        Some(p) => {
            p.state = ProcessState::Running;
            (
                &p.context as *const Context,
                p.address_space.page_table.pml4_phys().as_u64(),
                p.kernel_stack_top,
            )
        }
        None => {
            // has_ready=true なのに next が None になることは通常ない
            x86_64::instructions::interrupts::enable();
            return;
        }
    };

    // 同一プロセスへの切り替えはスキップ
    if (prev as *const Context) == next {
        x86_64::instructions::interrupts::enable();
        return;
    }

    // TSS のカーネルスタックと syscall_entry の RSP を更新
    crate::gdt::set_kernel_stack(next_kstack_top);
    crate::syscall::update_kernel_rsp(next_kstack_top);

    unsafe {
        core::arch::asm!(
            "mov cr3, {cr3}",
            "call {f}",
            "sti",
            cr3          = in(reg) next_pml4,
            inout("rdi") prev     => _,
            inout("rsi") next     => _,
            f            = sym do_switch,
            lateout("rax") _,
            lateout("rcx") _,
            lateout("rdx") _,
            lateout("r8")  _,
            lateout("r9")  _,
            lateout("r10") _,
            lateout("r11") _,
            lateout("r12") _,
            lateout("r13") _,
            lateout("r14") _,
            lateout("r15") _,
        );
    }

    let restored_fs_base = sched.current_mut().map(|p| p.fs_base).unwrap_or(0);
    unsafe {
        crate::syscall::write_fs_base(restored_fs_base);
    }
}
