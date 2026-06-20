#![allow(dead_code)]
use crate::apic;
use crate::interrupts;

static TRAMPOLINE_CODE: &[u8] = include_bytes!(env!("AP_BOOT_BIN"));

const TRAMPOLINE_PHYS: u64 = 0x8000;

include!(env!("AP_OFFSETS_RS"));

const GDT_PTR_OFFSET: usize = 0xf0;
const IDT_PTR_OFFSET: usize = 0xf8;
const PML4_OFFSET: usize = 0x108;
const STACK_OFFSET: usize = 0x110;
const AP_MAIN_OFFSET: usize = 0x118;

pub unsafe fn install(
    gdt_limit: u16,
    gdt_base: u32,
    idt_limit: u16,
    idt_base: u64,
    pml4_phys: u32,
    stack_top: u64,
    ap_main: u64,
) {
    crate::serial_println!(
        "[ap_boot] install: trampoline_phys={:#x} code_len={}",
        TRAMPOLINE_PHYS,
        TRAMPOLINE_CODE.len()
    );
    crate::serial_println!(
        "[ap_boot] install: gdt_limit={} gdt_base={:#x}",
        gdt_limit,
        gdt_base
    );
    crate::serial_println!(
        "[ap_boot] install: idt_limit={} idt_base={:#x}",
        idt_limit,
        idt_base
    );
    crate::serial_println!(
        "[ap_boot] install: pml4_phys={:#x} stack_top={:#x} ap_main={:#x}",
        pml4_phys,
        stack_top,
        ap_main
    );

    let dst = TRAMPOLINE_PHYS as *mut u8;
    let src = TRAMPOLINE_CODE.as_ptr();
    let len = TRAMPOLINE_CODE.len();

    assert!(len > 0, "[ap_boot] trampoline code length is 0");
    assert!(
        (TRAMPOLINE_PHYS as usize) + len <= 0x10000,
        "[ap_boot] trampoline overflows first 64KB: phys={:#x} len={}",
        TRAMPOLINE_PHYS,
        len
    );

    core::ptr::copy_nonoverlapping(src, dst, len);
    crate::serial_println!("[ap_boot] install: trampoline code copied ({} bytes)", len);

    let base = TRAMPOLINE_PHYS as usize;
    core::ptr::write_unaligned((base + AP_GDT_PTR_OFFSET) as *mut u16, gdt_limit);
    core::ptr::write_unaligned((base + AP_GDT_PTR_OFFSET + 2) as *mut u32, gdt_base);
    core::ptr::write_unaligned((base + AP_IDT_PTR_OFFSET) as *mut u16, idt_limit);
    core::ptr::write_unaligned((base + AP_IDT_PTR_OFFSET + 2) as *mut u64, idt_base);
    core::ptr::write_unaligned((base + AP_PML4_ADDR_OFFSET) as *mut u32, pml4_phys);
    core::ptr::write_unaligned((base + AP_STACK_PTR_OFFSET) as *mut u64, stack_top);
    core::ptr::write_unaligned((base + AP_MAIN_PTR_OFFSET) as *mut u64, ap_main);

    crate::serial_println!("[ap_boot] install: data fields written");
    crate::serial_println!(
        "[ap_boot] install: offsets GDT={:#x} IDT={:#x} PML4={:#x} STACK={:#x} MAIN={:#x}",
        AP_GDT_PTR_OFFSET,
        AP_IDT_PTR_OFFSET,
        AP_PML4_ADDR_OFFSET,
        AP_STACK_PTR_OFFSET,
        AP_MAIN_PTR_OFFSET
    );
}

pub fn trampoline_phys() -> u8 {
    let v = (TRAMPOLINE_PHYS >> 12) as u8;
    crate::serial_println!("[ap_boot] trampoline_phys vector={:#x}", v);
    v
}

pub extern "C" fn ap_main() -> ! {
    crate::serial_println!("[ap_boot] ap_main: AP started, loading IDT");
    interrupts::reload();
    crate::serial_println!("[ap_boot] ap_main: IDT reloaded, init APIC");
    apic::init();
    crate::serial_println!("[ap_boot] ap_main: APIC initialized, enabling interrupts");
    x86_64::instructions::interrupts::enable();
    crate::serial_println!("[ap_boot] ap_main: entering hlt loop");
    loop {
        x86_64::instructions::hlt();
    }
}
