#![allow(dead_code)]

use crate::allocator::{BuddyAllocator, PhysAddr};
use core::arch::asm;

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug)]
    pub struct PageFlags: u64 {
        const PRESENT  = 1 << 0;
        const WRITABLE = 1 << 1;
        const USER     = 1 << 2;
        const ACCESSED = 1 << 5;
        const DIRTY    = 1 << 6;
        const HUGE     = 1 << 7;
        const NO_EXEC  = 1 << 63;
    }
}

pub trait PageAllocator {
    fn alloc_page(&mut self) -> Option<(*mut u8, PhysAddr)>;
    fn free_page(&mut self, phys: PhysAddr);
    fn phys_to_ptr(&self, phys: PhysAddr) -> *mut u8;
}

const EARLY_PT_COUNT: usize = 64;

#[repr(C, align(4096))]
struct EarlyPageTableStorage {
    tables: [[u8; 4096]; EARLY_PT_COUNT],
}

static mut EARLY_PT_STORAGE: EarlyPageTableStorage = EarlyPageTableStorage {
    tables: [[0u8; 4096]; EARLY_PT_COUNT],
};
pub static EARLY_PT_IDX: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn early_storage_range() -> (u64, u64) {
    let base = &raw const EARLY_PT_STORAGE as u64;
    let end = base + ((EARLY_PT_COUNT * 4096) as u64);
    crate::serial_println!(
        "[paging] early_storage_range: [{:#x}, {:#x}) ({} KB)",
        base,
        end,
        (end - base) / 1024
    );
    (base, end)
}

pub struct EarlyAllocator;

impl PageAllocator for EarlyAllocator {
    fn alloc_page(&mut self) -> Option<(*mut u8, PhysAddr)> {
        let idx = EARLY_PT_IDX.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if idx >= EARLY_PT_COUNT {
            panic!("EarlyAllocator exhausted (idx={})", idx);
        }
        unsafe {
            let ptr = EARLY_PT_STORAGE.tables[idx].as_mut_ptr();
            core::ptr::write_bytes(ptr, 0, 4096);
            Some((ptr, PhysAddr::new(ptr as u64)))
        }
    }
    fn free_page(&mut self, _phys: PhysAddr) {}
    fn phys_to_ptr(&self, phys: PhysAddr) -> *mut u8 {
        phys.as_usize() as *mut u8
    }
}

impl PageAllocator for BuddyAllocator {
    fn alloc_page(&mut self) -> Option<(*mut u8, PhysAddr)> {
        let phys = self.alloc(0)?;
        let virt = phys_to_virt(phys.as_u64()) as *mut u8;
        Some((virt, phys))
    }
    fn free_page(&mut self, phys: PhysAddr) {
        self.free(phys, 0);
    }
    fn phys_to_ptr(&self, phys: PhysAddr) -> *mut u8 {
        phys_to_virt(phys.as_u64()) as *mut u8
    }
}

const PHYS_ADDR_MASK: u64 = 0x000f_ffff_ffff_f000;
pub const KERNEL_VIRT_BASE: u64 = 0xffff_8000_0000_0000;

pub static KERNEL_PHYS_BASE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

#[no_mangle]
pub static KERNEL_VIRT_BASE_STORAGE: u64 = KERNEL_VIRT_BASE;

