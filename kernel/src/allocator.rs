#![allow(dead_code)]

pub const PAGE_SIZE: usize = 4096;
pub const MAX_ORDER: usize = 11;

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

/// フリーブロックの先頭8バイトに「次のフリーブロックの物理アドレス」を
/// 埋め込むイントルーシブ連結リスト方式の buddy allocator。
/// ブロックがフリーである間だけ有効な手法（誰にもallocされていない前提）。
pub struct BuddyAllocator {
    free_heads: [u64; MAX_ORDER],    // 各orderの空きリスト先頭。0 = 空
    free_counts: [usize; MAX_ORDER], // 統計・整合性チェック用
}

unsafe impl Send for BuddyAllocator {}

impl BuddyAllocator {
    pub const fn new() -> Self {
        Self {
            free_heads: [0u64; MAX_ORDER],
            free_counts: [0usize; MAX_ORDER],
        }
    }

    fn read_next(addr: u64) -> u64 {
        let virt = crate::paging::phys_to_virt(addr) as *const u64;
        unsafe { core::ptr::read(virt) }
    }
    fn write_next(addr: u64, next: u64) {
        let virt = crate::paging::phys_to_virt(addr) as *mut u64;
        unsafe { core::ptr::write(virt, next) };
    }

    /// 各orderの空きリストを実際に辿って、長さがfree_countsと一致するか・
    /// 循環がないかを検査する（イントルーシブリスト版の整合性チェック）。
    pub fn check_metadata_integrity(&self, caller: &str) {
        for order in 0..MAX_ORDER {
            let mut addr = self.free_heads[order];
            let mut seen = 0usize;
            let limit = self.free_counts[order] + 1; // 循環検出用の余裕

            while addr != 0 {
                seen += 1;
                if seen > limit {
                    panic!(
                        "[FATAL] {}: free list cycle detected at order {} (count={})",
                        caller, order, self.free_counts[order]
                    );
                }
                addr = Self::read_next(addr);
            }

            if seen != self.free_counts[order] {
                panic!(
                    "[FATAL] {}: free list length mismatch at order {}: walked={} counted={}",
                    caller, order, seen, self.free_counts[order]
                );
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
        #[cfg(debug_assertions)]
        self.check_metadata_integrity("alloc_start");

        let found_order = (order..MAX_ORDER).find(|&o| self.free_counts[o] > 0)?;
        let block = self.pop_block(found_order);

        let mut current_order = found_order;
        while current_order > order {
            current_order -= 1;
            let buddy = PhysAddr::new(block.as_u64() + (order_to_size(current_order) as u64));
            self.push_block(buddy, current_order);
        }

        #[cfg(debug_assertions)]
        self.check_metadata_integrity("alloc_end");
        Some(block)
    }

    pub fn free(&mut self, addr: PhysAddr, order: usize) {
        #[cfg(debug_assertions)]
        self.check_metadata_integrity("free_start");

        let mut current_addr = addr;
        let mut current_order = order;
        while current_order < MAX_ORDER - 1 {
            let buddy = buddy_of(current_addr, current_order);
            if self.remove_from_list(buddy, current_order) {
                current_addr = PhysAddr::new(current_addr.as_u64().min(buddy.as_u64()));
                current_order += 1;
            } else {
                break;
            }
        }

        self.push_block(current_addr, current_order);
        #[cfg(debug_assertions)]
        self.check_metadata_integrity("free_end");
    }

    fn push_block(&mut self, addr: PhysAddr, order: usize) {
        if addr.as_u64() == 0 {
            panic!(
                "[FATAL] Pushing NULL (0) to BuddyAllocator! Order={}",
                order
            );
        }
        Self::write_next(addr.as_u64(), self.free_heads[order]);
        self.free_heads[order] = addr.as_u64();
        self.free_counts[order] += 1;
    }

    fn pop_block(&mut self, order: usize) -> PhysAddr {
        let addr = self.free_heads[order];
        debug_assert!(
            addr != 0,
            "[buddy] pop_block on empty list (order {})",
            order
        );
        let next = Self::read_next(addr);
        self.free_heads[order] = next;
        self.free_counts[order] -= 1;
        PhysAddr::new(addr)
    }

    /// 指定アドレスをそのorderの空きリストから取り除く（buddy合体用）
    fn remove_from_list(&mut self, addr: PhysAddr, order: usize) -> bool {
        let target = addr.as_u64();
        let mut cur = self.free_heads[order];
        let mut prev: Option<u64> = None;

        while cur != 0 {
            let next = Self::read_next(cur);
            if cur == target {
                match prev {
                    Some(p) => Self::write_next(p, next),
                    None => self.free_heads[order] = next,
                }
                self.free_counts[order] -= 1;
                return true;
            }
            prev = Some(cur);
            cur = next;
        }
        false
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
