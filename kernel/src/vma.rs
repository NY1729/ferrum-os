#![allow(dead_code)]

use crate::allocator::{BuddyAllocator, PAGE_SIZE};
use crate::paging::{PageFlags, PageTableManager};

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
}

pub struct AddressSpace {
    pub page_table: PageTableManager,
    pub vmas: [Option<VMA>; 64],
    vma_count: usize,
}

impl AddressSpace {
    pub fn new(page_table: PageTableManager) -> Self {
        Self {
            page_table,
            vmas: [None; 64],
            vma_count: 0,
        }
    }

    pub fn add_vma(&mut self, vma: VMA) -> Result<(), &'static str> {
        crate::serial_println!(
            "[vma] add_vma: [{:#x}, {:#x}) flags={:?} kind={:?}",
            vma.start,
            vma.end,
            vma.flags,
            vma.kind
        );
        if self.vma_count >= 64 {
            return Err("Too Many VMAs");
        }
        for existing in self.vmas.iter().flatten() {
            if vma.start < existing.end && vma.end > existing.start {
                return Err("VMA overlap");
            }
        }
        for slot in self.vmas.iter_mut() {
            if slot.is_none() {
                *slot = Some(vma);
                self.vma_count += 1;
                return Ok(());
            }
        }
        Err("No free VMA slot")
    }

    pub fn find_vma(&self, addr: u64) -> Option<&VMA> {
        self.vmas.iter().flatten().find(|vma| vma.contains(addr))
    }

    pub fn remove_vma(&mut self, start: u64) -> Option<VMA> {
        for slot in self.vmas.iter_mut() {
            if let Some(vma) = slot {
                if vma.start == start {
                    let removed = *vma;
                    *slot = None;
                    self.vma_count -= 1;
                    return Some(removed);
                }
            }
        }
        None
    }

    pub fn handle_page_fault(
        &mut self,
        fault_addr: u64,
        allocator: &mut BuddyAllocator,
    ) -> Result<(), &'static str> {
        let page_addr = fault_addr & !((PAGE_SIZE as u64) - 1);
        crate::serial_println!(
            "[vma] handle_page_fault: fault_addr={:#x} page_addr={:#x}",
            fault_addr,
            page_addr
        );

        // 既マップ済みの場合は TLB フラッシュだけして正常終了
        if self.page_table.translate(page_addr).is_some() {
            crate::serial_println!(
                "[vma] handle_page_fault: page {:#x} already mapped, flushing TLB",
                page_addr
            );
            crate::paging::flush_tlb_page(page_addr);
            return Ok(());
        }

        let vma = self
            .vmas
            .iter()
            .flatten()
            .find(|vma| vma.contains(fault_addr))
            .copied()
            .ok_or("No VMA for fault address")?;

        crate::serial_println!(
            "[vma] handle_page_fault: VMA [{:#x},{:#x}) flags={:?}",
            vma.start,
            vma.end,
            vma.flags
        );

        let phys = allocator.alloc(0).ok_or("Out of memory")?;

        // 物理ページをゼロ初期化
        unsafe {
            let ptr = crate::paging::phys_to_virt(phys.as_u64()) as *mut u8;
            core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
        }

        self.page_table
            .map(page_addr, phys, vma.flags.to_page_flags(), allocator)
            .ok_or("Failed to map page")?;

        // 新規マッピングは TLB キャッシュ不要だが、念のためフラッシュ
        crate::paging::flush_tlb_page(page_addr);

        crate::serial_println!(
            "[vma] handle_page_fault: mapped page_addr={:#x} -> phys={:#x}",
            page_addr,
            phys.as_u64()
        );
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
        crate::serial_println!(
            "[vma] mmap: addr={:#x} size={:#x} -> [{:#x},{:#x})",
            addr,
            size,
            start,
            end
        );
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
        crate::serial_println!("[vma] munmap: [{:#x},{:#x})", start, end);

        self.remove_vma(start).ok_or("VMA not found")?;

        let mut page_addr = start;
        while page_addr < end {
            // unmap() 内部で flush_tlb_page を呼んでいる
            if let Some(phys) = self.page_table.unmap(page_addr) {
                allocator.free(phys, 0);
            }
            page_addr += PAGE_SIZE as u64;
        }
        crate::serial_println!("[vma] munmap: done");
        Ok(())
    }

    // 全 VMA を unmap（物理ページ解放）し、VMA テーブルをクリアする。
    pub fn clear_user(&mut self, alloc: &mut BuddyAllocator) {
        for vma in self.vmas.iter().flatten() {
            let mut addr = vma.start;
            while addr < vma.end {
                // 1. translate して物理アドレスとフラグを取得
                if let Some(phys) = self.page_table.translate(addr) {
                    // translate を使ってマッピングが存在するか確認し、unmap を実行
                    alloc.free(phys, 0);
                    self.page_table.unmap(addr);
                }
                addr += PAGE_SIZE as u64;
            }
        }
        self.vmas = [None; 64];
        self.vma_count = 0;
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
            Err(e) => {
                return Err(e);
            }
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

    // vma.rs
    pub fn destroy(&mut self, alloc: &mut BuddyAllocator) {
        for vma in self.vmas.iter().flatten().copied() {
            let mut addr = vma.start;
            while addr < vma.end {
                if let Some(phys) = self.page_table.unmap(addr) {
                    alloc.free(phys, 0);
                }
                addr += PAGE_SIZE as u64;
            }
        }
        self.vmas = [None; 64];
        self.vma_count = 0;
        self.page_table.free_user_tables(alloc);
        alloc.free(self.page_table.pml4_phys(), 0);
        crate::paging::flush_tlb();
    }

    fn punch_hole(
        &mut self,
        hole_start: u64,
        hole_end: u64,
        alloc: &mut BuddyAllocator,
    ) -> Result<(), &'static str> {
        let mut overlapping: [Option<VMA>; 64] = [None; 64];
        let mut n = 0;

        for vma in self.vmas.iter().flatten() {
            if hole_start < vma.end && hole_end > vma.start {
                overlapping[n] = Some(*vma);
                n += 1;
            }
        }

        for slot in overlapping.iter().take(n) {
            let vma = slot.unwrap();
            self.remove_vma(vma.start);

            // 重なっている部分の物理ページを解放
            let mut addr = core::cmp::max(vma.start, hole_start);
            let clip_end = core::cmp::min(vma.end, hole_end);
            while addr < clip_end {
                if let Some(phys) = self.page_table.unmap(addr) {
                    alloc.free(phys, 0);
                }
                addr += PAGE_SIZE as u64;
            }

            if vma.start < hole_start {
                self.add_vma(VMA::new(vma.start, hole_end, vma.flags, vma.kind))?;
            }
            if vma.end > hole_end {
                self.add_vma(VMA::new(hole_end, vma.end, vma.flags, vma.kind))?;
            }
        }

        Ok(())
    }

    // 指定アドレスへ強制的にマッピングする
    // 既存の重なるVMAがあれば穴をあけてから新しい VMA を追加する。
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
        crate::serial_println!(
            "[vma] mmap_fixed: addr={:#x} size={:#x} -> [{:#x},{:#x})",
            addr,
            size,
            start,
            end
        );
        self.punch_hole(start, end, alloc)?;
        self.add_vma(VMA::new(start, end, flags, kind))?;
        Ok(start)
    }

    pub fn extend_heap(&mut self, brk_start: u64, new_end: u64) -> Result<(), &'static str> {
        let mut best_start: Option<u64> = None;
        let mut best_end = 0u64;
        for vma in self.vmas.iter().flatten() {
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
    fn find_free_area(&self, size: u64) -> Option<u64> {
        const MMAP_ZONE_START: u64 = 0x0000_6000_0000_0000;
        const MMAP_ZONE_END: u64 = 0x0000_7000_0000_0000;

        let mut zone_vmas: [(u64, u64); 64] = [(0, 0); 64];
        let mut n = 0;
        for vma in self.vmas.iter().flatten() {
            if vma.start >= MMAP_ZONE_START && vma.end <= MMAP_ZONE_END {
                zone_vmas[n] = (vma.start, vma.end);
                n += 1;
            }
        }

        for i in 1..n {
            let key = zone_vmas[i];
            let mut j = i;
            while j > 0 && zone_vmas[j - 1].0 > key.0 {
                zone_vmas[j] = zone_vmas[j - 1];
                j -= 1;
            }
            zone_vmas[j] = key;
        }

        let mut cursor = MMAP_ZONE_START;
        for i in 0..n {
            let (vstart, vend) = zone_vmas[i];
            if vstart.saturating_sub(cursor) >= size {
                return Some(cursor);
            }
            cursor = core::cmp::max(cursor, vend);
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
