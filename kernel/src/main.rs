#![no_std]
#![no_main]

mod allocator;
mod ap_boot;
mod apic;
mod driver;
mod elf;
mod fd;
mod fs;
mod gdt;
mod heap;
mod interrupts;
mod paging;
mod pic;
mod process;
mod spinlock;
mod syscall;
mod tests;
mod vma;

extern crate alloc;

use crate::{allocator::PAGE_SIZE, vma::AddressSpace};
use allocator::BuddyAllocator;
use core::sync::atomic::Ordering;
use fs::ramfs::Ramfs;
use fs::vfs::{FileType, Vfs};
use paging::{map_identity, MemoryRegion, PageTableManager};
use spinlock::IrqMutex;
use uefi::mem::memory_map::MemoryMap;

pub static ROOTFS: IrqMutex<Ramfs> = IrqMutex::new(Ramfs::new());
// バディアロケータ（物理ページ管理）
static ALLOCATOR: IrqMutex<BuddyAllocator> = IrqMutex::new(BuddyAllocator::new());

// カーネルのアドレス空間（テスト・demand_paging 用）
static ADDRESS_SPACE: IrqMutex<Option<AddressSpace>> = IrqMutex::new(None);

// プロセススケジューラ
static mut SCHEDULER: process::Scheduler = process::Scheduler::new();

// グローバルアロケータ（slab ヒープ）
#[global_allocator]
static HEAP: heap::LockedHeap = heap::LockedHeap::new();

pub fn _print(args: core::fmt::Arguments) {
    driver::serial::_print(args);
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => (crate::_print(format_args!($($arg)*)));
}
#[macro_export]
macro_rules! serial_println {
    () => (crate::serial_print!("\n"));
    ($fmt:expr) => (crate::serial_print!(concat!($fmt, "\n")));
    (
        $fmt:expr,
        $($arg:tt)*
    ) => (crate::serial_print!(concat!($fmt, "\n"), $($arg)*));
}

#[no_mangle]
pub extern "efiapi" fn efi_main(
    image_handle: uefi::Handle,
    system_table: *const core::ffi::c_void,
) -> uefi::Status {
    let is_bsp: bool;
    unsafe {
        let eax: u32;
        core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, out("eax") eax, out("edx") _);
        is_bsp = (eax & (1 << 8)) != 0;
    }
    if !is_bsp {
        x86_64::instructions::interrupts::disable();
        loop {
            x86_64::instructions::hlt();
        }
    }

    unsafe {
        uefi::boot::set_image_handle(image_handle);
        uefi::table::set_system_table(system_table.cast());
    }
    kernel_main()
}

fn kernel_main() -> uefi::Status {
    driver::serial::init();
    serial_println!("============================================================");
    serial_println!("Ferrum OS booting...");
    serial_println!("============================================================");

    // カーネルイメージの物理ベース・サイズを記録
    let (image_base, image_size) = get_image_info();
    serial_println!(
        "[main] kernel image: base={:#x} size={:#x} ({} KB)",
        image_base,
        image_size,
        image_size / 1024
    );
    paging::KERNEL_PHYS_BASE.store(image_base, Ordering::Relaxed);
    paging::KERNEL_IMAGE_SIZE.store(image_size, Ordering::Relaxed);

    let elf_files = [
        ("init", "\\user.elf"),
        ("hello", "\\hello.elf"),
        ("cat", "\\cat.elf"),
        ("echo", "\\echo.elf"),
        ("mmap_test", "\\mmap_test.elf"),
        ("dev_test", "\\dev_test.elf"),
        ("busybox", "\\busybox"),
    ];

    let mut loaded_elfs: [Option<(&'static str, &'static [u8])>; 16] = [None; 16];

    for (i, (name, path)) in elf_files.iter().enumerate() {
        if let Some(bytes) = load_file_from_esp(path) {
            serial_println!("[main] {} loaded: {} bytes", path, bytes.len());
            loaded_elfs[i] = Some((*name, bytes));
        } else {
            serial_println!("[main] {} not found", path);
        }
    }

    serial_println!("[main] exiting boot services...");
    let mmap = unsafe { uefi::boot::exit_boot_services(uefi::boot::MemoryType::LOADER_DATA) };
    serial_println!(
        "[main] boot services exited, memory map entries={}",
        mmap.entries().count()
    );

    pic::init();
    pic::init_pit(100);
    apic::init();
    serial_println!("[main] interrupt controllers initialized");

    serial_println!("[main] setting up paging...");
    setup_paging(&mmap);
    serial_println!("[main] paging setup complete");

    serial_println!(
        "[main] jumping to higher half (virt_base={:#x})...",
        paging::KERNEL_VIRT_BASE
    );
    unsafe {
        jump_to_higher_half();
    }
    serial_println!("[main] now running in higher half!");

    x86_64::instructions::interrupts::disable();

    serial_println!("[main] initializing GDT...");
    gdt::init();

    serial_println!("[main] initializing IDT...");
    interrupts::init();

    serial_println!("[main] initializing syscall...");
    syscall::init();

    serial_println!("[main] reloading APIC base address...");
    apic::reload_higher_half();

    serial_println!("[main] enabling interrupts for APIC timer calibration...");
    x86_64::instructions::interrupts::enable();

    serial_println!("[main] calibrating APIC timer...");
    apic::calibrate_timer(0x20);

    x86_64::instructions::interrupts::disable();

    serial_println!("[main] masking PIT IRQ0...");
    pic::mask_irq0();
    interrupts::PIT_MASKED.store(true, Ordering::Relaxed);
    serial_println!("[main] PIT masked, using APIC timer only");

    serial_println!("[main] running kernel tests...");
    tests::run_all();

    {
        let mut rootfs = ROOTFS.lock();
        rootfs
            .write_file("/hello.txt", b"Hello from ramfs!\n")
            .expect("write_file failed");

        for elf_opt in loaded_elfs.iter() {
            if let Some((name, bytes)) = elf_opt {
                let path = alloc::format!("/{}", name);
                rootfs
                    .write_file(&path, *bytes)
                    .expect("Failed to write elf to ramfs");
            }
        }
    }
    serial_println!("[main] wrote /hello.txt to ramfs");

    let init_bytes = loaded_elfs.iter().find_map(|opt| {
        if let Some(("init", bytes)) = opt {
            Some(*bytes)
        } else {
            None
        }
    });

    serial_println!("[main] starting scheduler...");
    start_scheduler(image_size, init_bytes);

    serial_println!("[main] returned from start_scheduler, entering idle hlt loop");
    loop {
        x86_64::instructions::hlt();
    }
}

