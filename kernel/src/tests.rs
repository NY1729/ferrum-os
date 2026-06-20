use crate::allocator::PAGE_SIZE;
use crate::process;
use crate::serial_println;
use crate::vma::{VMAFlags, VMAKind};
use crate::ADDRESS_SPACE;

macro_rules! run_test {
    ($name:expr, $f:expr) => {{
        serial_println!("test: {}", $name);

        // 前提条件: 割り込みは禁止されているべき
        let irq_before = x86_64::instructions::interrupts::are_enabled();
        if irq_before {
            serial_println!(
                "  WARN: interrupts were enabled before '{}' — disabling",
                $name
            );
            x86_64::instructions::interrupts::disable();
        }

        $f();

        // 事後条件: テスト後も割り込みは禁止されているべき
        let irq_after = x86_64::instructions::interrupts::are_enabled();
        if irq_after {
            serial_println!("  WARN: '{}' left interrupts enabled — disabling", $name);
            x86_64::instructions::interrupts::disable();
        }

        serial_println!("  {}: OK", $name);
    }};
}
macro_rules! test_eq {
    ($test:expr, $left:expr, $right:expr) => {{
        let left = $left;
        let right = $right;
        if left != right {
            panic!(
                "[{}] assertion failed: left={:?} right={:?}",
                $test, left, right
            );
        }
    }};
}

macro_rules! test_assert {
    ($test:expr, $cond:expr, $msg:expr) => {{
        if !$cond {
            panic!("[{}] assertion failed: {}", $test, $msg);
        }
    }};
}

pub fn run_all() {
    serial_println!("=== Running kernel tests ===");

    run_test!("heap_allocator", || heap_allocator());
    run_test!("preemptive_scheduling", || preemptive_scheduling());
    run_test!("demand_paging", || demand_paging());
    run_test!("isolated_address_spaces", || isolated_address_spaces());

    serial_println!("=== All tests passed ===");
}

fn heap_allocator() {
    use alloc::boxed::Box;
    use alloc::string::String;
    use alloc::vec::Vec;

    // Box: 基本的なアロケーション・解放
    let b = Box::new(0xdeadbeefu64);
    test_eq!("heap/box", *b, 0xdeadbeef);
    drop(b);

    // Vec: 小さなオブジェクトのスラブアロケーション
    let mut v: Vec<u64> = Vec::new();
    for i in 0..64u64 {
        v.push(i);
    }
    test_eq!("heap/vec_small/len", v.len(), 64);
    for i in 0..64u64 {
        test_eq!("heap/vec_small/val", v[i as usize], i);
    }
    drop(v);

    // Vec: バディアロケータからの大きなアロケーション + realloc
    let mut big: Vec<u8> = Vec::with_capacity(8192);
    for i in 0..8192u16 {
        big.push(i as u8);
    }
    test_eq!("heap/vec_big/len", big.len(), 8192);
    for i in 0..8192usize {
        test_eq!("heap/vec_big/val", big[i], (i & 0xff) as u8);
    }
    drop(big);

    // String: heap上の文字列
    let s = String::from("Ferrum OS");
    test_eq!("heap/string", s.as_str(), "Ferrum OS");
    drop(s);

    // alloc後のfreeが正しくフリーリストに戻るか確認
    // 同じサイズを2回アロケートして異なるアドレスが返ることを確認
    let a1 = Box::new(0u64);
    let a2 = Box::new(1u64);
    let p1 = &*a1 as *const u64;
    let p2 = &*a2 as *const u64;
    test_assert!(
        "heap/distinct_ptrs",
        p1 != p2,
        "same pointer returned twice"
    );
    drop(a1);
    drop(a2);
}

