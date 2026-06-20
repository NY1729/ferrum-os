#![no_std]
#![no_main]

use libuser::syscall;

#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

fn write_num(n: i64) {
    if n == 0 {
        syscall::write(1, b"0");
        return;
    }
    let neg = n < 0;
    let mut n = if neg { -n } else { n };
    let mut buf = [0u8; 20];
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if neg {
        syscall::write(1, b"-");
    }
    syscall::write(1, &buf[i..]);
}

#[unsafe(no_mangle)]
pub extern "C" fn user_main(_argc: i64, _argv: *const *const u8) -> ! {
    // /dev/null: 書き込みが捨てられて成功するか
    let fd_null = syscall::open(b"/dev/null\0", syscall::O_WRONLY, 0);
    let n = syscall::write(fd_null as u64, b"this goes nowhere\n");
    if n == 18 {
        syscall::write(1, b"dev_test: /dev/null write OK\n");
    } else {
        syscall::write(1, b"dev_test: /dev/null write FAILED\n");
    }

    // /dev/null に対する ioctl(TIOCGWINSZ): ENOTTYで失敗するはず
    let mut ws = Winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let r_null = syscall::ioctl(
        fd_null as u64,
        syscall::TIOCGWINSZ,
        &mut ws as *mut Winsize as u64,
    );
    if r_null < 0 {
        syscall::write(
            1,
            b"dev_test: ioctl(/dev/null, TIOCGWINSZ) correctly FAILED ret=",
        );
        write_num(r_null);
        syscall::write(1, b"\n");
    } else {
        syscall::write(
            1,
            b"dev_test: ioctl(/dev/null, TIOCGWINSZ) unexpectedly SUCCEEDED (bug)\n",
        );
    }
    syscall::close(fd_null as u64);

    // /dev/zero: 読み出すと全部ゼロになっているか
    let fd_zero = syscall::open(b"/dev/zero\0", syscall::O_RDONLY, 0);
    let mut buf = [0xFFu8; 16];
    let n = syscall::read(fd_zero as u64, &mut buf);
    syscall::close(fd_zero as u64);
    if n == 16 && buf.iter().all(|&b| b == 0) {
        syscall::write(1, b"dev_test: /dev/zero read OK\n");
    } else {
        syscall::write(1, b"dev_test: /dev/zero read FAILED\n");
    }

    // /dev/tty: 書き込みが画面に出るか(目視確認)
    let fd_tty = syscall::open(b"/dev/tty\0", syscall::O_WRONLY, 0);
    syscall::write(fd_tty as u64, b"dev_test: hello via /dev/tty\n");

    // /dev/tty に対する ioctl(TIOCGWINSZ): 成功して値が返るはず
    let r_tty = syscall::ioctl(
        fd_tty as u64,
        syscall::TIOCGWINSZ,
        &mut ws as *mut Winsize as u64,
    );
    if r_tty == 0 {
        syscall::write(1, b"dev_test: ioctl(/dev/tty, TIOCGWINSZ) OK row=");
        write_num(ws.ws_row as i64);
        syscall::write(1, b" col=");
        write_num(ws.ws_col as i64);
        syscall::write(1, b"\n");
    } else {
        syscall::write(1, b"dev_test: ioctl(/dev/tty, TIOCGWINSZ) FAILED ret=");
        write_num(r_tty);
        syscall::write(1, b"\n");
    }
    syscall::close(fd_tty as u64);

    syscall::exit(0)
}