fn setup_paging(mmap: &uefi::mem::memory_map::MemoryMapOwned) {
    serial_println!("[main] setup_paging: begin");

    let rsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
    }
    let stack_phys_base = rsp.saturating_sub(0x10_0000) & !0xfff;
    let stack_phys_end = stack_phys_base + 0x20_0000;
    serial_println!(
        "[main] setup_paging: stack phys=[{:#x},{:#x}) rsp={:#x}",
        stack_phys_base,
        stack_phys_end,
        rsp
    );

    let (early_phys_base, early_phys_end) = paging::early_storage_range();
    let mut early_alloc = paging::EarlyAllocator;
    let mut pt_manager =
        PageTableManager::new(&mut early_alloc).expect("Failed to create page table");

    // identity mapping
    serial_println!("[main] setup_paging: mapping identity...");
    let mut identity_count = 0u32;
    for entry in mmap.entries() {
        let region = MemoryRegion {
            start: entry.phys_start,
            size: entry.page_count * (PAGE_SIZE as u64),
        };
        map_identity(&mut pt_manager, &mut early_alloc, &[region]).expect("Failed to map identity");
        identity_count += 1;
    }
    map_identity(
        &mut pt_manager,
        &mut early_alloc,
        &[MemoryRegion {
            start: 0xfee0_0000,
            size: 0x1000,
        }],
    )
    .expect("Failed to map LAPIC identity");
    map_identity(
        &mut pt_manager,
        &mut early_alloc,
        &[MemoryRegion {
            start: 0x0000_0000,
            size: 0x0001_0000,
        }],
    )
    .expect("Failed to map low memory identity");
    serial_println!(
        "[main] setup_paging: identity mapped {} UEFI regions + LAPIC + low",
        identity_count
    );

    // higher half mapping
    serial_println!("[main] setup_paging: mapping higher half...");
    let mut hh_count = 0u32;
    for entry in mmap.entries() {
        paging::map_higher_half_region(
            &mut pt_manager,
            &mut early_alloc,
            entry.phys_start,
            entry.page_count * (PAGE_SIZE as u64),
        )
        .expect("Failed to map higher half");
        hh_count += 1;
    }
    paging::map_higher_half_region(&mut pt_manager, &mut early_alloc, 0xfee0_0000, 0x1000)
        .expect("Failed to map LAPIC higher half");
    paging::map_higher_half_region(
        &mut pt_manager,
        &mut early_alloc,
        stack_phys_base,
        0x20_0000,
    )
    .expect("Failed to map stack higher half");
    serial_println!(
        "[main] setup_paging: higher half mapped {} UEFI regions + LAPIC + stack",
        hh_count
    );

    unsafe {
        pt_manager.load();
    }
    serial_println!("[main] setup_paging: new CR3 loaded");

    // buddy アロケータにメモリ領域を登録
    serial_println!("[main] setup_paging: initializing buddy allocator...");
    {
        let mut alloc = ALLOCATOR.lock();
        let mut region_count = 0u32;
        for entry in mmap.entries() {
            use ::uefi::mem::memory_map::MemoryType;
            match entry.ty {
                MemoryType::CONVENTIONAL
                | MemoryType::BOOT_SERVICES_CODE
                | MemoryType::BOOT_SERVICES_DATA => {
                    let start = entry.phys_start;
                    let end = start + entry.page_count * (PAGE_SIZE as u64);

                    if start == 0 {
                        serial_println!("[main] setup_paging: skipping region starting at 0");
                        continue;
                    }

                    // カーネル領域をループ内で正しく取得
                    let kernel_base = paging::KERNEL_PHYS_BASE.load(Ordering::Relaxed);
                    let kernel_size = paging::KERNEL_IMAGE_SIZE.load(Ordering::Relaxed);
                    let kernel_end = kernel_base + kernel_size;

                    // 除外領域のリストを作成し、一度だけ呼び出す
                    add_region_excluding(
                        &mut alloc,
                        start,
                        end,
                        &[
                            (stack_phys_base, stack_phys_end),
                            (early_phys_base, early_phys_end),
                            (kernel_base, kernel_end),
                        ],
                    );
                    region_count += 1;
                }
                _ => {}
            }
        }
        serial_println!(
            "[main] setup_paging: buddy registered {} regions",
            region_count
        );
        alloc.dump();
    }

    *ADDRESS_SPACE.lock() = Some(AddressSpace::new(pt_manager));
    serial_println!("[main] setup_paging: AddressSpace initialized");
    serial_println!("[main] setup_paging: done");
}

