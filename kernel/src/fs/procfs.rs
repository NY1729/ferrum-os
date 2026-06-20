#![allow(dead_code)]

use crate::fs::vfs::{FileType, Stat, VfsNode};
use crate::process::{Process, ProcessState};
use crate::vma::VMAKind;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub struct ProcfsNode {
    data: Vec<u8>,
}

impl ProcfsNode {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl VfsNode for ProcfsNode {
    fn stat(&self) -> Stat {
        Stat {
            file_type: FileType::Regular,
            size: self.data.len(),
        }
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if offset >= self.data.len() {
            return Ok(0);
        }
        let avail = &self.data[offset..];
        let n = core::cmp::min(avail.len(), buf.len());
        buf[..n].copy_from_slice(&avail[..n]);
        Ok(n)
    }

    fn write(&mut self, _offset: usize, _buf: &[u8]) -> Result<usize, &'static str> {
        Err("procfs is read-only")
    }

    fn clone_box(&self) -> Box<dyn VfsNode> {
        Box::new(ProcfsNode {
            data: self.data.clone(),
        })
    }
}

pub fn format_maps(p: &Process) -> String {
    let mut entries: Vec<crate::vma::VMA> =
        p.address_space.vmas.iter().flatten().copied().collect();
    entries.sort_by_key(|v| v.start);

    let mut out = String::new();
    for vma in entries {
        let r = if vma.flags.read { 'r' } else { '-' };
        let w = if vma.flags.write { 'w' } else { '-' };
        let x = if vma.flags.exec { 'x' } else { '-' };
        let label = match vma.kind {
            VMAKind::Stack => "[stack]",
            VMAKind::Heap => "[heap]",
            VMAKind::Anonymous => "",
        };
        out.push_str(&format!(
            "{:016x}-{:016x} {}{}{}p 00000000 00:00 0          {}\n",
            vma.start, vma.end, r, w, x, label
        ));
    }
    out
}

pub fn format_status(p: &Process) -> String {
    let state_str = match p.state {
        ProcessState::Running | ProcessState::Ready => "R (running)",
        ProcessState::Blocked => "S (sleeping)",
        ProcessState::Dead => "Z (zombie)",
    };

    let heap_kb = p.brk_current.saturating_sub(p.brk_start) / 1024;
    let mapped_kb: u64 = p
        .address_space
        .vmas
        .iter()
        .flatten()
        .map(|v| (v.end - v.start) / 1024)
        .sum();

    format!(
        "Name:\tprocess\nPid:\t{}\nPPid:\t{}\nState:\t{}\nThreads:\t1\nVmSize:\t{} kB\nVmHeap:\t{} kB\n",
        p.pid, p.parent_pid, state_str, mapped_kb, heap_kb
    )
}
