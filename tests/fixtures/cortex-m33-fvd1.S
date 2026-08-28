.syntax unified
.cpu cortex-m33
.thumb

.section .vector_table,"a",%progbits
.word 0x38040000
.word reset
.rept 126
.word fault_handler
.endr

.section .bss.device_trace,"aw",%nobits
.align 2
.global firmverse_device_trace
.type firmverse_device_trace,%object
firmverse_device_trace:
    .space 64
.size firmverse_device_trace, .-firmverse_device_trace

.section .rodata.device_trace_init,"a",%progbits
.align 2
trace_init:
    .word 0x31445646, 1, 64, 1, 0x1f, 3, 2, 0x12345678
    .word 32, 0x87654321, 2, 2, 0xaabbccdd, 1, 0xddccbbaa, 0
trace_init_end:

.section .text.reset,"ax",%progbits
.thumb_func
.global reset
.type reset,%function
reset:
    ldr r0, =trace_init
    ldr r1, =firmverse_device_trace
    ldr r2, =trace_init_end
0:
    ldr r3, [r0], #4
    str r3, [r1], #4
    cmp r0, r2
    blo 0b

    sub.w sp, sp, #256
    movs r0, #0x42
    str r0, [sp]
    bl deep_frame
    add.w sp, sp, #256
    bl firmverse_done
1:
    b 1b

.thumb_func
.type deep_frame,%function
deep_frame:
    sub.w sp, sp, #512
    movs r1, #0x33
    str r1, [sp]
    add.w sp, sp, #512
    bx lr

.thumb_func
.global firmverse_done
.type firmverse_done,%function
firmverse_done:
    b .

.thumb_func
.type fault_handler,%function
fault_handler:
    b .