fn add_region_excluding(alloc: &mut BuddyAllocator, start: u64, end: u64, excludes: &[(u64, u64)]) {
    let mut remaining = [(0u64, 0u64); 8];
    remaining[0] = (start, end);
    let mut count = 1;

    for &(ex_s, ex_e) in excludes {
        let mut next = [(0u64, 0u64); 8];
        let mut next_count = 0;

        for i in 0..count {
            let (r_s, r_e) = remaining[i];
            // 重なり判定
            if r_s < ex_e && r_e > ex_s {
                if r_s < ex_s {
                    next[next_count] = (r_s, ex_s);
                    next_count += 1;
                }
                if r_e > ex_e {
                    next[next_count] = (ex_e, r_e);
                    next_count += 1;
                }
            } else {
                next[next_count] = (r_s, r_e);
                next_count += 1;
            }
        }
        remaining = next;
        count = next_count;
    }

    for i in 0..count {
        let (s, e) = remaining[i];
        if e > s {
            alloc.add_region(allocator::PhysAddr::new(s), (e - s) as usize);
        }
    }
}

fn start_scheduler(image_size: u64, init_bytes: Option<&'static [u8]>) {
    serial_println!("[main] start_scheduler: begin");

    let kernel_pml4 = ADDRESS_SPACE
        .lock()
        .as_ref()
        .unwrap()
        .page_table
        .pml4_phys();
    let image_base_phys = paging::KERNEL_PHYS_BASE.load(Ordering::Relaxed);

    unsafe {
        let current_cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) current_cr3);
        let src_pml4 = crate::paging::phys_to_virt(current_cr3) as *const crate::paging::PageTable;

        let (kstack_phys, kstack_virt, kstack_len) = {
            let mut alloc = ALLOCATOR.lock();
            let phys = alloc.alloc(3).expect("init kstack alloc");
            let virt = paging::phys_to_virt(phys.as_u64()) as *mut u8;
            (phys, virt, 4096usize * 8)
        };
        core::ptr::write_bytes(kstack_virt, 0, kstack_len);

        if let Some(elf_data) = init_bytes {
            serial_println!("[main] loading init ELF: {} bytes", elf_data.len());
            serial_println!("[main] ELF pointer = {}", elf_data.as_ptr() as u64);

            // ELF 用の新しいユーザー PML4 を作成
            let mut pt = {
                let mut alloc = ALLOCATOR.lock();
                paging::PageTableManager::new_user(kernel_pml4, &mut *alloc)
                    .expect("ELF: new_user pt")
            };

            // ELF セグメントをロードする AddressSpace
            // pt と同じ PML4 を指す（from_phys で別インスタンス、同じ物理PML4）
            let mut as_ =
                vma::AddressSpace::new(paging::PageTableManager::from_phys(pt.pml4_phys()));

            let entry = {
                let loader = elf::ElfLoader::new(elf_data).expect("ElfLoader::new");
                let mut alloc = ALLOCATOR.lock();
                loader
                    .load(&mut pt, &mut as_, &mut *alloc)
                    .expect("elf load")
            };

            // カーネルイメージを SUPERVISOR+WRITABLE でユーザー PML4 に追加
            paging::map_kernel_image(&mut as_.page_table, image_base_phys, image_size);

            // カーネル higher-half エントリを同期
            paging::sync_higher_half(as_.page_table.pml4_phys().as_u64(), src_pml4);

            // ELF プロセスを作成（ユーザースタック確保込み）
            let proc = process::Process::new_user_with_address_space(
                1,
                entry,
                kstack_virt,
                kstack_len,
                Some((kstack_phys, 3)),
                0x0000_7fff_0000,
                8,
                &[alloc::string::String::from("/init")],
                &[alloc::string::String::from("PATH=/")],
                as_,
            )
            .expect("ELF: new_user_with_address_space");

            (&raw mut SCHEDULER)
                .as_mut()
                .unwrap()
                .add_process(proc)
                .expect("add ELF proc");
            serial_println!("[main] ELF process registered: entry={:#x}", entry);
            crate::ALLOCATOR
                .lock()
                .check_metadata_integrity("after_elf_process_registered");
        } else {
            serial_println!("[main] no init ELF provided, nothing to run");
        }

        // syscall_entry 用の初期カーネル RSP を設定
        let first_kstack_top = (kstack_virt as u64) + (kstack_len as u64);
        crate::syscall::update_kernel_rsp(first_kstack_top);

        crate::gdt::set_kernel_stack(first_kstack_top);
    }

    // タイマー割り込みからスケジューラを呼び出せるようにする
    crate::interrupts::PREEMPT_SCHED_PTR.store(&raw mut SCHEDULER, Ordering::Relaxed);

    serial_println!("[main] first schedule");
    process::schedule(&raw mut SCHEDULER);

    serial_println!("[main] all processes done, idling");
    loop {
        x86_64::instructions::interrupts::enable();
        x86_64::instructions::hlt();
    }
}

