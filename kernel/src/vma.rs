#![allow(dead_code)]

use crate::allocator::{BuddyAllocator, PAGE_SIZE};
use crate::paging::{PageFlags, PageTableManager};
use crate::process::Pid;
use alloc::vec::Vec;

// ── VMAFlags ─────────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VMAFlags {
    pub read: bool,
    pub write: bool,
    pub exec: bool,
}

impl VMAFlags {
    pub fn rw() -> Self {
        Self {
            read: true,
            write: true,
            exec: false,
        }
    }
    pub fn rx() -> Self {
        Self {
            read: true,
            write: false,
            exec: true,
        }
    }
    pub fn r() -> Self {
        Self {
            read: true,
            write: false,
            exec: false,
        }
    }

    pub fn to_page_flags(self) -> PageFlags {
        let mut flags = PageFlags::PRESENT | PageFlags::USER;
        if self.write {
            flags |= PageFlags::WRITABLE;
        }
        if !self.exec {
            flags |= PageFlags::NO_EXEC;
        }
        flags
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VMAKind {
    Anonymous,
    Stack,
    Heap,
}

impl VMAKind {
    pub fn owner_tag(self) -> &'static str {
        match self {
            VMAKind::Anonymous => "vma:anon",
            VMAKind::Stack => "vma:stack",
            VMAKind::Heap => "vma:heap",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VMA {
    pub start: u64,
    pub end: u64,
    pub flags: VMAFlags,
    pub kind: VMAKind,
}

impl VMA {
    pub fn new(start: u64, end: u64, flags: VMAFlags, kind: VMAKind) -> Self {
        Self {
            start,
            end,
            flags,
            kind,
        }
    }
    pub fn contains(&self, addr: u64) -> bool {
        self.start <= addr && addr < self.end
    }
    pub fn size(&self) -> u64 {
        self.end - self.start
    }
    /// 隣接(端点が一致)した時に1つに統合してよい属性かどうか。
    fn mergeable_with(&self, other: &VMA) -> bool {
        self.flags == other.flags && self.kind == other.kind
    }
}

pub struct AddressSpace {
    pub page_table: PageTableManager,
    /// start で常にソートされ、互いに重ならない VMA 一覧。
    /// 不変条件: i < j ならば vmas[i].end <= vmas[j].start
    pub vmas: Vec<VMA>,
    pub owner_pid: Pid,
}

impl AddressSpace {
    pub fn new(page_table: PageTableManager, owner_pid: Pid) -> Self {
        Self {
            page_table,
            vmas: Vec::new(),
            owner_pid,
        }
    }

    /// `vmas[i].start >= start` となる最小の添字を返す（lower_bound）。
    fn lower_bound(&self, start: u64) -> usize {
        self.vmas.partition_point(|v| v.start < start)
    }

    pub fn add_vma(&mut self, vma: VMA) -> Result<(), &'static str> {
        let idx = self.lower_bound(vma.start);

        // ソート済み・重なり無し不変条件があるので、前後2つだけ見れば
        // 重なりチェックとして十分（Linuxの vma_merge と同じ考え方）。
        if idx > 0 {
            let prev = &self.vmas[idx - 1];
            if prev.end > vma.start {
                return Err("VMA overlap");
            }
        }
        if idx < self.vmas.len() {
            let next = &self.vmas[idx];
            if vma.end > next.start {
                return Err("VMA overlap");
            }
        }

        let mut merged = vma;
        let mut insert_idx = idx;

        // 直前のVMAと連続していて属性が一致するなら統合
        if insert_idx > 0 {
            let prev = self.vmas[insert_idx - 1];
            if prev.end == merged.start && prev.mergeable_with(&merged) {
                merged.start = prev.start;
                self.vmas.remove(insert_idx - 1);
                insert_idx -= 1;
            }
        }
        // 直後のVMAと連続していて属性が一致するなら統合
        if insert_idx < self.vmas.len() {
            let next = self.vmas[insert_idx];
            if merged.end == next.start && merged.mergeable_with(&next) {
                merged.end = next.end;
                self.vmas.remove(insert_idx);
            }
        }

        self.vmas.insert(insert_idx, merged);
        Ok(())
    }

    pub fn find_vma(&self, addr: u64) -> Option<&VMA> {
        // start <= addr な最後の要素が候補（ソート済み・重なり無しなので一意）
        let idx = self.vmas.partition_point(|v| v.start <= addr);
        if idx == 0 {
            return None;
        }
        let candidate = &self.vmas[idx - 1];
        if candidate.contains(addr) {
            Some(candidate)
        } else {
            None
        }
    }

    pub fn remove_vma(&mut self, start: u64) -> Option<VMA> {
        let idx = self.lower_bound(start);
        if idx < self.vmas.len() && self.vmas[idx].start == start {
            Some(self.vmas.remove(idx))
        } else {
            None
        }
    }

    pub fn handle_page_fault(
        &mut self,
        fault_addr: u64,
        allocator: &mut BuddyAllocator,
    ) -> Result<(), &'static str> {
        let page_addr = fault_addr & !((PAGE_SIZE as u64) - 1);

        if self.page_table.translate(page_addr).is_some() {
            crate::paging::flush_tlb_page(page_addr);
            return Ok(());
        }

        let vma = self
            .find_vma(fault_addr)
            .copied()
            .ok_or("No VMA for fault address")?;

        let phys = allocator.alloc(0).ok_or("Out of memory")?;
        crate::page_owner::track(phys.as_u64(), 0, vma.kind.owner_tag(), self.owner_pid);

        unsafe {
            let ptr = crate::paging::phys_to_virt(phys.as_u64()) as *mut u8;
            core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
        }

        self.page_table
            .map(page_addr, phys, vma.flags.to_page_flags(), allocator)
            .ok_or("Failed to map page")?;

        crate::paging::flush_tlb_page(page_addr);

        Ok(())
    }

    pub fn mmap(
        &mut self,
        addr: u64,
        size: usize,
        flags: VMAFlags,
        kind: VMAKind,
    ) -> Result<u64, &'static str> {
        let aligned_size = align_up(size as u64, PAGE_SIZE as u64);

        let start = if addr == 0 {
            self.find_free_area(aligned_size)
                .ok_or("No free area for mmap")?
        } else {
            align_up(addr, PAGE_SIZE as u64)
        };
        let end = align_up(start + (size as u64), PAGE_SIZE as u64);

        self.add_vma(VMA::new(start, end, flags, kind))?;
        Ok(start)
    }

    pub fn munmap(
        &mut self,
        addr: u64,
        size: usize,
        allocator: &mut BuddyAllocator,
    ) -> Result<(), &'static str> {
        let start = align_up(addr, PAGE_SIZE as u64);
        let end = align_up(start + (size as u64), PAGE_SIZE as u64);

        self.remove_vma(start).ok_or("VMA not found")?;

        let mut page_addr = start;
        while page_addr < end {
            if let Some(phys) = self.page_table.unmap(page_addr) {
                allocator.free(phys, 0);
                crate::page_owner::untrack(phys.as_u64());
            }
            page_addr += PAGE_SIZE as u64;
        }
        Ok(())
    }

