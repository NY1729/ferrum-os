#![allow(dead_code)]

pub const PAGE_SIZE: usize = 4096;
pub const MAX_ORDER: usize = 11;
const FREE_LIST_CAPACITY: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysAddr(u64);

impl PhysAddr {
    pub fn new(addr: u64) -> Self {
        Self(addr)
    }
    pub fn as_u64(self) -> u64 {
        self.0
    }
    pub fn as_usize(self) -> usize {
        self.0 as usize
    }
}

pub struct BuddyAllocator {
    free_blocks: [[u64; FREE_LIST_CAPACITY]; MAX_ORDER],
    free_counts: [usize; MAX_ORDER],
}

unsafe impl Send for BuddyAllocator {}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            free_blocks: [[0u64; FREE_LIST_CAPACITY]; MAX_ORDER],
            free_counts: [0usize; MAX_ORDER],
        }
    }

    /// メモリ破壊が発生していないか厳格にチェックする
    pub fn check_metadata_integrity(&self, caller: &str) {
        let meta_start = self as *const _ as u64;
        let meta_end = meta_start + (core::mem::size_of::<Self>() as u64);

        for order in 0..MAX_ORDER {
            if self.free_counts[order] > FREE_LIST_CAPACITY {
                panic!(
                    "[FATAL] {}: Metadata Corrupted at order {}: count={}",
                    caller, order, self.free_counts[order]
                );
            }
            for i in 0..self.free_counts[order] {
                let addr = self.free_blocks[order][i];
                if addr >= meta_start && addr < meta_end {
                    panic!(
                        "[FATAL] {}: Metadata pointer self-corruption! addr={:#x} in order {}",
                        caller, addr, order
                    );
                }
                if addr == 0 {
                    panic!(
                        "[FATAL] {}: NULL pointer in free list at order {} index {}",
                        caller, order, i
                    );
                }
            }
        }
    }

    pub fn add_region(&mut self, base: PhysAddr, size: usize) {
        let start = align_up(base.as_u64(), PAGE_SIZE as u64);
        let end = align_down(base.as_u64() + (size as u64), PAGE_SIZE as u64);

        if start >= end {
            return;
        }

        let mut addr = start;
        while addr + (PAGE_SIZE as u64) <= end {
            let remaining = (end - addr) as usize;
            let order = max_order_for(addr, remaining);
            self.push_block(PhysAddr::new(addr), order);
            addr += order_to_size(order) as u64;
        }
        self.check_metadata_integrity("add_region");
    }

    pub fn alloc(&mut self, order: usize) -> Option<PhysAddr> {
        self.check_metadata_integrity("alloc_start");

        let found_order = (order..MAX_ORDER).find(|&o| self.free_counts[o] > 0)?;
        let block = self.pop_block(found_order);

        let mut current_order = found_order;
        while current_order > order {
            current_order -= 1;
            let buddy = PhysAddr::new(block.as_u64() + (order_to_size(current_order) as u64));
            self.push_block(buddy, current_order);
        }

        self.check_metadata_integrity("alloc_end");
        Some(block)
    }

    pub fn free(&mut self, addr: PhysAddr, order: usize) {
        self.check_metadata_integrity("free_start");

        let mut current_addr = addr;
        let mut current_order = order;

        while current_order < MAX_ORDER - 1 {
            let buddy = buddy_of(current_addr, current_order);
            if let Some(idx) = self.find_block(buddy, current_order) {
                self.remove_block(idx, current_order);
                current_addr = PhysAddr::new(current_addr.as_u64().min(buddy.as_u64()));
                current_order += 1;
            } else {
                break;
            }
        }

        self.push_block(current_addr, current_order);
        self.check_metadata_integrity("free_end");
    }

    fn push_block(&mut self, addr: PhysAddr, order: usize) {
        if addr.as_u64() == 0 {
            panic!(
                "[FATAL] Pushing NULL (0) to BuddyAllocator! Order={}",
                order
            );
        }
        let count = self.free_counts[order];
        assert!(count < FREE_LIST_CAPACITY, "[buddy] free list overflow");
        self.free_blocks[order][count] = addr.as_u64();
        self.free_counts[order] += 1;
    }

    fn pop_block(&mut self, order: usize) -> PhysAddr {
        let count = self.free_counts[order] - 1;
        let addr = self.free_blocks[order][count];
        self.free_counts[order] = count;
        PhysAddr::new(addr)
    }

    fn find_block(&self, addr: PhysAddr, order: usize) -> Option<usize> {
        (0..self.free_counts[order]).find(|&i| self.free_blocks[order][i] == addr.as_u64())
    }

    fn remove_block(&mut self, idx: usize, order: usize) {
        let count = self.free_counts[order];
        self.free_blocks[order][idx] = self.free_blocks[order][count - 1];
        self.free_counts[order] -= 1;
    }

    pub fn dump(&self) {
        crate::serial_println!("[buddy] dump:");
        for order in 0..MAX_ORDER {
            if self.free_counts[order] > 0 {
                crate::serial_println!(
                    "[buddy]   order {:2}: {:4} blocks",
                    order,
                    self.free_counts[order]
                );
            }
        }
    }
}

// 補助関数
pub fn order_to_size(order: usize) -> usize {
    PAGE_SIZE << order
}
fn buddy_of(addr: PhysAddr, order: usize) -> PhysAddr {
    PhysAddr::new(((addr.as_u64() / (PAGE_SIZE as u64)) ^ (1u64 << order)) * (PAGE_SIZE as u64))
}
fn max_order_for(addr: u64, remaining: usize) -> usize {
    (0..MAX_ORDER)
        .rev()
        .find(|&o| {
            order_to_size(o) <= remaining && ((addr / (PAGE_SIZE as u64)) & ((1 << o) - 1)) == 0
        })
        .unwrap_or(0)
}
fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}
