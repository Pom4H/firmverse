.syntax unified
.cpu cortex-m0
.thumb

.section .boot_vectors,"a",%progbits
.align 2
.global __boot_vectors
__boot_vectors:
    .word g_top_irqstack
    .word Reset_Boot

.section .boot_text,"ax",%progbits
.align 2
.thumb_func
.global Reset_Boot
.type Reset_Boot, %function
Reset_Boot:
    ldr r0, =g_top_irqstack
    mov sp, r0

    ldr r0, =0x40000028
    movs r1, #1
    str r1, [r0]

    ldr r0, =0x40000008
    ldr r1, [r0]
    ldr r2, =0x2080
    orrs r1, r2
    str r1, [r0]

    ldr r0, =0x40003800
    ldr r1, [r0]
    ldr r2, =0x3F
    bics r1, r2
    str r1, [r0]

    ldr r0, =0x40008000
    ldr r1, =0x40008004
    ldr r2, =0x401091
    ldr r3, [r1]
    orrs r3, r2
    str r3, [r1]

    adr r4, led_bits
    movs r6, #5
chase:
    ldr r2, [r4]
    str r2, [r0]
    ldr r5, =600000
on_wait:
    subs r5, #1
    bne on_wait
    movs r3, #0
    str r3, [r0]
    ldr r5, =200000
off_wait:
    subs r5, #1
    bne off_wait
    adds r4, #4
    subs r6, #1
    bne chase

    ldr r0, =c_start
    bx r0
    b .

.align 2
led_bits:
    .word 0x0010
    .word 0x0080
    .word 0x1000
    .word 0x0001
    .word 0x00400000
.size Reset_Boot, . - Reset_Boot