fn preemptive_scheduling() {
    use core::sync::atomic::{AtomicU64, Ordering};

    static COUNT_A: AtomicU64 = AtomicU64::new(0);
    static COUNT_B: AtomicU64 = AtomicU64::new(0);
    static mut SCHED_STACK_A: [u8; 32768] = [0; 32768];
    static mut SCHED_STACK_B: [u8; 32768] = [0; 32768];
    static mut TEST_SCHED: process::Scheduler = process::Scheduler::new();

    #[unsafe(naked)]
    extern "C" fn proc_a() -> ! {
        core::arch::naked_asm!(
            "sti",
            "2:", "call {count}", "hlt", "jmp 2b",
            count = sym count_a,
        );
    }

    #[unsafe(naked)]
    extern "C" fn proc_b() -> ! {
        core::arch::naked_asm!(
            "sti",
            "2:", "call {count}", "hlt", "jmp 2b",
            count = sym count_b,
        );
    }

    extern "C" fn count_a() {
        let n = COUNT_A.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= 100 {
            unsafe {
                if let Some(p) = (&raw mut TEST_SCHED).as_mut().unwrap().current_mut() {
                    p.state = process::ProcessState::Dead;
                }
            }
        }
    }

    extern "C" fn count_b() {
        let n = COUNT_B.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= 100 {
            unsafe {
                if let Some(p) = (&raw mut TEST_SCHED).as_mut().unwrap().current_mut() {
                    p.state = process::ProcessState::Dead;
                }
            }
        }
    }

    serial_println!("  [1] getting kernel_pml4");
    let kernel_pml4 = ADDRESS_SPACE
        .lock()
        .as_ref()
        .unwrap()
        .page_table
        .pml4_phys();

    serial_println!("  [2] resetting counters and scheduler");
    COUNT_A.store(0, Ordering::Relaxed);
    COUNT_B.store(0, Ordering::Relaxed);
    unsafe {
        TEST_SCHED = process::Scheduler::new();
    }

    serial_println!("  [3] creating processes");
    unsafe {
        let pa = process::Process::new_with_static_stack(
            10,
            proc_a as *const () as u64,
            kernel_pml4,
            &raw mut SCHED_STACK_A as *mut u8,
            32768,
        );
        let pb = process::Process::new_with_static_stack(
            11,
            proc_b as *const () as u64,
            kernel_pml4,
            &raw mut SCHED_STACK_B as *mut u8,
            32768,
        );
        (&raw mut TEST_SCHED)
            .as_mut()
            .unwrap()
            .add_process(pa)
            .expect("add proc_a failed");
        (&raw mut TEST_SCHED)
            .as_mut()
            .unwrap()
            .add_process(pb)
            .expect("add proc_b failed");
    }

    // ここを追加：schedule より前に PREEMPT_SCHED_PTR を TEST_SCHED へ向ける
    crate::interrupts::PREEMPT_SCHED_PTR.store(&raw mut TEST_SCHED, Ordering::Relaxed);

    serial_println!("  [4] first schedule call");

    // 最初のスイッチ
    process::schedule(&raw mut TEST_SCHED);

    serial_println!("  [5] entering idle loop");

    // 全プロセスが Dead になるまで hlt で待ちながら schedule を回す
    loop {
        let all_done = unsafe {
            (&raw const TEST_SCHED)
                .as_ref()
                .unwrap()
                .processes
                .iter()
                .all(|p| {
                    p.as_ref()
                        .map_or(true, |p| p.state == process::ProcessState::Dead)
                })
        };
        if all_done {
            break;
        }
        x86_64::instructions::interrupts::enable();
        x86_64::instructions::hlt();
        x86_64::instructions::interrupts::disable();
        process::schedule(&raw mut TEST_SCHED);
    }

    serial_println!("  [6] all processes done");
    x86_64::instructions::interrupts::disable();

    // スケジューラポインタを元に戻す（不要になったが念のため）
    crate::interrupts::PREEMPT_SCHED_PTR.store(&raw mut crate::SCHEDULER, Ordering::Relaxed);

    let a = COUNT_A.load(Ordering::Relaxed);
    let b = COUNT_B.load(Ordering::Relaxed);
    test_assert!(
        "sched/count_a",
        a >= 100,
        alloc::format!("proc_a count={} should be >= 100", a).as_str()
    );
    test_assert!(
        "sched/count_b",
        b >= 100,
        alloc::format!("proc_b count={} should be >= 100", b).as_str()
    );

    serial_println!("    A={} B={}", a, b);
}

fn demand_paging() {
    const BASE: u64 = 0x0600_0000_0000;
    const PAGES: u64 = 3;

    {
        let mut as_ = ADDRESS_SPACE.lock();
        let as_ = as_.as_mut().unwrap();
        as_.mmap(
            BASE,
            PAGE_SIZE * PAGES as usize,
            VMAFlags::rw(),
            VMAKind::Anonymous,
        )
        .expect("mmap failed");
    }

    // 書き込み（ページフォルトを誘発）
    unsafe {
        for i in 0..PAGES {
            let ptr = (BASE + i * PAGE_SIZE as u64) as *mut u64;
            core::ptr::write_volatile(ptr, 0xcafe_0000 + i);
        }
    }

    // 読み戻し検証
    unsafe {
        for i in 0..PAGES {
            let ptr = (BASE + i * PAGE_SIZE as u64) as *const u64;
            let val = core::ptr::read_volatile(ptr);
            test_eq!(
                alloc::format!("demand_paging/page{}", i).as_str(),
                val,
                0xcafe_0000 + i
            );
        }
    }
}

