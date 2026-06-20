#![allow(dead_code)]
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};
pub struct IrqMutex<T> {
    pub locked: AtomicBool,
    data: UnsafeCell<T>,
}
unsafe impl<T: Send> Send for IrqMutex<T> {}
unsafe impl<T: Send> Sync for IrqMutex<T> {}

impl<T> IrqMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
    pub fn lock(&self) -> IrqMutexGuard<'_, T> {
        let irq_was_enabled = x86_64::instructions::interrupts::are_enabled();
        x86_64::instructions::interrupts::disable();
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        IrqMutexGuard {
            mutex: self,
            irq_was_enabled,
        }
    }
    pub unsafe fn get_mut(&self) -> *mut T {
        self.data.get()
    }
}
pub struct IrqMutexGuard<'a, T> {
    mutex: &'a IrqMutex<T>,
    irq_was_enabled: bool,
}
impl<T> Deref for IrqMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<T> DerefMut for IrqMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}
impl<T> Drop for IrqMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Ordering::Release);
        // ロック前の状態に戻す
        if self.irq_was_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }
}
pub struct InitCell<T>(core::cell::UnsafeCell<core::mem::MaybeUninit<T>>);
unsafe impl<T: Send> Sync for InitCell<T> {}
impl<T> InitCell<T> {
    pub const fn new() -> Self {
        Self(core::cell::UnsafeCell::new(core::mem::MaybeUninit::uninit()))
    }
    pub unsafe fn write(&self, val: T) -> &mut T {
        (*self.0.get()).write(val)
    }
    pub unsafe fn assume_init_ref(&self) -> &T {
        (*self.0.get()).assume_init_ref()
    }
    pub unsafe fn assume_init_mut(&self) -> &mut T {
        (*self.0.get()).assume_init_mut()
    }
}
