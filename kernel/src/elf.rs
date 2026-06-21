use crate::allocator::{BuddyAllocator, PhysAddr, PAGE_SIZE};
use crate::paging::PageTableManager;
use crate::vma::{AddressSpace, VMAFlags, VMAKind};

const ELFMAG: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1; // little-endian
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;

// p_flags
const PF_X: u32 = 1; // execute
const PF_W: u32 = 2; // write
const PF_R: u32 = 4; // read

#[repr(C, packed)]
struct Elf64Ehdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C, packed)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

pub struct ElfLoader<'a> {
    data: &'a [u8],
    ehdr: &'a Elf64Ehdr,
}

impl<'a> ElfLoader<'a> {
    /// ELFバイト列を検証してローダーを作成する。
    pub fn new(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < core::mem::size_of::<Elf64Ehdr>() {
            return Err("ELF: data too small for header");
        }

        // SAFETY: サイズチェック済み、Elf64Ehdrはpacked repr(C)
        let ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };

        // マジック
        if ehdr.e_ident[..4] != ELFMAG {
            return Err("ELF: bad magic");
        }
        // 64bit
        if ehdr.e_ident[4] != ELFCLASS64 {
            return Err("ELF: not ELF64");
        }
        // リトルエンディアン
        if ehdr.e_ident[5] != ELFDATA2LSB {
            return Err("ELF: not little-endian");
        }
        // 実行ファイル
        if { ehdr.e_type } != ET_EXEC {
            return Err("ELF: not ET_EXEC");
        }
        // x86-64
        if { ehdr.e_machine } != EM_X86_64 {
            return Err("ELF: not x86-64");
        }

        let phoff = ehdr.e_phoff as usize;
        let phnum = ehdr.e_phnum as usize;
        let phentsz = ehdr.e_phentsize as usize;

        if phentsz < core::mem::size_of::<Elf64Phdr>() {
            return Err("ELF: e_phentsize too small");
        }

        let phdr_end = phoff
            .checked_add(
                phnum
                    .checked_mul(phentsz)
                    .ok_or("ELF: phdr size overflow")?,
            )
            .ok_or("ELF: phdr offset overflow")?;
        if phdr_end > data.len() {
            return Err("ELF: program header table out of bounds");
        }

        crate::serial_println!(
            "[elf] ELF64 validated: entry={:#x} phnum={} phoff={:#x}",
            { ehdr.e_entry },
            phnum,
            phoff,
        );

        Ok(Self { data, ehdr })
    }

    pub fn load(
        &self,
        pt: &mut PageTableManager,
        as_: &mut AddressSpace,
        allocator: &mut BuddyAllocator,
    ) -> Result<u64, &'static str> {
        let phoff = self.ehdr.e_phoff as usize;
        let phnum = self.ehdr.e_phnum as usize;
        let phentsz = self.ehdr.e_phentsize as usize;

        for i in 0..phnum {
            let offset = phoff + i * phentsz;
            // SAFETY: new() でバウンドチェック済み
            let phdr = unsafe { &*(self.data[offset..].as_ptr() as *const Elf64Phdr) };

            if { phdr.p_type } != PT_LOAD {
                continue;
            }

            let p_vaddr = { phdr.p_vaddr };
            let p_filesz = { phdr.p_filesz };
            let p_memsz = { phdr.p_memsz };
            let p_offset = { phdr.p_offset };
            let p_flags = { phdr.p_flags };

            crate::serial_println!(
                "[elf] PT_LOAD[{}]: vaddr={:#x} filesz={:#x} memsz={:#x} flags={:#x}",
                i,
                p_vaddr,
                p_filesz,
                p_memsz,
                p_flags,
            );

            if p_memsz == 0 {
                continue;
            }

            // ファイルデータの境界チェック
            let file_start = p_offset as usize;
            let file_end = file_start
                .checked_add(p_filesz as usize)
                .ok_or("ELF: p_filesz overflow")?;
            if file_end > self.data.len() {
                return Err("ELF: PT_LOAD file data out of bounds");
            }

            // VMA フラグを計算
            let vma_flags = pflags_to_vma(p_flags);

            // ページ境界に揃えたマップ範囲
            let map_start = align_down(p_vaddr, PAGE_SIZE as u64);
            let map_end = align_up(p_vaddr + p_memsz, PAGE_SIZE as u64);
            let map_pages = ((map_end - map_start) / PAGE_SIZE as u64) as usize;

            crate::serial_println!(
                "[elf]   map [{:#x}, {:#x}) {} pages flags={:?}",
                map_start,
                map_end,
                map_pages,
                vma_flags,
            );

            // VMA 登録
            as_.add_vma(crate::vma::VMA::new(
                map_start,
                map_end,
                vma_flags,
                VMAKind::Anonymous,
            ))
            .map_err(|_| "ELF: add_vma failed")?;

            // ページを1枚ずつ確保してマップ・データコピー
            for page_idx in 0..map_pages {
                let page_vaddr = map_start + (page_idx as u64) * PAGE_SIZE as u64;

                // 物理ページ確保
                let phys: PhysAddr = allocator
                    .alloc(0)
                    .ok_or("ELF: out of memory for PT_LOAD page")?;
                crate::page_owner::track(phys.as_u64(), 0, "elf:segment", as_.owner_pid);

                // 仮想アドレス（higher-half経由）でゼロ埋め
                let virt_ptr = crate::paging::phys_to_virt(phys.as_u64()) as *mut u8;
                unsafe { core::ptr::write_bytes(virt_ptr, 0, PAGE_SIZE) };

                // このページにコピーすべきファイルデータを計算
                // ページ内のバイト範囲: [copy_start, copy_end)
                let page_va_start = page_vaddr;
                let page_va_end = page_vaddr + PAGE_SIZE as u64;

                // セグメントのファイルデータが存在するVA範囲: [p_vaddr, p_vaddr+p_filesz)
                let seg_file_va_start = p_vaddr;
                let seg_file_va_end = p_vaddr + p_filesz;

                // 交差部分
                let copy_va_start = page_va_start.max(seg_file_va_start);
                let copy_va_end = page_va_end.min(seg_file_va_end);

                if copy_va_start < copy_va_end {
                    let copy_len = (copy_va_end - copy_va_start) as usize;

                    // ページ内オフセット
                    let page_offset = (copy_va_start - page_va_start) as usize;

                    // ファイル内オフセット
                    let file_copy_start = file_start + (copy_va_start - seg_file_va_start) as usize;

                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            self.data[file_copy_start..].as_ptr(),
                            virt_ptr.add(page_offset),
                            copy_len,
                        );
                    }
                }

                // ページテーブルにマップ
                let page_flags = vma_flags.to_page_flags();
                pt.map(page_vaddr, phys, page_flags, allocator)
                    .ok_or("ELF: page table map failed")?;
            }
        }

        let entry = { self.ehdr.e_entry };
        crate::serial_println!("[elf] load complete: entry={:#x}", entry);
        Ok(entry)
    }
}

fn pflags_to_vma(p_flags: u32) -> VMAFlags {
    VMAFlags {
        read: (p_flags & PF_R) != 0,
        write: (p_flags & PF_W) != 0,
        exec: (p_flags & PF_X) != 0,
    }
}

fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}

fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
