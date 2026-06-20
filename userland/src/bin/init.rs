#![no_std]
#![no_main]

use libuser::syscall;

#[unsafe(no_mangle)]
pub extern "C" fn user_main(_argc: i64, _argv: *const *const u8) -> ! {
    loop {
        syscall::write(1, b"$ ");

        let mut buf = [0u8; 256];
        let n = syscall::read(0, &mut buf);
        if n <= 0 {
            continue;
        }

        let line = trim(&buf[..n as usize]);
        if line.is_empty() {
            continue;
        }

        // 空白区切りでトークン化
        const MAX_ARGS: usize = 8;
        const ARG_BUF_LEN: usize = 64;
        let mut arg_bufs = [[0u8; ARG_BUF_LEN]; MAX_ARGS];
        let mut arg_lens = [0usize; MAX_ARGS];
        let mut argc = 0usize;

        for token in line.split(|&b| b == b' ') {
            if token.is_empty() {
                continue;
            }
            if argc >= MAX_ARGS {
                break;
            }
            let len = token.len().min(ARG_BUF_LEN - 1);
            arg_bufs[argc][..len].copy_from_slice(&token[..len]);
            arg_bufs[argc][len] = 0; // NUL終端
            arg_lens[argc] = len + 1; // NUL込みの長さ
            argc += 1;
        }

        if argc == 0 {
            continue;
        }

        let pid = syscall::fork();
        if pid == 0 {
            let argv_slices: [&[u8]; MAX_ARGS] = core::array::from_fn(|i| {
                if i < argc {
                    &arg_bufs[i][..arg_lens[i]]
                } else {
                    &[]
                }
            });
            let r = syscall::execve(&arg_bufs[0][..arg_lens[0]], &argv_slices[..argc]);
            syscall::write(1, b"command not found\n");
            let _ = r;
            syscall::exit(1)
        } else if pid > 0 {
            let mut status: i32 = 0;
            syscall::wait4(-1, &mut status as *mut i32);
        } else {
            syscall::write(1, b"fork failed\n");
        }
    }
}

fn trim(buf: &[u8]) -> &[u8] {
    let mut end = buf.len();
    while end > 0 && matches!(buf[end - 1], b'\r' | b'\n' | b'\0' | b' ') {
        end -= 1;
    }
    &buf[..end]
}