pub fn kernel_phys_base() -> u64 {
    KERNEL_PHYS_BASE.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn phys_to_virt(phys: u64) -> u64 {
    match KERNEL_VIRT_BASE.checked_add(phys) {
        Some(v) => v,
        None => {
            crate::serial_println!("[paging] phys_to_virt OVERFLOW: phys={:#x}", phys);
            panic!("[paging] phys_to_virt: overflow phys={:#x}", phys);
        }
    }
}

pub fn ensure_virt(addr: u64) -> u64 {
    if addr >= KERNEL_VIRT_BASE {
        addr
    } else {
        phys_to_virt(addr)
    }
}

pub fn virt_to_phys(virt: u64) -> u64 {
    virt - KERNEL_VIRT_BASE
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        Self(0)
    }
    pub fn is_present(&self) -> bool {
        (self.0 & PageFlags::PRESENT.bits()) != 0
    }
    pub fn phys_addr(&self) -> PhysAddr {
        PhysAddr::new(self.0 & PHYS_ADDR_MASK)
    }
    pub fn flags(&self) -> PageFlags {
        PageFlags::from_bits_truncate(self.0)
    }
    pub fn new_table(phys: PhysAddr, flags: PageFlags) -> Self {
        Self(phys.as_u64() | flags.bits())
    }
    pub fn new_page(phys: PhysAddr, flags: PageFlags) -> Self {
        Self(phys.as_u64() | flags.bits())
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub fn zero(&mut self) {
        for e in self.entries.iter_mut() {
            *e = PageTableEntry::empty();
        }
    }
    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }
    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
    pub fn clear_entry(&mut self, index: usize) {
        self.entries[index] = PageTableEntry::empty();
    }
}

pub struct VirtAddrParts {
    pub pml4: usize,
    pub pdpt: usize,
    pub pd: usize,
    pub pt: usize,
    pub offset: usize,
}

impl VirtAddrParts {
    pub fn from_u64(addr: u64) -> Self {
        Self {
            pml4: ((addr >> 39) & 0x1ff) as usize,
            pdpt: ((addr >> 30) & 0x1ff) as usize,
            pd: ((addr >> 21) & 0x1ff) as usize,
            pt: ((addr >> 12) & 0x1ff) as usize,
            offset: (addr & 0xfff) as usize,
        }
    }
}

const SIZE_2MB: u64 = 0x20_0000;
const SIZE_4KB: u64 = 0x1000;

pub struct PageTableManager {
    pml4_phys: PhysAddr,
}

impl PageTableManager {
    pub fn new(allocator: &mut dyn PageAllocator) -> Option<Self> {
        let (ptr, pml4_phys) = allocator.alloc_page()?;
        unsafe {
            (*(ptr as *mut PageTable)).zero();
        }
        crate::serial_println!(
            "[paging] PageTableManager::new: pml4_phys={:#x}",
            pml4_phys.as_u64()
        );
        Some(Self { pml4_phys })
    }

    pub fn new_user(
        kernel_pml4_phys: PhysAddr,
        allocator: &mut dyn PageAllocator,
        owner_pid: crate::process::Pid,
    ) -> Option<Self> {
        let (ptr, new_pml4_phys) = allocator.alloc_page()?;
        crate::page_owner::track(new_pml4_phys.as_u64(), 0, "page-table:pml4", owner_pid);
        unsafe {
            let new_pml4 = ptr as *mut PageTable;
            let kern_pml4 = phys_to_virt(kernel_pml4_phys.as_u64()) as *const PageTable;
            (*new_pml4).zero();
            for i in 256..512 {
                (*new_pml4).entries[i] = (*kern_pml4).entries[i];
            }
        }
        Some(Self {
            pml4_phys: new_pml4_phys,
        })
    }

    pub fn map_range(
        &mut self,
        virt_start: u64,
        phys_start: u64,
        size: u64,
        flags: PageFlags,
        allocator: &mut dyn PageAllocator,
    ) -> Option<()> {
        if size == 0 {
            return Some(());
        }
        let mut virt = virt_start;
        let mut phys = phys_start;
        let end = virt_start.checked_add(size)?;

        while virt < end {
            let remaining = end - virt;
            if virt % SIZE_2MB == 0 && phys % SIZE_2MB == 0 && remaining >= SIZE_2MB {
                self.map_2mb(virt, PhysAddr::new(phys), flags, allocator)?;
                virt += SIZE_2MB;
                phys += SIZE_2MB;
            } else {
                self.map_4kb(virt, PhysAddr::new(phys), flags, allocator)?;
                virt += SIZE_4KB;
                phys += SIZE_4KB;
            }
        }
        Some(())
    }

