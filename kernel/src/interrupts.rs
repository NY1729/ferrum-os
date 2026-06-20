#![allow(dead_code)]
use crate::gdt;
use crate::process;
use crate::serial_println;
use crate::spinlock::InitCell;
use core::arch::naked_asm;
use core::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use x86_64::structures::idt::InterruptDescriptorTable;

static TIMER_TICKS: AtomicU64 = AtomicU64::new(0);

pub fn timer_ticks() -> u64 {
    TIMER_TICKS.load(Ordering::Relaxed)
}

pub static PREEMPT_SCHED_PTR: AtomicPtr<crate::process::Scheduler> =
    AtomicPtr::new(core::ptr::null_mut());

static IDT_STORAGE: InitCell<InterruptDescriptorTable> = InitCell::new();
static IDT_INITIALIZED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub static TIMER_INNER_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn init() {
    serial_println!("[idt] init: begin");

    unsafe {
        let idt = IDT_STORAGE.write(InterruptDescriptorTable::new());

        let h = |addr: u64| x86_64::VirtAddr::new_truncate(addr);

        let bp_addr = breakpoint_stub as *const () as u64;
        let df_addr = double_fault_stub as *const () as u64;
        let pf_addr = page_fault_stub as *const () as u64;
        let tm_addr = timer_stub as *const () as u64;

        serial_println!(
            "[idt] handler addrs: breakpoint={:#x} double_fault={:#x} page_fault={:#x} timer={:#x}",
            bp_addr,
            df_addr,
            pf_addr,
            tm_addr
        );

        idt.breakpoint.set_handler_addr(h(bp_addr));

        idt.double_fault
            .set_handler_addr(h(df_addr))
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);

        idt.page_fault
            .set_handler_addr(h(pf_addr))
            .set_stack_index(gdt::PAGE_FAULT_IST_INDEX);

        let gp_addr = gp_fault_stub as *const () as u64;
        serial_println!("[idt] handler addrs: general_protection={:#x}", gp_addr);
        idt.general_protection_fault.set_handler_addr(h(gp_addr));

        idt[0x20].set_handler_addr(h(tm_addr));

        idt.load();
        serial_println!("[idt] IDT loaded");
    }

    IDT_INITIALIZED.store(true, core::sync::atomic::Ordering::Relaxed);
    serial_println!("[idt] init: done");
}

pub fn reload() {
    if IDT_INITIALIZED.load(core::sync::atomic::Ordering::Relaxed) {
        unsafe {
            IDT_STORAGE.assume_init_ref().load();
        }
        serial_println!("[idt] reload: done");
    }
}

pub fn idt_base_and_limit() -> (u64, u16) {
    let mut idtr = x86_64::instructions::tables::DescriptorTablePointer {
        limit: 0,
        base: x86_64::VirtAddr::new(0),
    };
    unsafe {
        core::arch::asm!("sidt [{}]", in(reg) &mut idtr, options(nostack));
    }
    (idtr.base.as_u64(), idtr.limit)
}

pub fn reload_higher_half() {
    serial_println!("[idt] reload_higher_half: begin");
    let mut idtr = x86_64::instructions::tables::DescriptorTablePointer {
        limit: 0,
        base: x86_64::VirtAddr::new(0),
    };
    unsafe {
        core::arch::asm!("sidt [{}]", in(reg) &mut idtr, options(nostack));
    }
    let phys_base = idtr.base.as_u64();
    let virt_base = crate::paging::phys_to_virt(phys_base);
    serial_println!(
        "[idt] reload_higher_half: phys={:#x} -> virt={:#x}",
        phys_base,
        virt_base
    );
    idtr.base = x86_64::VirtAddr::new(virt_base);
    unsafe {
        core::arch::asm!("lidt [{}]", in(reg) &idtr, options(nostack));
    }
    serial_println!("[idt] reload_higher_half: done");
}

#[unsafe(naked)]
extern "C" fn breakpoint_stub() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx",
        "push rsi", "push rdi", "push rbp",
        "push r8",  "push r9",  "push r10", "push r11",
        "push r12", "push r13", "push r14", "push r15",
        "mov rbp, rsp",
        "and rsp, -16",
        "call {f}",
        "mov rsp, rbp",
        "pop r15", "pop r14", "pop r13", "pop r12",
        "pop r11",  "pop r10", "pop r9",  "pop r8",
        "pop rbp",
        "pop rdi",  "pop rsi",
        "pop rdx",  "pop rcx", "pop rbx", "pop rax",
        "iretq",
        f = sym breakpoint_inner,
    );
}

extern "sysv64" fn breakpoint_inner() {
    serial_println!("[exception] BREAKPOINT");
}

#[unsafe(naked)]
extern "C" fn double_fault_stub() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx",
        "push rsi", "push rdi", "push rbp",
        "push r8",  "push r9",  "push r10", "push r11",
        "push r12", "push r13", "push r14", "push r15",
        "mov r15, [rsp + 128]",  // faulting_rip を保存
        "mov dx, 0x3f8",
        "mov al, 0x52", "out dx, al",   // 'R'
        "mov al, 0x49", "out dx, al",   // 'I'
        "mov al, 0x50", "out dx, al",   // 'P'
        "mov al, 0x3D", "out dx, al",   // '='
        "mov rbx, 16",
        "5:",
        "mov rax, rbx",
        "dec rax",
        "shl rax, 2",
        "mov cl, al",
        "mov rax, r15",
        "shr rax, cl",
        "and al, 0xf",
        "cmp al, 10",
        "jb 6f",
        "add al, 0x57",       // 'a'-10 = 0x57 → 10→'a' ... 15→'f'
        "jmp 7f",
        "6:",
        "add al, 0x30",       // '0'
        "7:",
        "out dx, al",
        "dec rbx",
        "jnz 5b",

        "mov al, 0x0D", "out dx, al",   // CR
        "mov al, 0x0A", "out dx, al",   // LF
        "mov rdi, cr2",               // 第1引数: フォルトアドレス(CR2)
        "mov rsi, cr3",               // 第2引数: CR3
        "mov rdx, rsp",                // 第3引数: 現在のRSP(スタックダンプ用)
        "mov rcx, [rsp + 128]",   // 第4引数: 元の(フォルトした)RIP
        "mov r8,  [rsp + 136]",  // 第5引数: 元のCS (offset = 15*8+16 = 136)
        "sub rsp, 8",     // rsp%16 = 0
        "call {f}",
        f = sym double_fault_inner,
    );
}

extern "sysv64" fn double_fault_inner(
    cr2: u64,
    cr3: u64,
    rsp: u64,
    faulting_rip: u64,
    faulting_cs: u64,
) -> ! {
    serial_println!(
        "[exception] DOUBLE FAULT  faulting_rip={:#x}  cs={:#x}",
        faulting_rip,
        faulting_cs
    );
    serial_println!(
        "[exception]   rsp={:#x}  cr2={:#x}  cr3={:#x}",
        rsp,
        cr2,
        cr3
    );

    // rsp が不正だとここで再フォルトして triple fault するため、
    // カーネル仮想アドレス帯に収まっているか簡易チェックしてから読む。
    if rsp >= 0xffff_8000_0000_0000 && rsp < 0xffff_ffff_ffff_f000 {
        serial_println!("[exception] Stack dump (rsp ~ rsp+256):");
        for i in 0..32usize {
            let addr = rsp + (i as u64) * 8;
            let val = unsafe { *(addr as *const u64) };
            serial_println!("  [{:#x}] = {:#x}", addr, val);
        }
    } else {
        serial_println!(
            "[exception] rsp={:#x} looks invalid, skipping stack dump",
            rsp
        );
    }
    panic!(
        "[exception] DOUBLE FAULT faulting_rip={:#x} rsp={:#x} cr2={:#x} cr3={:#x}",
        faulting_rip, rsp, cr2, cr3
    );
}

#[unsafe(naked)]
extern "C" fn page_fault_stub() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx",
        "push rsi", "push rdi", "push rbp",
        "push r8",  "push r9",  "push r10", "push r11",
        "push r12", "push r13", "push r14", "push r15",
        "mov rdi, cr2",
        "sub rsp, 8",
        "call {f}",
        "add rsp, 8",
        "pop r15", "pop r14", "pop r13", "pop r12", // レジスタ復元
        "pop r11",  "pop r10", "pop r9",  "pop r8",
        "pop rbp",
        "pop rdi",  "pop rsi",
        "pop rdx",  "pop rcx", "pop rbx", "pop rax",
        "add rsp, 8",
        "iretq",
        f = sym page_fault_inner,
    );
}

