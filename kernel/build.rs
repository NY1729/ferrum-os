use std::io::Write;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/ap_boot.s");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let obj = format!("{}/ap_boot.o", out_dir);
    let bin = format!("{}/ap_boot.bin", out_dir);

    // アセンブル
    let status = Command::new("as")
        .args(&["--64", "-o", &obj, "src/ap_boot.s"])
        .status()
        .expect("Failed to run `as`");
    assert!(status.success(), "as failed");

    // nm を1回だけ呼んで全シンボルを取得
    let nm_out = Command::new("nm")
        .args(&["--radix=x", &obj])
        .output()
        .expect("Failed to run nm");
    let nm_stdout = String::from_utf8_lossy(&nm_out.stdout);

    let symbols = [
        "ap_trampoline_start",
        "ap_gdt_ptr",
        "ap_idt_ptr",
        "ap_pml4_addr",
        "ap_stack_ptr",
        "ap_main_ptr",
    ];

    let mut offsets = std::collections::HashMap::new();
    for line in nm_stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[2];
        if symbols.contains(&name) {
            let addr = u64::from_str_radix(parts[0], 16).unwrap();
            offsets.insert(name.to_string(), addr);
        }
    }

    // 全シンボルが見つかったか確認
    for sym in &symbols {
        assert!(offsets.contains_key(*sym), "Symbol not found: {}", sym);
    }

    let base = offsets["ap_trampoline_start"];

    // オフセット定数をRustファイルに出力
    let consts_path = format!("{}/ap_offsets.rs", out_dir);
    let mut f = std::fs::File::create(&consts_path).unwrap();
    for sym in &[
        "ap_gdt_ptr",
        "ap_idt_ptr",
        "ap_pml4_addr",
        "ap_stack_ptr",
        "ap_main_ptr",
    ] {
        let offset = offsets[*sym] - base;
        let name = sym.to_uppercase();
        writeln!(f, "pub const {}_OFFSET: usize = {:#x};", name, offset).unwrap();
    }
    println!("cargo:rustc-env=AP_OFFSETS_RS={}", consts_path);

    // フラットバイナリに変換
    let status = Command::new("objcopy")
        .args(&["-O", "binary", "--only-section=.ap_boot", &obj, &bin])
        .status()
        .expect("Failed to run `objcopy`");
    assert!(status.success(), "objcopy failed");

    println!("cargo:rustc-env=AP_BOOT_BIN={}", bin);
}