    // 全 VMA を unmap（物理ページ解放）し、VMA一覧をクリアする。
    pub fn clear_user(&mut self, alloc: &mut BuddyAllocator) {
        for vma in self.vmas.iter() {
            let mut addr = vma.start;
            while addr < vma.end {
                if let Some(phys) = self.page_table.translate(addr) {
                    alloc.free(phys, 0);
                    crate::page_owner::untrack(phys.as_u64());
                    self.page_table.unmap(addr);
                }
                addr += PAGE_SIZE as u64;
            }
        }
        self.vmas.clear();
        crate::paging::flush_tlb();
    }

    // 自身のアドレス空間に ELF をロード
    pub fn load_elf(
        &mut self,
        data: &[u8],
        alloc: &mut BuddyAllocator,
    ) -> Result<u64, &'static str> {
        let loader = match crate::elf::ElfLoader::new(data) {
            Ok(l) => l,
            Err(e) => return Err(e),
        };

        let pt = unsafe { &mut *(&mut self.page_table as *mut crate::paging::PageTableManager) };
        loader.load(pt, self, alloc)
    }

    pub fn write_to_phys_page(&mut self, phys: u64, offset: usize, data: &[u8]) {
        let temp_virt = 0xffff_ff00_0000_0000;

        let mut alloc = crate::ALLOCATOR.lock();
        self.page_table
            .map(
                temp_virt,
                crate::allocator::PhysAddr::new(phys),
                crate::paging::PageFlags::PRESENT | crate::paging::PageFlags::WRITABLE,
                &mut *alloc,
            )
            .expect("temp map failed");

        let dest = unsafe {
            core::slice::from_raw_parts_mut((temp_virt + (offset as u64)) as *mut u8, data.len())
        };
        dest.copy_from_slice(data);

        self.page_table.unmap(temp_virt).expect("temp unmap failed");
    }

    pub fn destroy(&mut self, alloc: &mut BuddyAllocator) {
        for vma in self.vmas.iter().copied() {
            let mut addr = vma.start;
            while addr < vma.end {
                if let Some(phys) = self.page_table.unmap(addr) {
                    alloc.free(phys, 0);
                    crate::page_owner::untrack(phys.as_u64());
                }
                addr += PAGE_SIZE as u64;
            }
        }
        self.vmas.clear();
        self.page_table.free_user_tables(alloc);
        alloc.free(self.page_table.pml4_phys(), 0);
        crate::page_owner::untrack(self.page_table.pml4_phys().as_u64());
        crate::paging::flush_tlb();
    }

    fn punch_hole(
        &mut self,
        hole_start: u64,
        hole_end: u64,
        alloc: &mut BuddyAllocator,
    ) -> Result<(), &'static str> {
        // hole と重なりうる最初のVMAを二分探索で特定
        let mut i = self.lower_bound(hole_start);
        if i > 0 && self.vmas[i - 1].end > hole_start {
            i -= 1;
        }

        let mut overlapping: Vec<VMA> = Vec::new();
        while i < self.vmas.len() && self.vmas[i].start < hole_end {
            overlapping.push(self.vmas[i]);
            i += 1;
        }

        for vma in overlapping {
            self.remove_vma(vma.start);

            let mut addr = core::cmp::max(vma.start, hole_start);
            let clip_end = core::cmp::min(vma.end, hole_end);
            while addr < clip_end {
                if let Some(phys) = self.page_table.unmap(addr) {
                    alloc.free(phys, 0);
                    crate::page_owner::untrack(phys.as_u64());
                }
                addr += PAGE_SIZE as u64;
            }

            // 左残り: [vma.start, hole_start)
            if vma.start < hole_start {
                self.add_vma(VMA::new(vma.start, hole_start, vma.flags, vma.kind))?;
            }
            // 右残り: [hole_end, vma.end)
            if vma.end > hole_end {
                self.add_vma(VMA::new(hole_end, vma.end, vma.flags, vma.kind))?;
            }
        }

        Ok(())
    }

    pub fn mmap_fixed(
        &mut self,
        addr: u64,
        size: usize,
        flags: VMAFlags,
        kind: VMAKind,
        alloc: &mut BuddyAllocator,
    ) -> Result<u64, &'static str> {
        let start = align_up(addr, PAGE_SIZE as u64);
        let end = align_up(start + (size as u64), PAGE_SIZE as u64);

        self.punch_hole(start, end, alloc)?;
        self.add_vma(VMA::new(start, end, flags, kind))?;
        Ok(start)
    }

    pub fn extend_heap(&mut self, brk_start: u64, new_end: u64) -> Result<(), &'static str> {
        let mut best_start: Option<u64> = None;
        let mut best_end = 0u64;
        for vma in self.vmas.iter() {
            if vma.kind == VMAKind::Heap && vma.end > best_end {
                best_end = vma.end;
                best_start = Some(vma.start);
            }
        }

        match best_start {
            Some(start) => {
                let existing = self.remove_vma(start).unwrap();
                self.add_vma(VMA::new(start, new_end, existing.flags, existing.kind))
            }
            None => self.add_vma(VMA::new(brk_start, new_end, VMAFlags::rw(), VMAKind::Heap)),
        }
    }

    // 匿名mmap用ゾーン内から、十分な大きさの空きギャップを探す。
    // vmas は既にソート済みなので、ゾーン内を1パス走査するだけでよい。
    fn find_free_area(&self, size: u64) -> Option<u64> {
        const MMAP_ZONE_START: u64 = 0x0000_6000_0000_0000;
        const MMAP_ZONE_END: u64 = 0x0000_7000_0000_0000;

        let mut cursor = MMAP_ZONE_START;
        for vma in self.vmas.iter() {
            if vma.start >= MMAP_ZONE_END {
                break;
            }
            if vma.end <= MMAP_ZONE_START {
                continue;
            }
            if vma.start.saturating_sub(cursor) >= size {
                return Some(cursor);
            }
            cursor = core::cmp::max(cursor, vma.end);
        }
        if MMAP_ZONE_END.saturating_sub(cursor) >= size {
            Some(cursor)
        } else {
            None
        }
    }
}

fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
