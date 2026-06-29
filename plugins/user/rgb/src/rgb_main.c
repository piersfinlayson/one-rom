// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM user plugin: NeoPixel smooth colour cycle

#include "plugin.h"
#include "onerom_metadata.h"

ORA_DEFINE_USER_PLUGIN(
    neopixel_main,
    0, 1, 1, 0,
    0, 6, 7
);

// RP2350 SIO register offsets differ from RP2040 — the HI variants
// (for pins 32-47) are interleaved, shifting SET/CLR/OE offsets.
#define SIO_BASE            0xD0000000u
#define SIO_GPIO_OUT_SET    (*(volatile uint32_t *)(SIO_BASE + 0x018u))
#define SIO_GPIO_OUT_CLR    (*(volatile uint32_t *)(SIO_BASE + 0x020u))
#define SIO_GPIO_OE_SET     (*(volatile uint32_t *)(SIO_BASE + 0x038u))
#define SIO_GPIO_HI_OUT_SET (*(volatile uint32_t *)(SIO_BASE + 0x01Cu))
#define SIO_GPIO_HI_OUT_CLR (*(volatile uint32_t *)(SIO_BASE + 0x024u))
#define SIO_GPIO_HI_OE_SET  (*(volatile uint32_t *)(SIO_BASE + 0x03Cu))
#define IO_BANK0_BASE       0x40028000u
#define GPIO_CTRL(n)        (*(volatile uint32_t *)(IO_BANK0_BASE + (uint32_t)(n) * 8u + 4u))
#define FUNCSEL_SIO         5u

#define PADS_BANK0_BASE     0x40038000
#define PAD_OFFSET_START    0x004
#define PAD_SPACING         0x004
#define GPIO_PAD(pin)       (*(volatile uint32_t*)(PADS_BANK0_BASE + PAD_OFFSET_START + pin*PAD_SPACING))  
#define PAD_DRIVE_BIT       4
#define PAD_DRIVE_8MA       0x2
#define PAD_DRIVE_MASK      0x3
#define PAD_DRIVE(X)        ((X & PAD_DRIVE_MASK) << PAD_DRIVE_BIT)
#define PAD_SLEW_FAST_BIT   0
#define PAD_SLEW_FAST       (1 << PAD_SLEW_FAST_BIT)

#define SYSINFO_BASE        0x40000000
#define SYSINFO_PACKAGE_SEL     (*((volatile uint32_t *)(SYSINFO_BASE + 0x04)))
#define SYSINFO_IS_QFN60()      (SYSINFO_PACKAGE_SEL & 0b1)

// Change to match your wiring
#define NEOPIXEL_PIN_A      29u
#define NEOPIXEL_PIN_B      44u

// WS2812 nominal pulse widths in nanoseconds
#define T0H_NS   350u
#define T0L_NS   800u
#define T1H_NS   700u
#define T1L_NS   600u
#define RST_US    60u

static volatile uint32_t *s_out_set;
static volatile uint32_t *s_out_clr;
static uint32_t s_t0h, s_t0l, s_t1h, s_t1l, s_rst;
static uint32_t s_pin_mask;

// ~3 cycles/iteration on Cortex-M33 (1 for SUBS, 2 for BNE taken).
// WS2812 tolerances are ±150ns so this approximation is fine at any
// reasonable sysclk.
static inline void delay_cycles(uint32_t cycles) {
    uint32_t n = cycles / 3u;
    __asm volatile (
        "1: subs %0, %0, #1\n"
        "   bne 1b\n"
        : "+r"(n) :: "cc"
    );
}

// Marked noinline so call overhead is constant and predictable across bits.
__attribute__((noinline))
static void send_bit(uint32_t bit) {
    if (bit) {
        *s_out_set = s_pin_mask; delay_cycles(s_t1h);
        *s_out_clr = s_pin_mask; delay_cycles(s_t1l);
    } else {
        *s_out_set = s_pin_mask; delay_cycles(s_t0h);
        *s_out_clr = s_pin_mask; delay_cycles(s_t0l);
    }
}