    pub fn map(
        &mut self,
        virt: u64,
        phys: PhysAddr,
        flags: PageFlags,
        allocator: &mut dyn PageAllocator,
    ) -> Option<()> {
        self.map_4kb(virt, phys, flags, allocator)
    }

    fn map_4kb(
        &mut self,
        virt: u64,
        phys: PhysAddr,
        flags: PageFlags,
        allocator: &mut dyn PageAllocator,
    ) -> Option<()> {
        let parts = VirtAddrParts::from_u64(virt);

        // 中間テーブル（PDPT/PD/PT）のフラグ。
        // USER ビットを含めないと ring3 からアクセスできない。
        // ユーザーページをマップする場合は USER を伝播させる。
        let table_flags = if flags.contains(PageFlags::USER) {
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER
        } else {
            PageFlags::PRESENT | PageFlags::WRITABLE
        };

        unsafe {
            let pml4 = allocator.phys_to_ptr(self.pml4_phys) as *mut PageTable;

            let pdpt_phys = ensure_table(&mut (*pml4).entries[parts.pml4], allocator, table_flags)?;
            let pdpt = allocator.phys_to_ptr(pdpt_phys) as *mut PageTable;

            let pd_phys = ensure_table(&mut (*pdpt).entries[parts.pdpt], allocator, table_flags)?;
            let pd = allocator.phys_to_ptr(pd_phys) as *mut PageTable;

            let pt_phys = ensure_table(&mut (*pd).entries[parts.pd], allocator, table_flags)?;
            let pt = allocator.phys_to_ptr(pt_phys) as *mut PageTable;

            (*pt).entries[parts.pt] = PageTableEntry::new_page(phys, flags | PageFlags::PRESENT);
        }
        Some(())
    }

    fn map_2mb(
        &mut self,
        virt: u64,
        phys: PhysAddr,
        flags: PageFlags,
        allocator: &mut dyn PageAllocator,
    ) -> Option<()> {
        let parts = VirtAddrParts::from_u64(virt);

        let table_flags = if flags.contains(PageFlags::USER) {
            PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER
        } else {
            PageFlags::PRESENT | PageFlags::WRITABLE
        };

        unsafe {
            let pml4 = allocator.phys_to_ptr(self.pml4_phys) as *mut PageTable;

            let pdpt_phys = ensure_table(&mut (*pml4).entries[parts.pml4], allocator, table_flags)?;
            let pdpt = allocator.phys_to_ptr(pdpt_phys) as *mut PageTable;

            let pd_phys = ensure_table(&mut (*pdpt).entries[parts.pdpt], allocator, table_flags)?;
            let pd = allocator.phys_to_ptr(pd_phys) as *mut PageTable;

            (*pd).entries[parts.pd] =
                PageTableEntry::new_page(phys, flags | PageFlags::PRESENT | PageFlags::HUGE);
        }
        Some(())
    }

    pub fn unmap(&mut self, virt: u64) -> Option<PhysAddr> {
        let parts = VirtAddrParts::from_u64(virt);
        unsafe {
            let pml4 = &mut *(phys_to_virt(self.pml4_phys.as_u64()) as *mut PageTable);
            let e = &pml4.entries[parts.pml4];
            if !e.is_present() {
                return None;
            }

            let pdpt = &mut *(phys_to_virt(e.phys_addr().as_u64()) as *mut PageTable);
            let e = &pdpt.entries[parts.pdpt];
            if !e.is_present() {
                return None;
            }

            let pd = &mut *(phys_to_virt(e.phys_addr().as_u64()) as *mut PageTable);
            let e = &pd.entries[parts.pd];
            if !e.is_present() {
                return None;
            }

            let pt = &mut *(phys_to_virt(e.phys_addr().as_u64()) as *mut PageTable);
            let e = &mut pt.entries[parts.pt];
            if !e.is_present() {
                return None;
            }

            let phys = e.phys_addr();
            *e = PageTableEntry::empty();
            flush_tlb_page(virt);
            Some(phys)
        }
    }

