#![no_std]
#![no_main]

use libuser::syscall;

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
    for i in 1..argc as isize {
        let arg_ptr = unsafe { *argv.offset(i) };
        let len = strlen(arg_ptr);
        let arg_slice = unsafe { core::slice::from_raw_parts(arg_ptr, len) };

        syscall::write(1, arg_slice);

        // 最後の引数でなければスペースを入れる
        if i < argc as isize - 1 {
            syscall::write(1, b" ");
        }
    }

    // 最後に改行
    syscall::write(1, b"\n");
    syscall::exit(0)
}