// 独立アドレス空間テスト
fn isolated_address_spaces() {
    use crate::paging::{phys_to_virt, PageAllocator, PageFlags, PageTableManager};

    const UTEST_VIRT: u64 = 0x0000_4000_0000;
    const VAL_A: u64 = 0xAAAA_0000_AAAA_0000;
    const VAL_B: u64 = 0xBBBB_0000_BBBB_0000;

    let kernel_pml4 = ADDRESS_SPACE
        .lock()
        .as_ref()
        .unwrap()
        .page_table
        .pml4_phys();

    let page_flags = PageFlags::PRESENT | PageFlags::WRITABLE | PageFlags::USER;

    // プロセスA: 独立PML4 + UTEST_VIRT にページをマップ
    let (pml4_a, phys_a) = {
        let mut alloc = crate::ALLOCATOR.lock();
        let mut pt =
            PageTableManager::new_user(kernel_pml4, &mut *alloc).expect("new_user A failed");
        let (_, phys) = alloc.alloc_page().expect("alloc page A failed");
        pt.map(UTEST_VIRT, phys, page_flags, &mut *alloc)
            .expect("map A failed");
        (pt.pml4_phys(), phys)
    };

    // プロセスB: 独立PML4 + 同じ UTEST_VIRT に別ページをマップ
    let (pml4_b, phys_b) = {
        let mut alloc = crate::ALLOCATOR.lock();
        let mut pt =
            PageTableManager::new_user(kernel_pml4, &mut *alloc).expect("new_user B failed");
        let (_, phys) = alloc.alloc_page().expect("alloc page B failed");
        pt.map(UTEST_VIRT, phys, page_flags, &mut *alloc)
            .expect("map B failed");
        (pt.pml4_phys(), phys)
    };

    serial_println!(
        "[isolated_as] pml4_a={:#x} phys_a={:#x}  pml4_b={:#x} phys_b={:#x}",
        pml4_a.as_u64(),
        phys_a.as_u64(),
        pml4_b.as_u64(),
        phys_b.as_u64(),
    );

    // 物理ページが別であることを確認
    test_assert!(
        "isolated_address_spaces/distinct_phys",
        phys_a != phys_b,
        "phys_a == phys_b!"
    );

    // pml4_a の PML4 エントリを確認（デバッグ）
    unsafe {
        // PML4 を higher-half 経由で読む
        let pml4_a_ptr = phys_to_virt(pml4_a.as_u64()) as *const u64;
        let pml4_k_ptr = phys_to_virt(kernel_pml4.as_u64()) as *const u64;

        // UTEST_VIRT の PML4 index = 0
        let a_pml4_0 = *pml4_a_ptr.add(0);
        // higher-half の PML4 index = 256
        let a_pml4_256 = *pml4_a_ptr.add(256);
        let k_pml4_256 = *pml4_k_ptr.add(256);

        serial_println!("[isolated_as] pml4_a[0]={:#x} (UTEST_VIRT entry)", a_pml4_0);
        serial_println!(
            "[isolated_as] pml4_a[256]={:#x} kernel[256]={:#x} (higher-half, should match)",
            a_pml4_256,
            k_pml4_256
        );
    }

    // higher-half 経由で物理ページに書き込む（CR3切り替え不要）
    unsafe {
        let va = phys_to_virt(phys_a.as_u64()) as *mut u64;
        let vb = phys_to_virt(phys_b.as_u64()) as *mut u64;
        core::ptr::write_volatile(va, VAL_A);
        core::ptr::write_volatile(vb, VAL_B);
    }
    serial_println!("[isolated_as] wrote values via higher-half");

    let read_a: u64;
    let read_b: u64;
    unsafe {
        core::arch::asm!(
            // CR3 = pml4_a
            "mov cr3, {cr3_a}",
            // UTEST_VIRT を読む
            "mov {out_a}, [{utest}]",
            // CR3 = pml4_b
            "mov cr3, {cr3_b}",
            // 同じ UTEST_VIRT を読む
            "mov {out_b}, [{utest}]",
            // カーネル CR3 に戻す
            "mov cr3, {cr3_k}",
            cr3_a = in(reg) pml4_a.as_u64(),
            cr3_b = in(reg) pml4_b.as_u64(),
            cr3_k = in(reg) kernel_pml4.as_u64(),
            utest = in(reg) UTEST_VIRT,
            out_a = out(reg) read_a,
            out_b = out(reg) read_b,
            out("dx") _, out("al") _,
            options(nostack),
        );
    }
    serial_println!(
        "[isolated_as] pml4_a: read={:#x} expected={:#x} {}",
        read_a,
        VAL_A,
        if read_a == VAL_A { "OK" } else { "MISMATCH" }
    );
    serial_println!(
        "[isolated_as] pml4_b: read={:#x} expected={:#x} {}",
        read_b,
        VAL_B,
        if read_b == VAL_B { "OK" } else { "MISMATCH" }
    );
    serial_println!("[isolated_as] restored kernel CR3");

    test_eq!("isolated_address_spaces/read_a", read_a, VAL_A);
    test_eq!("isolated_address_spaces/read_b", read_b, VAL_B);
    test_assert!(
        "isolated_address_spaces/independent",
        read_a != read_b,
        "read_a == read_b: address spaces are NOT isolated!"
    );
}
