#![allow(dead_code)]

use crate::fs::vfs::VfsNode;
use crate::spinlock::IrqMutex;
use alloc::boxed::Box;
use alloc::sync::Arc;

pub const FD_MAX: usize = 16;

#[derive(Clone, Copy)]
pub enum FdKind {
    Serial,
    File,
    DevNull,
    DevZero,
    Directory,
}
pub struct OpenFile {
    pub kind: FdKind,
    pub readable: bool,
    pub writable: bool,
    pub append: bool,
    pub node: Option<Box<dyn VfsNode>>,
    pub offset: usize,
    pub dir_entries: Option<alloc::vec::Vec<alloc::string::String>>,
}

impl OpenFile {
    pub fn stdin() -> Self {
        Self {
            kind: FdKind::Serial,
            readable: true,
            writable: true,
            append: false,
            node: None,
            offset: 0,
            dir_entries: None,
        }
    }
    pub fn stdout() -> Self {
        Self {
            kind: FdKind::Serial,
            readable: true,
            writable: true,
            append: false,
            node: None,
            offset: 0,
            dir_entries: None,
        }
    }
    pub fn file(node: Box<dyn VfsNode>, readable: bool, writable: bool, append: bool) -> Self {
        Self {
            kind: FdKind::File,
            readable,
            writable,
            append,
            node: Some(node),
            offset: 0,
            dir_entries: None,
        }
    }
    pub fn device(kind: FdKind) -> Self {
        Self {
            kind,
            readable: true,
            writable: true,
            append: false,
            node: None,
            offset: 0,
            dir_entries: None,
        }
    }
    pub fn directory(entries: alloc::vec::Vec<alloc::string::String>) -> Self {
        Self {
            kind: FdKind::Directory,
            readable: true,
            writable: false,
            append: false,
            node: None,
            offset: 0,
            dir_entries: Some(entries),
        }
    }
}

pub type FdEntry = Arc<IrqMutex<OpenFile>>;

pub struct FdTable {
    entries: [Option<FdEntry>; FD_MAX],
}

impl FdTable {
    pub fn new_stdio() -> Self {
        let mut t = Self {
            entries: core::array::from_fn(|_| None),
        };
        t.entries[0] = Some(Arc::new(IrqMutex::new(OpenFile::stdin())));
        t.entries[1] = Some(Arc::new(IrqMutex::new(OpenFile::stdout())));
        t.entries[2] = Some(Arc::new(IrqMutex::new(OpenFile::stdout())));
        t
    }
    pub fn get(&self, fd: usize) -> Option<FdEntry> {
        self.entries.get(fd)?.clone()
    }
    pub fn alloc(&mut self, file: OpenFile) -> Option<usize> {
        let (i, slot) = self
            .entries
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.is_none())?;
        *slot = Some(Arc::new(IrqMutex::new(file)));
        Some(i)
    }
    pub fn close(&mut self, fd: usize) -> bool {
        match self.entries.get_mut(fd) {
            Some(slot @ Some(_)) => {
                *slot = None;
                true
            }
            _ => false,
        }
    }
    pub fn fork_clone(&self) -> Self {
        let mut t = Self {
            entries: core::array::from_fn(|_| None),
        };
        for (i, slot) in self.entries.iter().enumerate() {
            t.entries[i] = slot.clone();
        }
        t
    }
    pub fn alloc_entry(&mut self, entry: FdEntry) -> Option<usize> {
        let (i, slot) = self
            .entries
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.is_none())?;
        *slot = Some(entry);
        Some(i)
    }
    pub fn alloc_from(&mut self, entry: FdEntry, minfd: usize) -> Option<usize> {
        let (i, slot) = self
            .entries
            .iter_mut()
            .enumerate()
            .skip(minfd)
            .find(|(_, s)| s.is_none())?;
        *slot = Some(entry);
        Some(i)
    }

    pub fn put(&mut self, fd: usize, entry: FdEntry) {
        if fd < FD_MAX {
            self.entries[fd] = Some(entry);
        }
    }
}