extern "sysv64" fn page_fault_inner(fault_addr: u64) {
    serial_println!("[exception] PAGE FAULT @ {:#x}", fault_addr);

    // 現在実行中のプロセスのアドレス空間で処理を試みる
    let result = {
        let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
        let current_alive = sched
            .current_mut()
            .map(|p| p.state != crate::process::ProcessState::Dead)
            .unwrap_or(false);

        if current_alive {
            // ユーザープロセスの AddressSpace で処理
            let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
            if let Some(p) = sched.current_mut() {
                let mut alloc = crate::ALLOCATOR.lock();
                p.address_space.handle_page_fault(fault_addr, &mut alloc)
            } else {
                Err("no current process")
            }
        } else {
            // スケジューラ未初期化 or カーネルコードの #PF → カーネル AddressSpace で処理
            let mut as_lock = crate::ADDRESS_SPACE.lock();
            let mut alloc = crate::ALLOCATOR.lock();
            match as_lock.as_mut() {
                Some(as_) => as_.handle_page_fault(fault_addr, &mut alloc),
                None => Err("AddressSpace not initialized"),
            }
        }
    };

    match result {
        Ok(()) => {
            serial_println!("[exception] PAGE FAULT @ {:#x}: handled OK", fault_addr);
        }
        Err(e) => {
            // ユーザープロセスの不正アクセス → SIGSEGV 相当
            let sched = unsafe { &mut *&raw mut crate::SCHEDULER };
            if let Some(p) = sched.current_mut() {
                if p.state != crate::process::ProcessState::Dead {
                    serial_println!(
                        "[exception] PAGE FAULT @ {:#x}: SIGSEGV pid={} ({})",
                        fault_addr,
                        p.pid,
                        e
                    );
                    p.state = crate::process::ProcessState::Dead;

                    x86_64::instructions::interrupts::enable();
                    crate::process::schedule(&raw mut crate::SCHEDULER);

                    // schedule が戻ってきた場合（Ready プロセスなし）は hlt で待つ
                    loop {
                        x86_64::instructions::hlt();
                    }
                }
            }
            panic!(
                "[exception] PAGE FAULT @ {:#x}: UNHANDLED: {}",
                fault_addr, e
            );
        }
    }
}

#[unsafe(naked)]
extern "C" fn timer_stub() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx",
        "push rsi", "push rdi",
        "push r8",  "push r9",  "push r10", "push r11",
        "push r12", "push r13", "push r14", "push r15",
        "push rbp",
        "mov rbp, rsp",
        "and rsp, -16",
        "call {inner}",    // EOI 送信
        "mov rsp, rbp",    // スタック復元（15 push 直後の位置に戻す）
        "and rsp, -16",    // call 前に再アライン（rbp からの一発オフセットなので安全）
        "call {sched}",    // プリエンプション
        "mov rsp, rbp",    // スタック復元
        "pop rbp",
        "pop r15", "pop r14", "pop r13", "pop r12",
        "pop r11",  "pop r10", "pop r9",  "pop r8",
        "pop rdi",  "pop rsi",
        "pop rdx",  "pop rcx", "pop rbx", "pop rax",
        "iretq",
        inner = sym timer_inner,
        sched = sym preempt_yield,
    );
}

pub static PIT_MASKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

extern "sysv64" fn timer_inner() {
    TIMER_TICKS.fetch_add(1, Ordering::Relaxed);
    TIMER_INNER_COUNT.fetch_add(1, Ordering::Relaxed);

    if PIT_MASKED.load(Ordering::Relaxed) {
        crate::apic::send_eoi();
    } else {
        crate::pic::send_eoi(0);
    }
}

extern "sysv64" fn preempt_yield() {
    let ptr = PREEMPT_SCHED_PTR.load(Ordering::Relaxed);
    if !ptr.is_null() {
        process::schedule(ptr);
    }
}

#[unsafe(naked)]
extern "C" fn gp_fault_stub() {
    naked_asm!(
        "push rax", "push rbx", "push rcx", "push rdx",
        "push rsi", "push rdi", "push rbp",
        "push r8",  "push r9",  "push r10", "push r11",
        "push r12", "push r13", "push r14", "push r15",
        "mov rdi, [rsp + 15*8]",  // error_code (ハードウェアが push 済み)
        "mov rsi, [rsp + 16*8]",  // 元の RIP
        "mov rbp, rsp",
        "and rsp, -16",
        "call {f}",
        f = sym gp_fault_inner,
    );
}

extern "sysv64" fn gp_fault_inner(error_code: u64, faulting_rip: u64) -> ! {
    serial_println!(
        "[exception] GENERAL PROTECTION FAULT error_code={:#x} rip={:#x}",
        error_code,
        faulting_rip
    );
    panic!(
        "[exception] #GP error_code={:#x} rip={:#x}",
        error_code, faulting_rip
    );
}
