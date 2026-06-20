use crate::spinlock::IrqMutex;
use core::alloc::{GlobalAlloc, Layout};

const SLAB_SIZES: [usize; 10] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

#[repr(C)]
struct FreeNode {
    next: *mut FreeNode,
}

struct SlabCache {
    object_size: usize,
    free_list: *mut FreeNode,
}

impl SlabCache {
    const fn new(object_size: usize) -> Self {
        Self {
            object_size,
            free_list: core::ptr::null_mut(),
        }
    }

    unsafe fn alloc(&mut self) -> *mut u8 {
        if !self.free_list.is_null() {
            let node = self.free_list;
            self.free_list = (*node).next;
            node as *mut u8
        } else {
            crate::serial_println!(
                "[heap/slab] alloc: size={} free list empty -> grow",
                self.object_size
            );
            self.grow()
        }
    }

    unsafe fn grow(&mut self) -> *mut u8 {
        use crate::allocator::PAGE_SIZE;

        crate::serial_println!(
            "[heap/slab] grow: size={} requesting new page from buddy",
            self.object_size
        );

        let phys = {
            let mut alloc = crate::ALLOCATOR.lock();
            alloc.alloc(0)
        };

        let base = match phys {
            Some(p) => {
                let v = crate::paging::phys_to_virt(p.as_u64()) as usize;
                crate::serial_println!(
                    "[heap/slab] grow: size={} phys={:#x} virt={:#x}",
                    self.object_size,
                    p.as_u64(),
                    v
                );
                v
            }
            None => {
                crate::serial_println!(
                    "[heap/slab] grow: size={} FAILED (buddy OOM)",
                    self.object_size
                );
                return core::ptr::null_mut();
            }
        };

        core::ptr::write_bytes(base as *mut u8, 0, PAGE_SIZE);

        let slots = PAGE_SIZE / self.object_size;
        let mut offset = self.object_size;
        while offset + self.object_size <= PAGE_SIZE {
            let node = (base + offset) as *mut FreeNode;
            (*node).next = self.free_list;
            self.free_list = node;
            offset += self.object_size;
        }

        crate::serial_println!(
            "[heap/slab] grow: size={} added {} slots to free list",
            self.object_size,
            slots - 1
        );

        base as *mut u8
    }

    unsafe fn free(&mut self, ptr: *mut u8) {
        let node = ptr as *mut FreeNode;
        (*node).next = self.free_list;
        self.free_list = node;
    }
}

unsafe impl Send for SlabCache {}

fn pages_to_order(pages: usize) -> usize {
    let mut order = 0;
    let mut n = 1;
    while n < pages {
        n <<= 1;
        order += 1;
    }
    order
}

fn size_to_slab(size: usize) -> Option<usize> {
    SLAB_SIZES.iter().position(|&s| s >= size)
}

pub struct KernelHeap {
    slabs: [SlabCache; 10],
}

impl KernelHeap {
    pub const fn new() -> Self {
        Self {
            slabs: [
                SlabCache::new(8),
                SlabCache::new(16),
                SlabCache::new(32),
                SlabCache::new(64),
                SlabCache::new(128),
                SlabCache::new(256),
                SlabCache::new(512),
                SlabCache::new(1024),
                SlabCache::new(2048),
                SlabCache::new(4096),
            ],
        }
    }

    unsafe fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(layout.align()).max(8);
        if let Some(idx) = size_to_slab(size) {
            self.slabs[idx].alloc()
        } else {
            use crate::allocator::PAGE_SIZE;
            let pages = size.div_ceil(PAGE_SIZE);
            let order = pages_to_order(pages);
            crate::serial_println!(
                "[heap] alloc: large size={} pages={} order={}",
                size,
                pages,
                order
            );
            let mut alloc = crate::ALLOCATOR.lock();
            match alloc.alloc(order) {
                Some(p) => {
                    let v = crate::paging::phys_to_virt(p.as_u64()) as *mut u8;
                    crate::serial_println!(
                        "[heap] alloc: large phys={:#x} virt={:#x}",
                        p.as_u64(),
                        v as u64
                    );
                    v
                }
                None => {
                    crate::serial_println!("[heap] alloc: large FAILED (OOM)");
                    core::ptr::null_mut()
                }
            }
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(layout.align()).max(8);
        if let Some(idx) = size_to_slab(size) {
            self.slabs[idx].free(ptr);
        } else {
            use crate::allocator::{PhysAddr, PAGE_SIZE};
            let pages = size.div_ceil(PAGE_SIZE);
            let order = pages_to_order(pages);
            let phys = crate::paging::virt_to_phys(ptr as u64);
            crate::serial_println!(
                "[heap] dealloc: large ptr={:#x} phys={:#x} pages={} order={}",
                ptr as u64,
                phys,
                pages,
                order
            );
            let mut alloc = crate::ALLOCATOR.lock();
            alloc.free(PhysAddr::new(phys), order);
        }
    }
}

unsafe impl Send for KernelHeap {}

pub struct LockedHeap(IrqMutex<KernelHeap>);

impl LockedHeap {
    pub const fn new() -> Self {
        Self(IrqMutex::new(KernelHeap::new()))
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = self.0.lock().alloc(layout);
        if ptr.is_null() {
            panic!(
                "[heap] GlobalAlloc::alloc OOM: size={} align={}",
                layout.size(),
                layout.align()
            );
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().dealloc(ptr, layout);
    }
}
