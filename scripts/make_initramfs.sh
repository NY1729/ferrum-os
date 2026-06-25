#!/usr/bin/env bash
# make_initramfs.sh
# ferrum-os の target/esp に initramfs.cpio を生成する。
#
# 使い方:
#   ./scripts/make_initramfs.sh
#
# 前提:
#   - binaries/busybox がビルド済み（static ELF）
#   - ferrum-os リポジトリルートから実行する

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUSYBOX="$REPO_ROOT/binaries/busybox"
ESP="$REPO_ROOT/target/esp"
OUTPUT="$ESP/initramfs.cpio"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "[initramfs] repo root: $REPO_ROOT"
echo "[initramfs] busybox:   $BUSYBOX"
echo "[initramfs] output:    $OUTPUT"

if [ ! -f "$BUSYBOX" ]; then
    echo "[initramfs] ERROR: $BUSYBOX not found"
    echo "[initramfs] Build busybox first:"
    echo "  cd busybox-1.36.1 && make -j\$(nproc) CFLAGS='-static -Os' LDFLAGS='--static'"
    echo "  cp busybox-1.36.1/busybox binaries/busybox"
    exit 1
fi

# ── ディレクトリ構造 ──────────────────────────────────────────────────────────
mkdir -p "$WORK"/{bin,sbin,usr/bin,dev,proc,sys,tmp,etc,root,var/log}

# ── busybox を配置 ────────────────────────────────────────────────────────────
# RAMFSはsymlinkを解決しないので各applet名でbusyboxをコピーする。
cp "$BUSYBOX" "$WORK/bin/busybox"
chmod 755 "$WORK/bin/busybox"

echo "[initramfs] installing applets..."
for cmd in sh ash ls cat echo pwd mkdir rm cp mv grep sed awk \
           find xargs head tail wc cut sort uniq env printenv \
           true false test kill sleep; do
    cp "$WORK/bin/busybox" "$WORK/bin/$cmd"
done

for cmd in mount umount; do
    cp "$WORK/bin/busybox" "$WORK/sbin/$cmd"
done

APPLET_COUNT=$(ls "$WORK/bin" | wc -l)
echo "[initramfs] installed $APPLET_COUNT applets in /bin"

# ── /init ─────────────────────────────────────────────────────────────────────
cp "$WORK/bin/sh" "$WORK/init"
chmod 755 "$WORK/init"

cat > "$WORK/etc/inittab" << 'INITTAB'
::sysinit:/bin/sh -c "echo 'Ferrum OS'; export PATH=/bin:/sbin:/usr/bin; exec /bin/sh"
::respawn:/bin/sh
::ctrlaltdel:/bin/reboot
INITTAB

# ── /etc の最小設定 ───────────────────────────────────────────────────────────
printf 'root:x:0:0:root:/root:/bin/sh\n' > "$WORK/etc/passwd"
printf 'root:x:0:\n'                     > "$WORK/etc/group"
printf 'ferrum\n'                         > "$WORK/etc/hostname"

cat > "$WORK/etc/profile" << 'PROFILE'
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
export HOME=/root
export TERM=vt100
export PS1='ferrum# '
PROFILE

# ── cpio アーカイブを生成 ─────────────────────────────────────────────────────
mkdir -p "$ESP"
echo "[initramfs] generating $OUTPUT..."
(cd "$WORK" && find . | sort | cpio -o --format=newc --quiet) > "$OUTPUT"

SIZE=$(wc -c < "$OUTPUT")
echo "[initramfs] done: $SIZE bytes ($(( SIZE / 1024 )) KB)"
ls -lh "$ESP/"