#![no_std]
#![no_main]

use libuser::syscall;

// C言語風のNUL終端文字列の長さを測るヘルパー関数
fn strlen(s: *const u8) -> usize {
    let mut len = 0;
    unsafe {
        while *s.add(len) != 0 {
            len += 1;
        }
    }
    len
}

#[unsafe(no_mangle)]
pub extern "C" fn user_main(argc: i64, argv: *const *const u8) -> ! {
    if argc < 2 {
        syscall::write(1, b"Usage: cat <file>...\n");
        syscall::exit(1);
    }

    // argv[1] から順番に処理 (argv[0] は "cat" 自身)
    for i in 1..argc as isize {
        let arg_ptr = unsafe { *argv.offset(i) };
        let len = strlen(arg_ptr);

        // システムコールに渡すためのスライスを作成
        let path = unsafe { core::slice::from_raw_parts(arg_ptr, len) };

        let fd = syscall::open(path, syscall::O_RDONLY, 0);
        if fd < 0 {
            syscall::write(1, b"cat: open failed\n");
            continue; // 失敗しても次のファイルの読み込みへ進む
        }

        // バッファを大きくしてループで最後まで読み切る
        let mut buf = [0u8; 1024];
        loop {
            let n = syscall::read(fd as u64, &mut buf);
            if n <= 0 {
                break; // 0ならEOF（ファイル終端）、負ならエラー
            }
            syscall::write(1, &buf[..n as usize]);
        }

        syscall::close(fd as u64);
    }

    syscall::exit(0)
}
