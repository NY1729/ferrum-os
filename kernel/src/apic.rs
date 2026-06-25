#![allow(dead_code)]
const LAPIC_ID: usize = 0x020;
const LAPIC_VER: usize = 0x030;
const LAPIC_TPR: usize = 0x080;
const LAPIC_EOI: usize = 0x0b0;
const LAPIC_SVR: usize = 0x0f0;
const LAPIC_ICR_LOW: usize = 0x300;
const LAPIC_ICR_HIGH: usize = 0x310;
const LAPIC_TIMER: usize = 0x320;
const LAPIC_TIMER_INIT: usize = 0x380;
const LAPIC_TIMER_CUR: usize = 0x390;
const LAPIC_TIMER_DIV: usize = 0x3e0;

const LAPIC_SVR_ENABLE: u32 = 1 << 8;
const LAPIC_TIMER_PERIODIC: u32 = 1 << 17;

static LAPIC_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
pub static COUNTS_PER_10MS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn init() {
    crate::serial_println!("[apic] init: begin");

    let base = unsafe {
        let eax: u32;
        let edx: u32;
        core::arch::asm!(
            "rdmsr",
            in("ecx") 0x1Bu32,
            out("eax") eax,
            out("edx") edx,
        );
        let raw = ((edx as u64) << 32) | (eax as u64);
        let base = raw & 0xffff_ffff_f000;
        crate::serial_println!(
            "[apic] MSR 0x1B: raw={:#x} base={:#x} BSP={}",
            raw,
            base,
            (raw >> 8) & 1
        );
        base
    };

    LAPIC_BASE.store(base, core::sync::atomic::Ordering::Relaxed);

    let svr_before = read(LAPIC_SVR);
    write(LAPIC_SVR, svr_before | LAPIC_SVR_ENABLE | 0xff);
    let svr_after = read(LAPIC_SVR);
    crate::serial_println!(
        "[apic] SVR: before={:#010x} after={:#010x}",
        svr_before,
        svr_after
    );

    write(LAPIC_TPR, 0);
    crate::serial_println!("[apic] TPR set to 0 (accept all interrupts)");

    let apic_id = id();
    let ver = read(LAPIC_VER);
    crate::serial_println!("[apic] init: done ID={} VER={:#010x}", apic_id, ver);
}

pub fn id() -> u32 {
    read(LAPIC_ID) >> 24
}

/// EOI 送信
#[inline(always)]
pub fn send_eoi() {
    write(LAPIC_EOI, 0);
}

pub fn init_timer(vector: u8) {
    crate::serial_println!("[apic] init_timer: vector={:#x}", vector);
    write(LAPIC_TIMER_DIV, 0x3);
    write(LAPIC_TIMER, LAPIC_TIMER_PERIODIC | (vector as u32));
    write(LAPIC_TIMER_INIT, 625_000);
    crate::serial_println!("[apic] init_timer: done (init_count=625000, div=16)");
}

pub fn send_sipi(apic_id: u8, vector: u8) {
    crate::serial_println!("[apic] send_sipi: apic_id={} vector={:#x}", apic_id, vector);

    write(LAPIC_ICR_HIGH, (apic_id as u32) << 24);
    write(LAPIC_ICR_LOW, 0x0000_c500);
    crate::serial_println!("[apic] send_sipi: INIT IPI assert sent");
    busy_wait_ms(10);

    write(LAPIC_ICR_HIGH, (apic_id as u32) << 24);
    write(LAPIC_ICR_LOW, 0x0000_8500);
    crate::serial_println!("[apic] send_sipi: INIT IPI deassert sent");
    busy_wait_ms(10);

    for i in 0..2u8 {
        write(LAPIC_ICR_HIGH, (apic_id as u32) << 24);
        write(LAPIC_ICR_LOW, 0x0000_4600 | (vector as u32));
        crate::serial_println!("[apic] send_sipi: SIPI #{} sent", i + 1);
        busy_wait_ms(1);
    }

    crate::serial_println!("[apic] send_sipi: done");
}

fn busy_wait_ms(ms: u64) {
    let cur = read(LAPIC_TIMER_CUR);
    if cur == 0 {
        for _ in 0..ms * 1_000_000 {
            core::hint::spin_loop();
        }
        return;
    }
    for _ in 0..ms * 500_000 {
        core::hint::spin_loop();
    }
}

fn read(offset: usize) -> u32 {
    let base = LAPIC_BASE.load(core::sync::atomic::Ordering::Relaxed);
    unsafe { core::ptr::read_volatile(((base as usize) + offset) as *const u32) }
}

fn write(offset: usize, value: u32) {
    let base = LAPIC_BASE.load(core::sync::atomic::Ordering::Relaxed);
    unsafe { core::ptr::write_volatile(((base as usize) + offset) as *mut u32, value) }
}

pub fn calibrate_timer(vector: u8) -> u32 {
    crate::serial_println!("[apic] calibrate_timer: begin vector={:#x}", vector);

    // One-shot モードで最大カウント（マスクなし = bit16=0）
    write(LAPIC_TIMER_DIV, 0x3);
    write(LAPIC_TIMER, vector as u32); // bit16=0 = unmasked, one-shot
    write(LAPIC_TIMER_INIT, 0xffff_ffff);

    // PIT 1 tick 分待つ（エッジ検出）
    let start = crate::interrupts::timer_ticks();
    crate::serial_println!(
        "[apic] calibrate_timer: waiting for first PIT tick (current={})",
        start
    );
    while crate::interrupts::timer_ticks() == start {
        core::hint::spin_loop();
    }

    let tick_start = crate::interrupts::timer_ticks();
    let apic_start = read(LAPIC_TIMER_CUR);
    crate::serial_println!(
        "[apic] calibrate_timer: tick_start={} apic_start={:#x}",
        tick_start,
        apic_start
    );

    // もう 1 tick 待つ（= 10ms）
    while crate::interrupts::timer_ticks() == tick_start {
        core::hint::spin_loop();
    }
    let apic_end = read(LAPIC_TIMER_CUR);
    crate::serial_println!(
        "[apic] calibrate_timer: tick_end={} apic_end={:#x}",
        crate::interrupts::timer_ticks(),
        apic_end
    );

    let counts_per_10ms = apic_start.wrapping_sub(apic_end);
    crate::serial_println!(
        "[apic] calibrate_timer: counts_per_10ms={} (~{} MHz)",
        counts_per_10ms,
        counts_per_10ms / 10_000
    );
    COUNTS_PER_10MS.store(
        counts_per_10ms as u64,
        core::sync::atomic::Ordering::Relaxed,
    );

    // 周期モード 100 Hz（10ms 周期）
    write(LAPIC_TIMER, LAPIC_TIMER_PERIODIC | (vector as u32));
    write(LAPIC_TIMER_INIT, counts_per_10ms);
    crate::serial_println!("[apic] calibrate_timer: periodic timer armed at 100Hz");

    counts_per_10ms
}

pub fn reload_higher_half() {
    let phys = LAPIC_BASE.load(core::sync::atomic::Ordering::Relaxed);
    let virt = crate::paging::phys_to_virt(phys);
    crate::serial_println!(
        "[apic] reload_higher_half: phys={:#x} -> virt={:#x}",
        phys,
        virt
    );
    LAPIC_BASE.store(virt, core::sync::atomic::Ordering::Relaxed);
}