// WS2812 expects GRB order, MSB first
static void send_pixel(uint8_t r, uint8_t g, uint8_t b) {
    uint32_t grb = ((uint32_t)g << 16) | ((uint32_t)r << 8) | b;
    for (int i = 23; i >= 0; i--) {
        send_bit((grb >> i) & 1u);
    }
}

static void reset_pulse(void) {
    *s_out_clr = s_pin_mask;
    delay_cycles(s_rst);
}

// Integer HSV→RGB. h in [0,360), s and v in [0,255].
static void hsv_to_rgb(uint16_t h, uint8_t s, uint8_t v,
                       uint8_t *r, uint8_t *g, uint8_t *b) {
    if (s == 0) { *r = *g = *b = v; return; }

    uint8_t  region    = (uint8_t)(h / 60u);
    uint32_t remainder = (h % 60u) * 256u / 60u;
    uint8_t  p = (uint8_t)((uint32_t)v * (255u - s) / 255u);
    uint8_t  q = (uint8_t)((uint32_t)v * (255u - s * remainder / 256u) / 255u);
    uint8_t  t = (uint8_t)((uint32_t)v * (255u - s * (255u - remainder) / 256u) / 255u);

    switch (region) {
        case 0:  *r = v; *g = t; *b = p; break;
        case 1:  *r = q; *g = v; *b = p; break;
        case 2:  *r = p; *g = v; *b = t; break;
        case 3:  *r = p; *g = q; *b = v; break;
        case 4:  *r = t; *g = p; *b = v; break;
        default: *r = v; *g = p; *b = q; break;
    }
}

void neopixel_main(
    ora_lookup_fn_t ora_lookup_fn,
    ora_plugin_type_t plugin_type,
    const ora_entry_args_t *entry_args
) {
    (void)plugin_type;
    (void)entry_args;

    ora_get_sysclk_mhz_fn_t get_sysclk = ora_lookup_fn(ORA_ID_GET_SYSCLK_MHZ);
    uint32_t mhz = get_sysclk();

    // cycles = (ns × MHz) / 1000
    s_t0h = (T0H_NS * mhz) / 1000u;
    s_t0l = (T0L_NS * mhz) / 1000u;
    s_t1h = (T1H_NS * mhz) / 1000u;
    s_t1l = (T1L_NS * mhz) / 1000u;
    s_rst = RST_US * mhz;           // µs × MHz = cycles directly

    // Figure out whether we're an RP235xA or B.
    uint8_t neopixel_pin = NEOPIXEL_PIN_B;
    uint8_t rp235xa = SYSINFO_IS_QFN60();
    if (rp235xa) {
        neopixel_pin = NEOPIXEL_PIN_A;
    }

    // Configure pin: set function to SIO, drive low, enable output
    s_pin_mask = 1u << (neopixel_pin & 31u);
    GPIO_PAD(neopixel_pin) = PAD_DRIVE(PAD_DRIVE_8MA) | PAD_SLEW_FAST;
    GPIO_CTRL(neopixel_pin) = FUNCSEL_SIO;
    if (neopixel_pin >= 32u) {
        s_out_set = &SIO_GPIO_HI_OUT_SET;
        s_out_clr = &SIO_GPIO_HI_OUT_CLR;
        SIO_GPIO_HI_OUT_CLR = s_pin_mask;
        SIO_GPIO_HI_OE_SET  = s_pin_mask;
    } else {
        s_out_set = &SIO_GPIO_OUT_SET;
        s_out_clr = &SIO_GPIO_OUT_CLR;
        SIO_GPIO_OUT_CLR = s_pin_mask;
        SIO_GPIO_OE_SET  = s_pin_mask;
    }

    // Cycle through hue 0–359 at full saturation and brightness.
    // One full cycle takes ~360 frames; adjust the inter-frame delay to
    // taste — it is clock-dependent as in the blink plugin.
    uint16_t hue = 0;
    while (1) {
        uint8_t r, g, b;
        hsv_to_rgb(hue, 255, 255, &r, &g, &b);

        reset_pulse();
        send_pixel(r, g, b);

        hue = (hue + 1u) % 360u;

        // ~2-3ms inter-frame delay at 150MHz; one full hue cycle ≈ 1 second
        for (volatile int i = 0; i < 100000; i++);
    }
}