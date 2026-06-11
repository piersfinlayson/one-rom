; Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
;
; MIT License

; C64 program to work alongside the One ROM c64-kernal-mod example plugin.
; This program instructs the One ROM plugin to modify the KERNAL ROM.  Once
; that's complete, the program returns.  You can then reset the C64 and the
; modified kernal will show a different boot banner.

; First two bytes of the PRG at the address to load to
.segment "LOADADDR"
    .word $0801

; BASIC stub to launch the machine code program which follows
; 10 SYS2061
.segment "BASIC"
    .byte $0B, $08, $0A, $00, $9E, $32, $30, $36, $31, $00, $00, $00

; Address of CHROUT routine in the C64 KERNAL ROM
CHROUT = $FFD2

.segment "CODE"
start:
    sei

    ; Issue knock sequence - reads within Kernal ROM ($E000-$FFFF)
    ; where A[7:0] spells ONEROM!
    lda $E04F       ; 'O' $4F
    lda $E04E       ; 'N' $4E
    lda $E045       ; 'E' $45
    lda $E052       ; 'R' $52
    lda $E04F       ; 'O' $4F
    lda $E04D       ; 'M' $4D
    lda $E021       ; '!' $21

    ; Mode byte - value ignored by this demo
    lda $E001

    ; Poll $E4A9 until One ROM has reprogrammed the Kernal.
    ; Original value is $4D ('M'), new value is $53 ('S') from "PIERS.ROCKS"
    ; The last byte of the modified banner is actually 0xE4AA, but that
    ; doesn't actually change from the original value of 0x20.
@wait:
    lda $E4A9
    cmp #$53
    bne @wait

    ; Reprogramming complete - re-enable interrupts and return to BASIC
    cli

    ldx #$00
@print:
    lda msg,x
    beq @done
    jsr CHROUT
    inx
    bne @print
@done:
    rts

msg:
    .byte "KERNAL MODIFIED", $0D, $00