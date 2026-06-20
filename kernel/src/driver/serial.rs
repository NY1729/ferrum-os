// kernel/src/driver/serial.rs
use crate::spinlock::IrqMutex;
use core::fmt::Write;
use uart_16550::SerialPort;

pub static SERIAL: IrqMutex<SerialPort> = unsafe { IrqMutex::new(SerialPort::new(0x3f8)) };

pub fn init() {
    SERIAL.lock().init();
}

/// ノンブロッキング受信。データがなければ None。
pub fn read_byte() -> Option<u8> {
    let ready: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") ready, in("dx") 0x3FDu16);
    }
    if (ready & 1) != 0 {
        let byte: u8;
        unsafe {
            core::arch::asm!("in al, dx", out("al") byte, in("dx") 0x3F8u16);
        }
        Some(byte)
    } else {
        None
    }
}

/// ブロッキング受信。割り込み有効状態で呼ぶこと。
pub fn read_byte_blocking() -> u8 {
    loop {
        if let Some(b) = read_byte() {
            return b;
        }
        x86_64::instructions::hlt();
    }
}

pub fn _print(args: core::fmt::Arguments) {
    SERIAL.lock().write_fmt(args).unwrap();
}
