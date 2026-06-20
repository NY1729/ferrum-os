#![allow(dead_code)]
use core::arch::asm;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xa0;
const PIC2_DATA: u16 = 0xa1;

const PIC_EOI: u8 = 0x20;
const ICW1_INIT: u8 = 0x11;
const ICW4_8086: u8 = 0x01;

unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val);
}

unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port);
    val
}

unsafe fn io_wait() {
    outb(0x80, 0);
}

pub fn init() {
    crate::serial_println!("[pic] init: begin");
    unsafe {
        outb(PIC1_CMD, ICW1_INIT);
        io_wait();
        outb(PIC2_CMD, ICW1_INIT);
        io_wait();

        // Master: IRQ0-7 -> IDT 0x20-0x27
        outb(PIC1_DATA, 0x20);
        io_wait();
        // Slave: IRQ8-15 -> IDT 0x28-0x2F
        outb(PIC2_DATA, 0x28);
        io_wait();

        outb(PIC1_DATA, 0x04);
        io_wait();
        outb(PIC2_DATA, 0x02);
        io_wait();

        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();

        // IRQ0（タイマー）のみ許可、残りはマスク
        outb(PIC1_DATA, 0b1111_1110);
        outb(PIC2_DATA, 0b1111_1111);

        let m1 = inb(PIC1_DATA);
        let m2 = inb(PIC2_DATA);
        crate::serial_println!("[pic] init: mask PIC1={:#010b} PIC2={:#010b}", m1, m2);
    }
    crate::serial_println!("[pic] init: done");
}

#[inline(always)]
pub fn send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_CMD, PIC_EOI);
        }
        outb(PIC1_CMD, PIC_EOI);
    }
}

pub fn init_pit(frequency: u32) {
    let divisor = 1193182u32 / frequency;
    crate::serial_println!(
        "[pic] init_pit: frequency={} divisor={}",
        frequency,
        divisor
    );
    unsafe {
        outb(0x43, 0x36);
        outb(0x40, (divisor & 0xff) as u8);
        outb(0x40, ((divisor >> 8) & 0xff) as u8);
    }
    crate::serial_println!("[pic] init_pit: done {}Hz", frequency);
}

pub fn mask_irq0() {
    crate::serial_println!("[pic] mask_irq0: masking PIT IRQ0");
    // フラグを先にセットしてからマスク（競合防止）
    crate::interrupts::PIT_MASKED.store(true, core::sync::atomic::Ordering::SeqCst);
    unsafe {
        let mut port = x86_64::instructions::port::Port::<u8>::new(0x21);
        let before = port.read();
        port.write(before | 0x01);
        let after = port.read();
        crate::serial_println!(
            "[pic] mask_irq0: PIC1 mask before={:#010b} after={:#010b}",
            before,
            after
        );
    }
    crate::serial_println!("[pic] mask_irq0: done");
}