    pub fn translate(&self, virt: u64) -> Option<PhysAddr> {
        let parts = VirtAddrParts::from_u64(virt);
        unsafe {
            let pml4 = &*(phys_to_virt(self.pml4_phys.as_u64()) as *const PageTable);
            let e = &pml4.entries[parts.pml4];
            if !e.is_present() {
                return None;
            }

            let pdpt = &*(phys_to_virt(e.phys_addr().as_u64()) as *const PageTable);
            let e = &pdpt.entries[parts.pdpt];
            if !e.is_present() {
                return None;
            }

            let pd = &*(phys_to_virt(e.phys_addr().as_u64()) as *const PageTable);
            let e = &pd.entries[parts.pd];
            if !e.is_present() {
                return None;
            }

            if e.flags().contains(PageFlags::HUGE) {
                let offset = virt & (SIZE_2MB - 1);
                return Some(PhysAddr::new(e.phys_addr().as_u64() + offset));
            }

            let pt = &*(phys_to_virt(e.phys_addr().as_u64()) as *const PageTable);
            let e = &pt.entries[parts.pt];
            if !e.is_present() {
                return None;
            }

            Some(PhysAddr::new(
                e.phys_addr().as_u64() + (parts.offset as u64),
            ))
        }
    }

    pub unsafe fn load(&self) {
        crate::serial_println!(
            "[paging] load CR3: pml4_phys={:#x}",
            self.pml4_phys.as_u64()
        );
        asm!("mov cr3, {}", in(reg) self.pml4_phys.as_u64());
    }

    pub fn pml4_phys(&self) -> PhysAddr {
        self.pml4_phys
    }

    pub fn from_phys(pml4_phys: PhysAddr) -> Self {
        Self { pml4_phys }
    }

    pub fn free_user_tables(&mut self, alloc: &mut BuddyAllocator) {
        unsafe {
            let pml4 = phys_to_virt(self.pml4_phys.as_u64()) as *mut PageTable;
            for i in 0..256 {
                let e = (*pml4).entries[i];
                if e.is_present() {
                    free_pdpt(e.phys_addr(), alloc);
                    (*pml4).entries[i] = PageTableEntry::empty();
                }
            }
        }
    }
}

unsafe fn free_pdpt(phys: PhysAddr, alloc: &mut BuddyAllocator) {
    let t = phys_to_virt(phys.as_u64()) as *mut PageTable;
    for i in 0..512 {
        let e = (*t).entries[i];
        if e.is_present() && !e.flags().contains(PageFlags::HUGE) {
            free_pd(e.phys_addr(), alloc);
        }
    }
    alloc.free(phys, 0);
}

unsafe fn free_pd(phys: PhysAddr, alloc: &mut BuddyAllocator) {
    let t = phys_to_virt(phys.as_u64()) as *mut PageTable;
    for i in 0..512 {
        let e = (*t).entries[i];
        if e.is_present() && !e.flags().contains(PageFlags::HUGE) {
            free_pt(e.phys_addr(), alloc);
        }
    }
    alloc.free(phys, 0);
}

unsafe fn free_pt(phys: PhysAddr, alloc: &mut BuddyAllocator) {
    let t = phys_to_virt(phys.as_u64()) as *mut PageTable;
    for i in 0..512 {
        let e = (*t).entries[i];
        if e.is_present() {
            (*t).entries[i] = PageTableEntry::empty();
        }
    }
    alloc.free(phys, 0); // PTページ自体はbuddy管理なので返す
}

