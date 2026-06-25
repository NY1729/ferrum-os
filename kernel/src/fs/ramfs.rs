#![allow(dead_code)]

use super::vfs::{FileType, Stat, Vfs, VfsNode};
use crate::spinlock::IrqMutex;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

pub struct RamfsNode {
    pub file_type: FileType,
    pub data: Vec<u8>,
}

impl RamfsNode {
    fn stat_inner(&self) -> Stat {
        Stat {
            file_type: self.file_type,
            size: self.data.len(),
        }
    }
    fn read_inner(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if offset >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[offset..];
        let n = buf.len().min(available.len());
        buf[..n].copy_from_slice(&available[..n]);
        Ok(n)
    }
    fn write_inner(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let end = offset + buf.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[offset..end].copy_from_slice(buf);
        Ok(buf.len())
    }
}

pub struct RamfsHandle(Arc<IrqMutex<RamfsNode>>);

impl VfsNode for RamfsHandle {
    fn stat(&self) -> Stat {
        self.0.lock().stat_inner()
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        self.0.lock().read_inner(offset, buf)
    }

    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        self.0.lock().write_inner(offset, buf)
    }

    fn clone_box(&self) -> Box<dyn VfsNode> {
        Box::new(RamfsHandle(self.0.clone()))
    }
}
struct Entry {
    path: String,
    node: Arc<IrqMutex<RamfsNode>>,
}

pub struct Ramfs {
    entries: Vec<Entry>,
}

impl Ramfs {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub fn rename(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let entry = self
            .entries
            .iter_mut()
            .find(|e| e.path == old)
            .ok_or("not found")?;
        entry.path = String::from(new);
        Ok(())
    }
}

impl Vfs for Ramfs {
    fn open(&self, path: &str) -> Option<Box<dyn VfsNode>> {
        let entry = self.entries.iter().find(|e| e.path == path)?;
        Some(Box::new(RamfsHandle(entry.node.clone())))
    }
    fn create(&mut self, path: &str, file_type: FileType) -> Result<(), &'static str> {
        if self.entries.iter().any(|e| e.path == path) {
            return Err("file already exists");
        }
        self.entries.push(Entry {
            path: String::from(path),
            node: Arc::new(IrqMutex::new(RamfsNode {
                file_type,
                data: Vec::new(),
            })),
        });
        Ok(())
    }
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.node.lock().data = data.to_vec();
            return Ok(());
        }
        self.entries.push(Entry {
            path: String::from(path),
            node: Arc::new(IrqMutex::new(RamfsNode {
                file_type: FileType::Regular,
                data: data.to_vec(),
            })),
        });
        Ok(())
    }

    fn list_dir(&self, dir: &str) -> Vec<String> {
        // 既存のメソッドをトレイト経由で呼べるようにする
        // （元々 inherent method だった list_dir をトレイトに移す）
        let prefix = if dir == "/" {
            String::from("/")
        } else {
            alloc::format!("{}/", dir)
        };
        let mut names = Vec::new();
        for e in &self.entries {
            if let Some(rest) = e.path.strip_prefix(prefix.as_str()) {
                if !rest.is_empty() && !rest.contains('/') {
                    names.push(String::from(rest));
                }
            }
        }
        names
    }

    fn mkdir(&mut self, path: &str) -> Result<(), &'static str> {
        // 既に同パスのエントリがあればOK
        if self.entries.iter().any(|e| e.path == path) {
            return Ok(());
        }
        // ディレクトリエントリとして空ファイルを登録
        self.entries.push(Entry {
            path: String::from(path),
            node: Arc::new(IrqMutex::new(RamfsNode {
                file_type: FileType::Directory,
                data: Vec::new(),
            })),
        });
        Ok(())
    }
    fn unlink(&mut self, path: &str) -> Result<(), &'static str> {
        let pos = self
            .entries
            .iter()
            .position(|e| e.path == path)
            .ok_or("not found")?;
        self.entries.remove(pos);
        Ok(())
    }
}
