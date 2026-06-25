use crate::spinlock::SpinMutex;
use alloc::collections::VecDeque;
use alloc::sync::Arc;

const PIPE_BUF_SIZE: usize = 4096;

pub struct PipeInner {
    pub buf: VecDeque<u8>,
    pub write_ends: usize,
    pub read_ends: usize,
}

impl PipeInner {
    pub fn new() -> Self {
        Self {
            buf: VecDeque::with_capacity(PIPE_BUF_SIZE),
            write_ends: 1,
            read_ends: 1,
        }
    }
}

pub type PipeRef = Arc<SpinMutex<PipeInner>>;

pub fn new_pipe() -> (PipeRef, PipeRef) {
    let inner = Arc::new(SpinMutex::new(PipeInner::new()));
    (inner.clone(), inner)
}
