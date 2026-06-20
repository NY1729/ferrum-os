#![allow(dead_code)]
use crate::spinlock::InitCell;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

use crate::serial_println;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub const PAGE_FAULT_IST_INDEX: u16 = 1;

const STACK_SIZE: usize = 4096 * 4;

pub static mut DOUBLE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];
pub static mut PAGE_FAULT_STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

static TSS_STORAGE: InitCell<TaskStateSegment> = InitCell::new();
static GDT_STORAGE: InitCell<GlobalDescriptorTable> = InitCell::new();
static SELECTORS: InitCell<Selectors> = InitCell::new();
struct Selectors {
    pub kernel_cs: SegmentSelector,
    pub kernel_ds: SegmentSelector,
    pub tss_selector: SegmentSelector,
    pub user_cs: SegmentSelector,
    pub user_ds: SegmentSelector,
}

pub fn init() {
    unsafe {
        let tss = TSS_STORAGE.write(TaskStateSegment::new());

        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] =
            VirtAddr::new_truncate(&raw const DOUBLE_FAULT_STACK as u64 + STACK_SIZE as u64);
        tss.interrupt_stack_table[PAGE_FAULT_IST_INDEX as usize] =
            VirtAddr::new_truncate(&raw const PAGE_FAULT_STACK as u64 + STACK_SIZE as u64);

        let gdt = GDT_STORAGE.write(GlobalDescriptorTable::new());

        let kernel_cs = gdt.append(Descriptor::kernel_code_segment()); // 0x08
        let kernel_ds = gdt.append(Descriptor::kernel_data_segment()); // 0x10
        let tss_selector = gdt.append(Descriptor::tss_segment(tss)); // 0x18,0x20
        let user_ds = gdt.append(Descriptor::user_data_segment()); // 0x28 → 0x2B
        let user_cs = gdt.append(Descriptor::user_code_segment()); // 0x30 → 0x33

        SELECTORS.write(Selectors {
            kernel_cs,
            kernel_ds,
            tss_selector,
            user_cs,
            user_ds,
        });

        gdt.load();

        use x86_64::instructions::segmentation::{Segment, CS, DS, SS};
        use x86_64::instructions::tables::load_tss;
        CS::set_reg(kernel_cs);
        DS::set_reg(kernel_ds);
        SS::set_reg(kernel_ds);
        load_tss(tss_selector);

        crate::serial_println!(
            "[gdt] DOUBLE_FAULT_STACK: virt={:#x}",
            &raw const DOUBLE_FAULT_STACK as u64,
        );
    }

    let sel = unsafe { SELECTORS.assume_init_ref() };
    serial_println!(
        "[gdt] init: done  kernel_cs={:?} kernel_ds={:?} user_cs={:?} user_ds={:?}",
        sel.kernel_cs,
        sel.kernel_ds,
        sel.user_cs,
        sel.user_ds
    );
    serial_println!(
        "[gdt] IST stacks: double_fault={:#x} page_fault={:#x}",
        &raw const DOUBLE_FAULT_STACK as u64 + STACK_SIZE as u64,
        &raw const PAGE_FAULT_STACK as u64 + STACK_SIZE as u64,
    );
}

pub fn ring3_selectors() -> (u16, u16) {
    let sel = unsafe { SELECTORS.assume_init_ref() };
    (sel.user_cs.0, sel.user_ds.0)
}

pub fn kernel_selectors() -> (u16, u16) {
    let sel = unsafe { SELECTORS.assume_init_ref() };
    (sel.kernel_cs.0, sel.kernel_ds.0)
}

pub fn set_kernel_stack(rsp0: u64) {
    unsafe {
        let tss = TSS_STORAGE.assume_init_mut();
        tss.privilege_stack_table[0] = VirtAddr::new_truncate(rsp0);
    }
}

pub fn init_syscall(syscall_handler: u64) {
    unsafe {
        use core::arch::asm;

        let efer_lo: u32;
        let efer_hi: u32;
        asm!("rdmsr", in("ecx") 0xC000_0080u32,
            out("eax") efer_lo, out("edx") efer_hi, options(nostack));
        let efer = ((efer_hi as u64) << 32) | (efer_lo as u64) | 1;
        asm!("wrmsr", in("ecx") 0xC000_0080u32,
            in("eax") efer as u32, in("edx") (efer >> 32) as u32, options(nostack));

        // STAR: [47:32]=0x0008, [63:48]=0x0020
        let star: u64 = (0x0008u64 << 32) | (0x0020u64 << 48);
        asm!("wrmsr", in("ecx") 0xC000_0081u32,
            in("eax") star as u32, in("edx") (star >> 32) as u32, options(nostack));

        // LSTAR: syscall エントリポイント
        asm!("wrmsr", in("ecx") 0xC000_0082u32,
            in("eax") syscall_handler as u32,
            in("edx") (syscall_handler >> 32) as u32, options(nostack));

        // FMASK: syscall 時に RFLAGS からクリアするビット (IF=bit9, DF=bit10)
        let fmask: u64 = (1 << 9) | (1 << 10);
        asm!("wrmsr", in("ecx") 0xC000_0084u32,
            in("eax") fmask as u32, in("edx") 0u32, options(nostack));
    }

    serial_println!(
        "[gdt] init_syscall: handler={:#x} STAR=0x0020_0008_0000_0000",
        syscall_handler
    );
}

pub fn gdt_base_and_limit() -> (u32, u16) {
    let mut gdtr = x86_64::instructions::tables::DescriptorTablePointer {
        limit: 0,
        base: x86_64::VirtAddr::new(0),
    };
    unsafe {
        core::arch::asm!("sgdt [{}]", in(reg) &mut gdtr, options(nostack));
    }
    (gdtr.base.as_u64() as u32, gdtr.limit)
}
