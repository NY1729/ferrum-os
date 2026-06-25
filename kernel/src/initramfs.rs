/// newc フォーマット（SVR4, マジック "070701"）の cpio アーカイブを
/// インプレースで解析し、RAMFSに展開するモジュール。
///
/// ビルドシステム側では以下で生成する:
///   find initramfs_root | cpio -o --format=newc > initramfs.cpio
///
/// カーネル側では load_file_from_esp("\\initramfs.cpio") で読み込んだ
/// バイト列をそのまま渡せばよい。
use crate::fs::vfs::Vfs;

const NEWC_MAGIC: &[u8; 6] = b"070701";
const TRAILER: &[u8] = b"TRAILER!!!";

/// cpioアーカイブをRAMFSに展開する。
/// エラーはシリアルに出力してスキップ（パニックしない）。
pub fn extract(data: &[u8], rootfs: &mut crate::fs::ramfs::Ramfs) {
    let mut pos = 0usize;

    loop {
        // ヘッダ境界（4バイトアライン）
        pos = align4(pos);

        if pos + 110 > data.len() {
            break;
        }

        // マジックチェック
        if &data[pos..pos + 6] != NEWC_MAGIC {
            crate::serial_println!(
                "[initramfs] bad magic at offset {:#x}: {:?}",
                pos,
                &data[pos..pos + 6]
            );
            break;
        }

        let hdr = match Header::parse(&data[pos..pos + 110]) {
            Some(h) => h,
            None => {
                crate::serial_println!("[initramfs] header parse failed at {:#x}", pos);
                break;
            }
        };

        pos += 110;

        // ファイル名
        if pos + hdr.namesize > data.len() {
            break;
        }
        let name_bytes = &data[pos..pos + hdr.namesize];
        // NUL終端を除いてUTF-8に変換
        let name = core::str::from_utf8(name_bytes.split(|&b| b == 0).next().unwrap_or(name_bytes))
            .unwrap_or("<invalid>");

        pos = align4(pos + hdr.namesize);

        // TRAILERでループ終了
        if name == "TRAILER!!!" {
            crate::serial_println!("[initramfs] TRAILER found, extraction complete");
            break;
        }

        // ファイルデータ
        if pos + hdr.filesize > data.len() {
            crate::serial_println!("[initramfs] file data out of bounds: {}", name);
            break;
        }
        let file_data = &data[pos..pos + hdr.filesize];
        pos = align4(pos + hdr.filesize);

        // モードでファイル種別を判定
        // S_IFMT = 0o170000
        // S_IFREG = 0o100000, S_IFDIR = 0o040000, S_IFLNK = 0o120000
        let mode = hdr.mode;
        let file_type = (mode >> 12) & 0xF;

        match file_type {
            0o10 => {
                // 通常ファイル
                // "." はルートそのものなのでスキップ
                if name == "." {
                    continue;
                }
                let path = normalize_path(name);
                crate::serial_println!("[initramfs] file: {} ({} bytes)", path, hdr.filesize);
                if let Err(e) = rootfs.write_file(&path, file_data) {
                    crate::serial_println!("[initramfs] write_file failed: {:?}", e);
                }
            }
            0o04 => {
                // ディレクトリ
                if name == "." {
                    continue;
                }
                let path = normalize_path(name);
                crate::serial_println!("[initramfs] dir:  {}", path);
                // RAMFSはwrite_fileで自動的に親ディレクトリを作るが、
                // 空ディレクトリのために明示的に作成する
                let _ = rootfs.mkdir(&path);
            }
            0o12 => {
                // シンボリックリンク: データがリンク先パス
                let target = core::str::from_utf8(file_data)
                    .unwrap_or("")
                    .trim_end_matches('\0');
                let path = normalize_path(name);
                crate::serial_println!("[initramfs] symlink: {} -> {}", path, target);
                // RAMFSがsymlink未対応の場合は、実体を直接書く手もあるが
                // ここでは記録のみ（対応はVFS拡張で行う）
                let _ = rootfs.symlink(&path, target);
            }
            _ => {
                // デバイスファイル等は無視
                crate::serial_println!("[initramfs] skip: {} (mode={:#o})", name, mode);
            }
        }
    }
}

/// 先頭の "./" を除いて絶対パスに正規化する
fn normalize_path(name: &str) -> alloc::string::String {
    let stripped = name.strip_prefix("./").unwrap_or(name);
    alloc::format!("/{}", stripped)
}

fn align4(x: usize) -> usize {
    (x + 3) & !3
}

/// newc ヘッダ（110バイト固定、すべて8桁16進ASCII）
struct Header {
    mode: u32,
    namesize: usize,
    filesize: usize,
}

impl Header {
    fn parse(data: &[u8]) -> Option<Self> {
        // フィールドオフセット（マジック6バイトの後）:
        //  ino     [6..14]
        //  mode    [14..22]
        //  uid     [22..30]
        //  gid     [30..38]
        //  nlink   [38..46]
        //  mtime   [46..54]
        //  filesize[54..62]
        //  devmajor[62..70]
        //  devminor[70..78]
        //  rdevmajor[78..86]
        //  rdevminor[86..94]
        //  namesize[94..102]
        //  check   [102..110]
        Some(Self {
            mode: parse_hex8(&data[14..22])? as u32,
            filesize: parse_hex8(&data[54..62])? as usize,
            namesize: parse_hex8(&data[94..102])? as usize,
        })
    }
}

fn parse_hex8(s: &[u8]) -> Option<u64> {
    if s.len() < 8 {
        return None;
    }
    let mut val = 0u64;
    for &b in &s[..8] {
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        val = val * 16 + digit as u64;
    }
    Some(val)
}
