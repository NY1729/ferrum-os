#![allow(dead_code)]

use alloc::boxed::Box;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FileType {
    Regular,
    Directory,
}

#[derive(Debug, Clone, Copy)]
pub struct Stat {
    pub file_type: FileType,
    pub size: usize,
}

pub trait VfsNode: Send + Sync {
    fn stat(&self) -> Stat;
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str>;
    fn read_all(&self) -> alloc::vec::Vec<u8> {
        let size = self.stat().size;
        let mut buf = alloc::vec![0u8; size];
        let _ = self.read(0, &mut buf);
        buf
    }
    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str>;
    fn clone_box(&self) -> Box<dyn VfsNode>;
}

pub trait Vfs: Send + Sync {
    fn open(&self, path: &str) -> Option<alloc::boxed::Box<dyn VfsNode>>;
    fn create(&mut self, path: &str, file_type: FileType) -> Result<(), &'static str>;
    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), &'static str>;
    fn list_dir(&self, dir: &str) -> alloc::vec::Vec<alloc::string::String>;

    // initramfs展開用
    fn mkdir(&mut self, path: &str) -> Result<(), &'static str>;
    fn symlink(&mut self, path: &str, target: &str) -> Result<(), &'static str> {
        // デフォルト実装: 無視（RAMFSがsymlink未対応の場合）
        let _ = (path, target);
        Ok(())
    }
    fn unlink(&mut self, path: &str) -> Result<(), &'static str>;
}