// ensure_table: 中間テーブルのエントリが未設定なら新しいページを割り当てる。
// table_flags でUSERビットを制御する（ユーザーページには USER を含める）。
unsafe fn ensure_table(
    entry: &mut PageTableEntry,
    allocator: &mut dyn PageAllocator,
    table_flags: PageFlags,
) -> Option<PhysAddr> {
    if entry.is_present() {
        if entry.flags().contains(PageFlags::HUGE) {
            crate::serial_println!(
                "[paging] ensure_table: entry is a HUGE page, cannot walk into it as a table!"
            );
            return None;
        }
        // 既存エントリに USER が必要だが立っていない場合は追加する
        if table_flags.contains(PageFlags::USER) && !entry.flags().contains(PageFlags::USER) {
            let phys = entry.phys_addr();
            *entry = PageTableEntry::new_table(phys, entry.flags() | PageFlags::USER);
        }
        Some(entry.phys_addr())
    } else {
        let (ptr, phys) = allocator.alloc_page()?;
        if phys.as_u64() == 0 {
            panic!("[FATAL] ensure_table: allocator returned physical address 0!");
        }
        (*(ptr as *mut PageTable)).zero();
        *entry = PageTableEntry::new_table(phys, table_flags);
        Some(phys)
    }
}

pub fn flush_tlb() {
    unsafe {
        let cr3: u64;
        asm!("mov {}, cr3", out(reg) cr3);
        asm!("mov cr3, {}", in(reg) cr3);
    }
}

pub fn flush_tlb_page(virt: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) virt);
    }
}

pub fn map_identity(
    pt_manager: &mut PageTableManager,
    allocator: &mut dyn PageAllocator,
    mmap: &[MemoryRegion],
) -> Option<()> {
    for region in mmap {
        let start = region.start & !(SIZE_2MB - 1);
        let end = (region.start + region.size + SIZE_2MB - 1) & !(SIZE_2MB - 1);
        pt_manager.map_range(
            start,
            start,
            end - start,
            PageFlags::PRESENT | PageFlags::WRITABLE,
            allocator,
        )?;
    }
    Some(())
}

pub fn map_higher_half_region(
    pt_manager: &mut PageTableManager,
    allocator: &mut dyn PageAllocator,
    phys_start: u64,
    size: u64,
) -> Option<()> {
    let phys = phys_start & !(SIZE_2MB - 1);
    let end = (phys_start + size + SIZE_2MB - 1) & !(SIZE_2MB - 1);
    pt_manager.map_range(
        KERNEL_VIRT_BASE + phys,
        phys,
        end - phys,
        PageFlags::PRESENT | PageFlags::WRITABLE,
        allocator,
    )
}

pub fn map_higher_half(
    pt_manager: &mut PageTableManager,
    allocator: &mut dyn PageAllocator,
    image_base: u64,
    image_size: u64,
) -> Option<()> {
    map_higher_half_region(pt_manager, allocator, image_base, image_size)
}

#[derive(Clone, Copy, Default)]
pub struct MemoryRegion {
    pub start: u64,
    pub size: u64,
}

/// カーネルイメージサイズ（fork 時に子プロセス PML4 へのマップで使用）
pub static KERNEL_IMAGE_SIZE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// カーネルイメージの物理アドレス範囲をユーザー PML4 に identity map する。
pub fn map_kernel_image(pt: &mut PageTableManager, image_base_phys: u64, image_size: u64) {
    let flags = PageFlags::PRESENT | PageFlags::WRITABLE;
    let mut alloc = crate::ALLOCATOR.lock();
    let start = image_base_phys & !0xfff;
    let end = (image_base_phys + image_size + 0xfff) & !0xfff;

    let mut addr = start;
    while addr < end {
        {
            pt.map(
                addr,
                crate::allocator::PhysAddr::new(addr),
                flags,
                &mut *alloc,
            )
            .expect("map_kernel_image: map failed");
        }
        addr += 0x1000;
    }
}

pub unsafe fn sync_higher_half(dst_pml4_phys: u64, src: *const PageTable) {
    let dst = phys_to_virt(dst_pml4_phys) as *mut PageTable;
    for i in 256..512 {
        *(*dst).entry_mut(i) = *(*src).entry(i);
    }
}
