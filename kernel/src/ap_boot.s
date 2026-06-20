.section .ap_boot, "awx", @progbits

.global ap_trampoline_start
.global ap_trampoline_end
.global ap_gdt_ptr
.global ap_idt_ptr
.global ap_pml4_addr
.global ap_stack_ptr
.global ap_main_ptr

# トランポリンは 0x8000 にコピーされる前提
# .org はセクション先頭からのオフセット

.code16
ap_trampoline_start:                    # 0x8000
    cli
    cld

    xorw %ax, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss

    # シリアルに 'A' を出力（デバッグ）
    mov $0x3F8, %dx
    mov $0x41, %al
    out %al, %dx

    # GDTロード（CS相対: CS=0x0800, offset=ap_gdt_ptr-ap_trampoline_start）
    lgdtl %cs:(ap_gdt_ptr - ap_trampoline_start)

    # プロテクトモード有効化
    movl %cr0, %eax
    orl  $0x1, %eax
    movl %eax, %cr0

    # 32bitコードへ（アドレスは.orgで固定）
    ljmpl $0x08, $0x8040

.org 0x40                               # 0x8040 に固定
.code32
ap_32bit:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss

    # PAE有効化
    movl %cr4, %eax
    orl  $0x20, %eax
    movl %eax, %cr4

    # PML4アドレスをCR3に設定
    movl (ap_pml4_addr - ap_trampoline_start + 0x8000), %eax
    movl %eax, %cr3

    # IA32_EFER.LME = 1（ロングモード有効化）
    movl $0xC0000080, %ecx
    rdmsr
    orl  $0x100, %eax
    wrmsr

    # ページング有効化
    movl %cr0, %eax
    orl  $0x80000001, %eax
    movl %eax, %cr0

    # 64bitコードへ
    ljmpl $0x08, $0x8080

.org 0x80                               # 0x8080 に固定
.code64
ap_64bit:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    xorw %ax, %ax
    movw %ax, %fs
    movw %ax, %gs

    # スタック設定
    movq (ap_stack_ptr - ap_trampoline_start + 0x8000), %rsp

    # IDTロード
    lidt (ap_idt_ptr - ap_trampoline_start + 0x8000)

    # ap_main 呼び出し
    movq (ap_main_ptr - ap_trampoline_start + 0x8000), %rax
    callq *%rax

.Lhalt:
    cli
    hlt
    jmp .Lhalt


.org 0xf0
ap_gdt_ptr:
    .word 0         # limit
    .long 0         # base (32bit)

.org 0xf8
ap_idt_ptr:
    .word 0         # limit
    .quad 0         # base (64bit)

.org 0x108
ap_pml4_addr:
    .long 0

.org 0x110
ap_stack_ptr:
    .quad 0

.org 0x118
ap_main_ptr:
    .quad 0

ap_trampoline_end: