#![allow(dead_code)]

use crate::process::Pid;
use crate::spinlock::IrqMutex;
use alloc::vec::Vec;

#[derive(Debug, Clone, Copy)]
pub struct PageOwner {
    pub tag: &'static str,
    pub pid: Pid,
    pub order: usize,
}

pub static PAGE_OWNERS: IrqMutex<Vec<(u64, PageOwner)>> = IrqMutex::new(Vec::new());

const RESERVE_CAPACITY: usize = 8192;

pub fn init() {
    PAGE_OWNERS.lock().reserve(RESERVE_CAPACITY);
}

pub fn track(phys: u64, order: usize, tag: &'static str, pid: Pid) {
    let mut owners = PAGE_OWNERS.lock();
    if owners.len() >= owners.capacity() {
        panic!("[page_owner] capacity exhausted ({} entries)", owners.capacity());
    }
    owners.push((phys, PageOwner { tag, pid, order }));
}

pub fn untrack(phys: u64) -> Option<PageOwner> {
    let mut owners = PAGE_OWNERS.lock();
    let idx = owners.iter().position(|(addr, _)| *addr == phys)?;
    Some(owners.swap_remove(idx).1)
}

/// 用途タグごとの集計。ALLOCATOR.lock() を握っていない安全な文脈でのみ呼ぶこと
pub fn dump() {
    let owners = PAGE_OWNERS.lock();
    let mut by_tag: alloc::collections::BTreeMap<
        &'static str,
        (usize, usize)
    > = alloc::collections::BTreeMap::new();
    for (_, owner) in owners.iter() {
        let e = by_tag.entry(owner.tag).or_insert((0, 0));
        e.0 += 1;
        e.1 += 1usize << owner.order;
    }
    crate::serial_println!(
        "[page_owner] {} tracked blocks (capacity={}):",
        owners.len(),
        owners.capacity()
    );
    for (tag, (blocks, pages)) in by_tag.iter() {
        crate::serial_println!(
            "[page_owner]   {:<16} blocks={:4} pages={:5} ({} KB)",
            tag,
            blocks,
            pages,
            pages * 4
        );
    }
}

pub fn owned_by(pid: Pid) -> Vec<(u64, PageOwner)> {
    PAGE_OWNERS.lock()
        .iter()
        .filter(|(_, o)| o.pid == pid)
        .copied()
        .collect()
}
