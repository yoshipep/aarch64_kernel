.section .text

.global evt
.balign 2048

evt:
.skip 0x200

sync:
    b .

.balign 0x80
irq:
    b .

.balign 0x80
fiq:
    b .

.balign 0x80
serror:
    b .