fn get_image_info() -> (u64, u64) {
    use uefi::boot;
    use uefi::proto::loaded_image::LoadedImage;
    let handle = boot::image_handle();
    let loaded_image =
        boot::open_protocol_exclusive::<LoadedImage>(handle).expect("Failed to get LoadedImage");
    let (base, size) = loaded_image.info();
    serial_println!(
        "[main] get_image_info: base={:#x} size={:#x}",
        base as u64,
        size as u64
    );
    (base as u64, size as u64)
}

fn load_file_from_esp(path: &str) -> Option<&'static [u8]> {
    use uefi::boot;
    use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode, RegularFile};
    use uefi::CStr16;

    serial_println!("[main] load_file_from_esp: '{}'", path);

    let mut fs = boot::get_image_file_system(boot::image_handle()).ok()?;
    let mut root = fs.open_volume().ok()?;

    let mut buf = [0u16; 64];
    let mut i = 0;
    for c in path.encode_utf16() {
        if i >= 63 {
            break;
        }
        buf[i] = c;
        i += 1;
    }
    buf[i] = 0;
    let cpath = CStr16::from_u16_with_nul(&buf[..=i]).ok()?;

    let handle = root
        .open(cpath, FileMode::Read, FileAttribute::empty())
        .ok()?;
    let mut file: RegularFile = handle.into_regular_file()?;

    let mut info_buf = [0u8; 512];
    let info = file.get_info::<FileInfo>(&mut info_buf).ok()?;
    let size = info.file_size() as usize;
    serial_println!("[main] load_file_from_esp: size={} bytes", size);

    let ptr = boot::allocate_pool(boot::MemoryType::LOADER_DATA, size).ok()?;
    let slice = unsafe { core::slice::from_raw_parts_mut(ptr.as_ptr(), size) };
    file.read(slice).ok()?;

    serial_println!("[main] load_file_from_esp: loaded at {:p}", ptr);
    Some(unsafe { core::slice::from_raw_parts(ptr.as_ptr(), size) })
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("============================================================");
    serial_println!("KERNEL PANIC: {}", info);
    serial_println!("============================================================");
    loop {}
}

#[unsafe(naked)]
unsafe extern "C" fn jump_to_higher_half() {
    core::arch::naked_asm!(
        "mov rax, [rsp]",
        "add rax, [{virt_base}]",
        "mov [rsp], rax",
        "mov rax, rsp",
        "add rax, [{virt_base}]",
        "mov rsp, rax",
        "mov rax, rbp",
        "add rax, [{virt_base}]",
        "mov rbp, rax",
        "ret",
        virt_base = sym paging::KERNEL_VIRT_BASE_STORAGE,
    );
}
