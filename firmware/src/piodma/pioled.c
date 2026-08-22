// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// One ROM's LED engine.
//
// Owns both of One ROM's LEDs: the discrete status LED, driven from SIO, and
// the RGB LED, driven by a PIO state machine.  A caller sets a mode and the
// engine keeps it going, so nothing has to tick an LED from its own loop.
//
// The RGB LED's state machine is claimed on the first request that needs it.
// Until then the GPIO is left exactly as boot configured it, so a device
// nothing has asked about drives nothing and takes no interrupts.

#include "include.h"

#include "piodma/piodma.h"

#if defined(TEST_BUILD)
#include "test/stub.h"
#endif // TEST_BUILD

// The state machine's clock, chosen so a WS2812 bit divides evenly: a bit is
// 1.25us, its short high 0.35us and its long high 0.7us, which at 0.05us per
// cycle are 25, 7 and 14 cycles.
#define LED_PIO_HZ              20000000u

// Delays for the four program instructions, derived from those cycle counts.
// An instruction takes 1 cycle plus its delay, and the pin holds whatever the
// previous instruction left it at.
//
//   out x, 1      line low, running out the tail of the previous bit
//   set pins, 1   every bit starts high
//   mov pins, x   a 1 stays high, a 0 drops here
//   set pins, 0   the rest of the bit period
//
// So the short high is 1 + LED_DELAY_HIGH cycles, the long high adds another
// 1 + LED_DELAY_BIT, and the two low delays make the bit up to 25 cycles.
#define LED_DELAY_OUT           3u
#define LED_DELAY_HIGH          6u
#define LED_DELAY_BIT           6u
#define LED_DELAY_LOW           6u

// What a request can leave unsaid, and the floors it cannot go below, are in
// the metadata schema: LED_*_DEFAULT_*, LED_*_MIN_PERIOD_MS, LED_*_STEPS,
// LED_MIN_FRAME_MS and LED_DEFAULT_*.  The host CLI states them in its help and
// refuses what this engine would, and the plugin API tester predicts this
// engine's answers from them.
//
// A beacon takes a default duration rather than a rule of its own because a
// beacon is a bounded identify, not a mode a device sits in, and a hold is
// exactly that bound.
//
// The frame interval is a mode's period over its steps, so a longer period is
// smoother rather than slower, and blink has two steps because it has two
// states.  Each mode's minimum period is what keeps that interval at or above
// LED_MIN_FRAME_MS: below it the engine would have to run frames closer
// together than that and could not do what was asked.  A shorter period is
// refused rather than quietly treated as the minimum, which would have the
// engine report back a period it was not running at.
//
// Flame divides its own flicker table instead.  The table plays over
// LED_FLAME_DEFAULT_PERIOD_MS, so a period of that length runs it at the speed
// it is written at and a shorter one hurries it, and its shortest entry is 10ms
// of 575 - so the 500 minimum leaves every entry whole.

// The engine's state is written from two places at once: a caller in thread
// context, and the frame interrupt.  Those can be on different cores - a user
// plugin on core 0 can drive the status LED while the system plugin's animation
// runs on core 1 - so masking interrupts is not enough on its own, and neither
// is LDREX/STREX, which spans cores only when the memory is marked shareable
// and this firmware configures no MPU.  See the same reasoning at
// SPINLOCK_ORA_LOG in plugin.c.
//
// The lock is SPINLOCK_LED, allocated with the others in reg-rp235x.h.

// How long after handing a pixel to the state machine the pin is given back, on
// a board where the two LEDs share it.  The bits take about 30us at this clock
// and the pixel latches on a low of at least 50us after them, so the colour is
// taken before the pin changes hands.  For the rest of a frame - 40ms at the
// slowest mode, and no frame at all once a colour is static - the pin is the
// status LED's.
#define LED_PARK_DELAY_US       150u

// How long the pin is held low before a pixel is clocked at it, on a board
// where the status LED may have parked it high.
//
// The datasheet parameter is the reset time, Treset, which the part One ROM
// fits gives as 200us.  A WS2812 takes a frame only after that low, and a line
// left high is a state it has no reading for - so data arriving straight off a
// parked-high pin is mis-clocked, which shows as a pixel flickering or taking
// wrong colours.
#define LED_RESET_DELAY_US      200u

// How many LEDs this firmware knows.  ora_led_t numbers them.
#define LED_COUNT               2u

// A caller declares how big it believes each structure to be, and this engine
// reads and writes no more than that.  LED_REQUEST_MIN_SIZE and
// LED_STATE_MIN_SIZE, from the metadata schema, are the sizes those structures
// first shipped at and the smallest a caller may declare - anything below is a
// size no version of this API ever had.  These asserts catch a structure
// shrinking below the floor it is checked against, which would let a caller
// declare bytes the structure no longer has.
STATIC_ASSERT(
    sizeof(ora_led_request_t) >= LED_REQUEST_MIN_SIZE,
    "ora_led_request_t is smaller than the size it first shipped at"
);
STATIC_ASSERT(
    sizeof(ora_led_state_t) >= LED_STATE_MIN_SIZE,
    "ora_led_state_t is smaller than the size it first shipped at"
);

// The first GPIO the state machine can address, given GPIOBASE 16.
#define LED_GPIOBASE_FIRST      16u

// TIMER0_IRQ_1, this firmware's interrupt for the frame timer.  Alarm 0 is
// left to plugins, which reach it through ORA_IRQ_TIMER0_IRQ_0.
#define LED_TIMER_IRQ           1u

// The flame's flicker, as on and off times in milliseconds.
static const struct {
    uint8_t  on;
    uint16_t ms;
} led_flame_table[] = {
    {1, 60}, {1, 40}, {0, 15}, {1, 35}, {1, 55},
    {0, 20}, {1, 70}, {0, 10}, {1, 45}, {1, 30},
    {0, 25}, {1, 50}, {1, 65}, {0, 15}, {1, 40},
};
#define LED_FLAME_TABLE_LEN \
    (sizeof(led_flame_table) / sizeof(led_flame_table[0]))

// What one LED is doing.  24 bytes, and the saved half is what a hold returns
// the LED to.
typedef struct {
    uint8_t  mode;
    uint8_t  brightness;
    uint8_t  saved_mode;
    uint8_t  saved_brightness;

    uint8_t  red;
    uint8_t  green;
    uint8_t  blue;
    uint8_t  step;

    uint8_t  saved_red;
    uint8_t  saved_green;
    uint8_t  saved_blue;
    uint8_t  lit;

    uint16_t period_ms;
    uint16_t saved_period_ms;

    uint32_t next_frame_ms;
    uint32_t hold_expiry_ms;
} led_channel_t;
STATIC_ASSERT(sizeof(led_channel_t) == 24, "led_channel_t must be 24 bytes");

static led_channel_t led_channels[LED_COUNT];

// Interrupts are masked for as long as the lock is held.  The frame interrupt
// takes this same lock, so a holder that could be preempted on its own core
// would spin forever on a lock only the code it interrupted can release.
// Masking removes that: the holder cannot be preempted on its core, and the
// other core is excluded by the lock itself.
#if defined(TEST_BUILD)
static uint32_t led_lock(void) { return 0u; }
static void led_unlock(uint32_t primask) { (void)primask; }
#else
static uint32_t led_lock(void) {
    uint32_t primask;

    __asm volatile ("mrs %0, primask \n\t"
                    "cpsid i"
                    : "=r" (primask) :: "memory");

    while (SIO_SPINLOCK(SPINLOCK_LED) == 0u)
        ;
    __asm volatile ("dmb" ::: "memory");

    return primask;
}

static void led_unlock(uint32_t primask) {
    __asm volatile ("dmb" ::: "memory");
    SIO_SPINLOCK(SPINLOCK_LED) = 0u;
    __asm volatile ("msr primask, %0" :: "r" (primask) : "memory");
}
#endif // TEST_BUILD

// Whether the RGB LED's state machine has been programmed and enabled.
static uint8_t led_pio_claimed;

// A pin handover owed to the status LED, and when it comes due.  Only a board
// whose two LEDs share a GPIO ever has one.
static uint32_t led_park_due_us;

// When a pixel held behind a reset may go, and the pixel itself.  Only ever
// used where the pin is shared and was parked high.
static uint32_t led_write_due_us;
static uint32_t led_pending_pixel;

// The engine's flags, in one byte.  bss is capped at 3840 bytes by the linker
// and the RTT log buffer holds 3768 of it, so what is left for the rest of the
// firmware is measured in tens of bytes - see the ASSERT in link/common.ld.
//
//   PARK_PENDING   the pin is owed back to the status LED
//   PIN_WITH_PIO   the state machine holds it now, so an SIO write would not
//                  reach the pad.  Distinct from the handover being owed
//   WRITE_PENDING  a pixel is waiting behind the reset low
//   PARKED_HIGH    the pin was parked high, so the next pixel needs that reset
//   TIMER_ARMED    alarm 1 is armed, so there is something to disarm
//   RESET_OWED     the line went low too recently to be a reset, so the next
//                  pixel waits for one
#define LED_FLAG_PARK_PENDING   (1u << 0)
#define LED_FLAG_PIN_WITH_PIO   (1u << 1)
#define LED_FLAG_WRITE_PENDING  (1u << 2)
#define LED_FLAG_PARKED_HIGH    (1u << 3)
#define LED_FLAG_TIMER_ARMED    (1u << 4)
#define LED_FLAG_RESET_OWED     (1u << 5)
static uint8_t led_flags;

#define LED_FLAG_SET(f)     (led_flags |= (f))
#define LED_FLAG_CLEAR(f)   (led_flags &= (uint8_t)~(f))
#define LED_FLAG_IS_SET(f)  ((led_flags & (f)) != 0u)

// Whether this board wires both LEDs to one GPIO.
static uint8_t led_pin_is_shared(void) {
    return (HW->gpio_status < MAX_GPIOS) &&
           (HW->gpio_status == HW->gpio_neopixel);
}

// Which GPIO an LED is on, or GPIO_NONE if this board does not have it.
static uint8_t led_gpio(uint8_t led) {
    uint8_t gpio = (led == ORA_LED_RGB) ? HW->gpio_neopixel : HW->gpio_status;

    return (gpio < MAX_GPIOS) ? gpio : GPIO_NONE;
}

// Whether a mode needs the engine to come back to it.
static uint8_t led_mode_repeats(uint8_t mode) {
    return (mode == ORA_LED_MODE_BEACON) || (mode == ORA_LED_MODE_FLAME) ||
           (mode == ORA_LED_MODE_CYCLE) || (mode == ORA_LED_MODE_BREATHE) ||
           (mode == ORA_LED_MODE_BLINK);
}

// Whether a mode is built out of a colour, and so works on the RGB LED alone.
//
// Blink is not one of them.  It alternates an LED with dark, which an LED that
// has no colour does as readily as one that has.
static uint8_t led_mode_needs_colour(uint8_t mode) {
    return (mode == ORA_LED_MODE_CYCLE) || (mode == ORA_LED_MODE_BREATHE);
}

// The default period for a repeating mode, in milliseconds.
static uint16_t led_default_period(uint8_t mode) {
    switch (mode) {
        case ORA_LED_MODE_CYCLE:   return LED_CYCLE_DEFAULT_PERIOD_MS;
        case ORA_LED_MODE_BREATHE: return LED_BREATHE_DEFAULT_PERIOD_MS;
        case ORA_LED_MODE_BLINK:   return LED_BLINK_DEFAULT_PERIOD_MS;
        case ORA_LED_MODE_BEACON:  return LED_BEACON_DEFAULT_PERIOD_MS;
        case ORA_LED_MODE_FLAME:   return LED_FLAME_DEFAULT_PERIOD_MS;
        default:                   return 0;
    }
}

// The shortest period a mode accepts, in milliseconds, or 0 for a mode that
// does not repeat and so has no period to bound.
static uint16_t led_min_period(uint8_t mode) {
    switch (mode) {
        case ORA_LED_MODE_CYCLE:   return LED_CYCLE_MIN_PERIOD_MS;
        case ORA_LED_MODE_BREATHE: return LED_BREATHE_MIN_PERIOD_MS;
        case ORA_LED_MODE_BLINK:   return LED_BLINK_MIN_PERIOD_MS;
        case ORA_LED_MODE_BEACON:  return LED_BEACON_MIN_PERIOD_MS;
        case ORA_LED_MODE_FLAME:   return LED_FLAME_MIN_PERIOD_MS;
        default:                   return 0;
    }
}

// How many steps a repetition of a mode is divided into.
static uint8_t led_mode_steps(uint8_t mode) {
    switch (mode) {
        case ORA_LED_MODE_CYCLE:   return LED_CYCLE_STEPS;
        case ORA_LED_MODE_BREATHE: return LED_BREATHE_STEPS;
        case ORA_LED_MODE_BLINK:   return LED_BLINK_STEPS;
        case ORA_LED_MODE_BEACON:  return LED_BEACON_STEPS;
        default:                   return 1;
    }
}

// The interval between frames of a repeating mode, in milliseconds.
static uint32_t led_frame_interval(const led_channel_t *ch) {
    uint32_t period = ch->period_ms;
    uint32_t interval = period / led_mode_steps(ch->mode);

    return (interval < LED_MIN_FRAME_MS) ? LED_MIN_FRAME_MS : interval;
}

// The colour at a point through the hue circle, at full saturation and value.
//
// The hue is given as a step out of LED_CYCLE_STEPS rather than in degrees, so
// the arithmetic stays whole: the circle is six ramps, and a step falls at a
// known place along one of them.
static void led_hue(uint8_t step, uint8_t *red, uint8_t *green, uint8_t *blue) {
    uint32_t position = (uint32_t)step * 1536u / LED_CYCLE_STEPS;
    uint8_t  ramp = (uint8_t)(position / 256u);
    uint8_t  along = (uint8_t)(position % 256u);
    uint8_t  back = (uint8_t)(255u - along);

    switch (ramp) {
        case 0:  *red = 255;   *green = along; *blue = 0;     break;
        case 1:  *red = back;  *green = 255;   *blue = 0;     break;
        case 2:  *red = 0;     *green = 255;   *blue = along; break;
        case 3:  *red = 0;     *green = back;  *blue = 255;   break;
        case 4:  *red = along; *green = 0;     *blue = 255;   break;
        default: *red = 255;   *green = 0;     *blue = back;  break;
    }
}

// How far up the fade a breathe step is, 0 to 255 and back down.
static uint8_t led_breathe_level(uint8_t step) {
    uint8_t half = (uint8_t)(LED_BREATHE_STEPS / 2u);

    if (step < half) {
        return (uint8_t)((uint32_t)step * 255u / half);
    }

    return (uint8_t)((uint32_t)(LED_BREATHE_STEPS - step) * 255u / half);
}

// ---------------------------------------------------------------------------
// The RGB LED
// ---------------------------------------------------------------------------

// Hand one 24-bit GRB value to the state machine.
//
// APIO_TXF_AT names the block and SM, so this reaches the FIFO without going
// through APIO_SET_SM and resetting the program bookkeeping that belongs to
// building a program rather than feeding one.
#if defined(TEST_BUILD)
// The last colour handed to the state machine, and how many have been handed
// over.  A host test cannot watch the wire, and the emulated FIFO is four deep
// with nothing draining it, so the engine says what it sent.
static uint32_t led_last_pixel;
static uint32_t led_pixel_count;
#endif // TEST_BUILD

static void led_pio_write(uint32_t grb) {
#if defined(TEST_BUILD)
    led_last_pixel = grb;
    led_pixel_count++;
#endif // TEST_BUILD

    APIO_TXF_AT(BLOCK_LED, SM_LED) = grb << 8;
}

// Program and enable the state machine that drives the RGB LED.
//
// Extends the PIO configuration built at boot rather than starting over, the
// way the address monitor does, so nothing already running is disturbed.  Block
// BLOCK_LED carries no serving state machine, so the block is built from
// instruction zero.
static void led_pio_claim(uint8_t gpio) {
    uint32_t mhz = ora_get_sysclk_mhz();
    uint32_t clkdiv_int;
    uint32_t clkdiv_frac;

    // GPIOBASE 16 covers GPIOs 16 to 47, which is every GPIO an RGB LED sits
    // on, so the state machine addresses the pin relative to that window.
    uint8_t pin = (uint8_t)(gpio - LED_GPIOBASE_FIRST);

    // The SM's clock, as a divider of the system clock.  The fraction is in
    // 1/256ths.
    clkdiv_int  = (mhz * 1000000u) / LED_PIO_HZ;
    clkdiv_frac = (((mhz * 1000000u) % LED_PIO_HZ) * 256u) / LED_PIO_HZ;

    APIO_ASM_CONTINUE();

    APIO_GPIO_PULL_NONE(gpio);
    APIO_GPIO_INPUT_OUTPUT(gpio, BLOCK_LED);
    APIO_GPIO_DRIVE(gpio, APIO_DRIVE_8MA);
    APIO_GPIO_SLEW_FAST(gpio);

    APIO_SET_BLOCK_FROM(BLOCK_LED, 0);
    APIO_SET_SM(SM_LED);

    APIO_WRAP_BOTTOM();
    APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_OUT_X(1), LED_DELAY_OUT));
    APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_SET_PINS(1), LED_DELAY_HIGH));
    APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_MOV_PINS_X, LED_DELAY_BIT));
    APIO_WRAP_TOP();
    APIO_ADD_INSTR(APIO_ADD_DELAY(APIO_SET_PINS(0), LED_DELAY_LOW));

    APIO_SM_EXECCTRL_SET(0);

    // Shifting left sends the most significant bit first, which is the order a
    // WS2812 reads.  Autopull at 24 bits takes one pixel per FIFO word.
    APIO_SM_SHIFTCTRL_SET(
        APIO_AUTOPULL |
        APIO_PULL_THRESH(24) |
        APIO_OUT_SHIFTDIR_L
    );

    // SET and OUT both address the one pin, since the program drives it with
    // both.
    APIO_SM_PINCTRL_SET(
        APIO_SET_BASE(pin) |
        APIO_SET_COUNT(1) |
        APIO_OUT_BASE(pin) |
        APIO_OUT_COUNT(1)
    );

    APIO_SM_CLKDIV_SET(clkdiv_int, clkdiv_frac);

    // Take the pin low and make it an output before the program runs, so the
    // line is at the WS2812's reset level rather than wherever boot left it.
    APIO_SM_EXEC_INSTR(APIO_SET_PINS(0));
    APIO_SM_EXEC_INSTR(APIO_SET_PIN_DIRS(1));
    APIO_SM_JMP_TO_START();

    APIO_GPIOBASE_16();

    APIO_END_BLOCK();
    APIO_ENABLE_SMS(BLOCK_LED, 1u << SM_LED);

    led_pio_claimed = 1;

    // Setting the pin low above started a low, and the chip takes a frame only
    // after one that has lasted its reset time.  Microseconds have passed, so
    // the first pixel owes that wait - this is the frame that follows power-up,
    // and without the wait the chip reads it out of step and lights a colour
    // nobody asked for.
    LED_FLAG_SET(LED_FLAG_RESET_OWED);

    // On a shared pin the claim is only ever momentary: the status LED has the
    // pin the rest of the time, so it is owed straight back.
    if (led_pin_is_shared()) {
        led_park_due_us = (uint32_t)onerom_timer_us64() + LED_PARK_DELAY_US;
        LED_FLAG_SET(LED_FLAG_PARK_PENDING);
    }
}

// Hand the shared pin back to the status LED.
//
// The state machine has long since clocked the pixel out and the LED has
// latched it, so it keeps its colour while the pin is SIO's - a WS2812 holds
// what it was last given and ignores a line that is not clocking data at it.
static void led_park(void) {
    uint8_t gpio = HW->gpio_status;

    LED_FLAG_CLEAR(LED_FLAG_PARK_PENDING);

    if (!led_pin_is_shared()) {
        return;
    }

    LED_FLAG_CLEAR(LED_FLAG_PIN_WITH_PIO);
    gpio_pio_release(gpio);

    // Reclaims funcsel, drive and output enable, then applies the level the
    // status LED is meant to be at.  Called unconditionally: setup_status_led
    // carries its own REAL_HARDWARE guard, and on a host the other two drive
    // the pad model.
    setup_status_led();
    if (RUNTIME->status_led_enabled) {
        status_led_on(gpio);
    } else {
        status_led_off(gpio);
    }

    // A status LED that is off leaves the line high, which is not a state a
    // WS2812 can read a frame out of.  The next pixel has to wait behind a
    // reset because of it.
    if (RUNTIME->status_led_enabled) {
        LED_FLAG_CLEAR(LED_FLAG_PARKED_HIGH);
    } else {
        LED_FLAG_SET(LED_FLAG_PARKED_HIGH);
    }
}

// Take the shared pin back for the state machine, which owns it only while a
// pixel is being clocked out.
static void led_borrow_pin(uint8_t gpio) {
    if (!led_pin_is_shared()) {
        return;
    }

    APIO_GPIO_INPUT_OUTPUT(gpio, BLOCK_LED);
    gpio_pio_claim(gpio);
    LED_FLAG_SET(LED_FLAG_PIN_WITH_PIO);
    led_park_due_us = (uint32_t)onerom_timer_us64() + LED_PARK_DELAY_US;
    LED_FLAG_SET(LED_FLAG_PARK_PENDING);
}

// Send a colour to the RGB LED, scaled by brightness and by an envelope.
//
// The RGB LED supports brightness, and this is where it is applied: the chip
// takes a colour and nothing else, so half brightness is half of each of the
// three values.  The envelope is how far up a fade the frame is, 0 to 255, and
// multiplies with brightness rather than replacing it - a breathe at 40%
// brightness peaks at 40%.
static void led_rgb_write(
    const led_channel_t *ch,
    uint8_t red,
    uint8_t green,
    uint8_t blue,
    uint8_t level
) {
    uint32_t scale = (uint32_t)ch->brightness * (uint32_t)level;
    uint8_t  gpio = led_gpio(ORA_LED_RGB);
    uint32_t out_red;
    uint32_t out_green;
    uint32_t out_blue;

    if (gpio == GPIO_NONE) {
        return;
    }

    if (!led_pio_claimed) {
        led_pio_claim(gpio);
    }

    // On a shared pin the state machine gets it for as long as the pixel takes,
    // and the status LED has it the rest of the time.
    led_borrow_pin(gpio);

    // Brightness is a percentage and the envelope is out of 255, so the two
    // divisors are applied together to keep the rounding in one place.
    out_red   = ((uint32_t)red * scale) / (100u * 255u);
    out_green = ((uint32_t)green * scale) / (100u * 255u);
    out_blue  = ((uint32_t)blue * scale) / (100u * 255u);

    // A WS2812 reads green, then red, then blue.
    {
        uint32_t pixel = (out_green << 16) | (out_red << 8) | out_blue;

        // What decides this is whether the line has been low long enough, not
        // who owns the pin.  Two things leave a reset owed: the state machine
        // has just been claimed, and a shared pin the status LED had parked
        // high, which the borrow above has only now taken low.  A board where
        // the two LEDs sit on separate pins needs the first of those just as
        // much - it escapes only where a shared status LED happened to be
        // holding the line low already.
        if (LED_FLAG_IS_SET(LED_FLAG_RESET_OWED) ||
            (led_pin_is_shared() && LED_FLAG_IS_SET(LED_FLAG_PARKED_HIGH))) {
            // The pixel goes when that low has lasted long enough to be the
            // reset a frame needs.
            led_pending_pixel = pixel;
            led_write_due_us = (uint32_t)onerom_timer_us64() +
                               LED_RESET_DELAY_US;
            LED_FLAG_SET(LED_FLAG_WRITE_PENDING);
            LED_FLAG_CLEAR(LED_FLAG_RESET_OWED);
            led_park_due_us = led_write_due_us + LED_PARK_DELAY_US;
        } else if (LED_FLAG_IS_SET(LED_FLAG_WRITE_PENDING)) {
            // One is already waiting out a reset, so this replaces it rather
            // than going out in front of it.  Sending this one now would put
            // the older colour on the wire afterwards, and the LED would end
            // up showing whatever was asked for first.
            led_pending_pixel = pixel;
        } else {
            led_pio_write(pixel);
        }
    }
}

// Show the channel's own colour, lit or dark.
static void led_rgb_show(const led_channel_t *ch, uint8_t lit) {
    led_rgb_write(ch, ch->red, ch->green, ch->blue, lit ? 255u : 0u);
}

// ---------------------------------------------------------------------------
// The status LED
// ---------------------------------------------------------------------------

// Light or darken the status LED.
static void led_status_show(uint8_t lit) {
    uint8_t gpio = led_gpio(ORA_LED_STATUS);

    // status_led_enabled is the live state and the cross-plugin coordination
    // channel, so it is recorded whether or not there is a pin to drive.
    RUNTIME->status_led_enabled = lit ? 1u : 0u;

    if (gpio == GPIO_NONE) {
        return;
    }

    // The status LED is active low, so lit drives the pin low.  On a board that
    // shares the pin, a write while the state machine holds it reaches nothing
    // - the block drives the pad - and neither the hardware nor the pad model
    // needs telling that here.
    if (lit) {
        status_led_on(gpio);
    } else {
        status_led_off(gpio);
    }
}

// ---------------------------------------------------------------------------
// Driving a channel
// ---------------------------------------------------------------------------

// Put an LED at the level its channel currently calls for.
static void led_show(uint8_t led, led_channel_t *ch, uint8_t lit) {
    ch->lit = lit ? 1u : 0u;

    if (led == ORA_LED_RGB) {
        led_rgb_show(ch, ch->lit);
    } else {
        led_status_show(ch->lit);
    }
}

// ---------------------------------------------------------------------------
// The frame timer
// ---------------------------------------------------------------------------

#if defined(TEST_BUILD)
// When the engine next wants attention, and whether it wants any.  A device
// carries this in the alarm register and never reads it back, so it is kept
// only where something reads it: the harness, through pio_led_next_deadline().
static uint32_t led_deadline_ms;
static uint8_t  led_have_deadline;
#endif // TEST_BUILD

// Arm or disarm TIMER0 alarm 1 for the earliest deadline any channel has.
//
// The core that arms the alarm is the core that services it, since the NVIC is
// per core.  Neither core serves ROM bytes, so it does not matter which, but it
// is worth knowing which one an animation runs on.
//
// now_ms is passed in rather than read here.  Both callers have just read it,
// so reading it again costs a call out of a frame this reaches from a timer
// interrupt, on a stack a plugin shares.  It also means the deadlines a caller
// acted on and the alarm armed for them are the same instant, where two reads
// could straddle a millisecond and arm for one already gone.
static void led_rearm(uint32_t now_ms) {
    uint32_t earliest = 0;
    uint8_t  have_deadline = 0;
    uint32_t wait_us;

    for (uint8_t led = 0; led < LED_COUNT; led++) {
        led_channel_t *ch = &led_channels[led];
        uint32_t deadlines[2];
        uint8_t  count = 0;

        if (led_mode_repeats(ch->mode)) {
            deadlines[count++] = ch->next_frame_ms;
        }
        if (ch->hold_expiry_ms != 0u) {
            deadlines[count++] = ch->hold_expiry_ms;
        }

        for (uint8_t i = 0; i < count; i++) {
            if (!have_deadline || ((int32_t)(deadlines[i] - earliest) < 0)) {
                earliest = deadlines[i];
                have_deadline = 1;
            }
        }
    }

    // A frame is due in whole milliseconds.  A pin owed back to the status LED
    // is due in microseconds, and sooner than any frame, so it wins.
    {
        int32_t wait_ms = have_deadline ? (int32_t)(earliest - now_ms) : 0;

        wait_us = (wait_ms > 0) ? ((uint32_t)wait_ms * 1000u) : 0u;

        if (LED_FLAG_IS_SET(LED_FLAG_PARK_PENDING | LED_FLAG_WRITE_PENDING)) {
            uint32_t now_us = (uint32_t)onerom_timer_us64();
            uint32_t due_us = LED_FLAG_IS_SET(LED_FLAG_WRITE_PENDING) ? led_write_due_us
                                                : led_park_due_us;
            int32_t  wait = (int32_t)(due_us - now_us);
            uint32_t soon_us = (wait > 0) ? (uint32_t)wait : 0u;

            if (!have_deadline || (soon_us < wait_us)) {
                wait_us = soon_us;
            }
            have_deadline = 1;
        }

#if defined(TEST_BUILD)
        // What the engine is waiting for, in the milliseconds a caller reads.
        // Rounded up, so a caller that waits for it has passed the deadline
        // rather than arrived just short of it - a pin owed back in 150us is
        // due within the millisecond, not at the top of this one.
        led_have_deadline = have_deadline;
        led_deadline_ms = now_ms + ((wait_us + 999u) / 1000u);
#endif // TEST_BUILD
    }

#if !defined(TEST_BUILD)
    if (!have_deadline) {
        // Nothing to disarm if nothing was armed.  That is not only tidiness:
        // the first call comes from boot, before setup_timer0() has run, and
        // the engine must not write a peripheral's registers to clear a state
        // it never set.  Setting an LED and leaving it lit schedules nothing,
        // so it reaches here and returns without touching the timer at all.
        if (LED_FLAG_IS_SET(LED_FLAG_TIMER_ARMED)) {
            TIMER0_INTE &= ~TIMER0_INT_ALARM1;
            NVIC_ICER0 = (1u << LED_TIMER_IRQ);
            LED_FLAG_CLEAR(LED_FLAG_TIMER_ARMED);
        }
        return;
    }

    // The alarm compares against the low word of the microsecond counter, so
    // the wait is added to the counter as it reads now.  A deadline already
    // passed becomes a wait of zero, which the hardware fires immediately.
    TIMER0_INTR = TIMER0_INT_ALARM1;
    TIMER0_ALARM1 = TIMER0_TIMERAWL + wait_us;
    TIMER0_INTE |= TIMER0_INT_ALARM1;
    NVIC_ISER0 = (1u << LED_TIMER_IRQ);
    LED_FLAG_SET(LED_FLAG_TIMER_ARMED);
#else // TEST_BUILD
    (void)wait_us;
#endif // !TEST_BUILD
}

// Take a channel back to what it was doing before its hold.
static void led_restore(uint8_t led, led_channel_t *ch) {
    ch->mode       = ch->saved_mode;
    ch->brightness = ch->saved_brightness;
    ch->red          = ch->saved_red;
    ch->green        = ch->saved_green;
    ch->blue         = ch->saved_blue;
    ch->period_ms  = ch->saved_period_ms;
    ch->step       = 0;
    ch->hold_expiry_ms = 0;

    led_show(led, ch, (ch->mode != ORA_LED_MODE_OFF));

    if (led_mode_repeats(ch->mode)) {
        ch->next_frame_ms = ora_get_plugin_uptime_ms();
    }
}

// Advance one repeating channel, if its deadline has come.
static void led_advance(uint8_t led, led_channel_t *ch, uint32_t now_ms) {
    if ((int32_t)(now_ms - ch->next_frame_ms) < 0) {
        return;
    }

    switch (ch->mode) {
        case ORA_LED_MODE_BEACON:
            // Half a period lit and half dark.  What ends it is its hold, which
            // a request either gave or took the default of.
            ch->step = (uint8_t)((ch->step + 1u) % LED_BEACON_STEPS);
            led_show(led, ch, ch->lit ? 0u : 1u);
            ch->next_frame_ms = now_ms + led_frame_interval(ch);
            break;

        case ORA_LED_MODE_FLAME: {
            uint32_t entry_ms;

            ch->step = (uint8_t)((ch->step + 1u) % LED_FLAME_TABLE_LEN);
            led_show(led, ch, led_flame_table[ch->step].on);

            // The table is written to play over LED_FLAME_DEFAULT_PERIOD_MS, so a
            // period scales every entry by the same amount and the flicker
            // keeps its shape.
            entry_ms = ((uint32_t)led_flame_table[ch->step].ms *
                        (uint32_t)ch->period_ms) / LED_FLAME_DEFAULT_PERIOD_MS;
            if (entry_ms < 1u) {
                entry_ms = 1u;
            }
            ch->next_frame_ms = now_ms + entry_ms;
            break;
        }

        case ORA_LED_MODE_CYCLE: {
            uint8_t red;
            uint8_t green;
            uint8_t blue;

            ch->step = (uint8_t)((ch->step + 1u) % LED_CYCLE_STEPS);
            led_hue(ch->step, &red, &green, &blue);
            led_rgb_write(ch, red, green, blue, 255u);
            ch->lit = 1;
            ch->next_frame_ms = now_ms + led_frame_interval(ch);
            break;
        }

        case ORA_LED_MODE_BREATHE: {
            uint8_t level;

            ch->step = (uint8_t)((ch->step + 1u) % LED_BREATHE_STEPS);
            level = led_breathe_level(ch->step);
            led_rgb_write(ch, ch->red, ch->green, ch->blue, level);
            ch->lit = (level != 0u);
            ch->next_frame_ms = now_ms + led_frame_interval(ch);
            break;
        }

        case ORA_LED_MODE_BLINK:
            ch->step = (uint8_t)((ch->step + 1u) % LED_BLINK_STEPS);
            led_show(led, ch, (ch->step == 0u) ? 1u : 0u);
            ch->next_frame_ms = now_ms + led_frame_interval(ch);
            break;

        default:
            break;
    }
}

void pio_led_frame(void) {
    uint32_t now_ms = ora_get_plugin_uptime_ms();
    uint32_t primask = led_lock();

    // Both of these are owed in microseconds where a frame is owed in
    // milliseconds, so if any two are due these are the ones that were late.
    // The waiting pixel goes first: the handover after it is what its own
    // deadline was set from.
    if (LED_FLAG_IS_SET(LED_FLAG_WRITE_PENDING) &&
        ((int32_t)((uint32_t)onerom_timer_us64() - led_write_due_us) >= 0)) {
        LED_FLAG_CLEAR(LED_FLAG_WRITE_PENDING);
        led_pio_write(led_pending_pixel);
    }

    if (LED_FLAG_IS_SET(LED_FLAG_PARK_PENDING) &&
        ((int32_t)((uint32_t)onerom_timer_us64() - led_park_due_us) >= 0)) {
        led_park();
    }

    for (uint8_t led = 0; led < LED_COUNT; led++) {
        led_channel_t *ch = &led_channels[led];

        if ((ch->hold_expiry_ms != 0u) &&
            ((int32_t)(now_ms - ch->hold_expiry_ms) >= 0)) {
            led_restore(led, ch);
            continue;
        }

        if (led_mode_repeats(ch->mode)) {
            led_advance(led, ch, now_ms);
        }
    }

    led_rearm(now_ms);
    led_unlock(primask);
}

// ---------------------------------------------------------------------------
// The API
// ---------------------------------------------------------------------------

#if defined(TEST_BUILD)
void pio_led_reset(void) {
    memset(led_channels, 0, sizeof(led_channels));
    led_pio_claimed = 0;
    led_deadline_ms = 0;
    led_have_deadline = 0;
    led_flags = 0;
    led_park_due_us = 0;
    led_write_due_us = 0;
    led_pending_pixel = 0;
    led_last_pixel = 0;
    led_pixel_count = 0;
}

uint32_t pio_led_last_pixel(uint32_t *count_out) {
    if (count_out != NULL) {
        *count_out = led_pixel_count;
    }

    return led_last_pixel;
}

uint8_t pio_led_next_deadline(uint32_t *ms_out) {
    if (led_have_deadline && (ms_out != NULL)) {
        *ms_out = led_deadline_ms;
    }

    return led_have_deadline;
}
#endif // TEST_BUILD

ora_result_t pio_led_set(const ora_led_request_t *req) {
    uint32_t now_ms;
    uint32_t primask;
    led_channel_t *ch;
    uint8_t hold;
    uint32_t hold_ms;
    uint8_t bounded;
    uint8_t already_bounded;

    if (req == NULL) {
        return ORA_RESULT_INVALID_ARG;
    }

    if (req->size < LED_REQUEST_MIN_SIZE) {
        return ORA_RESULT_INVALID_SIZE;
    }

    if (req->led >= LED_COUNT) {
        return ORA_RESULT_INVALID_ARG;
    }

    if (req->mode > ORA_LED_MODE_BLINK) {
        return ORA_RESULT_INVALID_ARG;
    }

    // A mode built out of a colour has nothing to do on an LED that has none.
    if (led_mode_needs_colour(req->mode) && (req->led != ORA_LED_RGB)) {
        return ORA_RESULT_INVALID_ARG;
    }

    // Brightness is a percentage, and the scale applied in led_rgb_show would
    // otherwise carry a larger value past 255 and out of the colour's byte.
    if (req->brightness > 100u) {
        return ORA_RESULT_INVALID_ARG;
    }

    // Refused rather than clamped, so a caller is never told it got something
    // it did not.
    if (req->hold_ms > LED_MAX_HOLD_MS) {
        return ORA_RESULT_INVALID_ARG;
    }

    // Zero is "the mode's own default", so only a stated period is bounded.
    if ((req->period_ms != 0u) && (req->period_ms < led_min_period(req->mode))) {
        return ORA_RESULT_INVALID_ARG;
    }

    if (led_gpio(req->led) == GPIO_NONE) {
        return ORA_RESULT_NOT_SUPPORTED;
    }

    now_ms = ora_get_plugin_uptime_ms();
    ch = &led_channels[req->led];

    // A beacon that named no hold gets its own default, because a beacon is
    // bounded by definition - it is an identify, not a mode to sit in.
    hold_ms = req->hold_ms;
    if ((req->mode == ORA_LED_MODE_BEACON) && (hold_ms == 0u)) {
        hold_ms = LED_BEACON_DEFAULT_DURATION_MS;
    }
    hold = (hold_ms != 0u);

    // From here the channel is written, and the frame interrupt writes the same
    // state.  The RGB LED's state machine is claimed inside this too, on the
    // one call that finds it unclaimed.
    primask = led_lock();

    // A beacon is bounded whether or not a hold was asked for - it ends itself -
    // so it captures what it interrupts the same way a hold does.
    bounded = hold || (req->mode == ORA_LED_MODE_BEACON);
    already_bounded = (ch->hold_expiry_ms != 0u) ||
                      (ch->mode == ORA_LED_MODE_BEACON);

    // What a bounded mode returns to is captured on the way in only.  One
    // arriving while another is running leaves that alone, so the LED goes back
    // to what it was doing before the first of them.
    if (bounded && !already_bounded) {
        ch->saved_mode       = ch->mode;
        ch->saved_brightness = ch->brightness;
        ch->saved_red          = ch->red;
        ch->saved_green        = ch->green;
        ch->saved_blue         = ch->blue;
        ch->saved_period_ms  = ch->period_ms;
    }

    ch->mode       = req->mode;
    ch->period_ms  = (req->period_ms != 0u) ? req->period_ms
                                             : led_default_period(req->mode);
    ch->step       = 0;

    if (req->led == ORA_LED_RGB) {
        ch->brightness = (req->brightness != 0u) ? req->brightness
                                                 : LED_DEFAULT_BRIGHTNESS;

        if ((req->red | req->green | req->blue) != 0u) {
            ch->red = req->red;
            ch->green = req->green;
            ch->blue = req->blue;
        } else {
            // A caller that named no colour gets red rather than a dark LED.
            // One ROM is red, and so is its status LED.
            ch->red = LED_DEFAULT_RED;
            ch->green = LED_DEFAULT_GREEN;
            ch->blue = LED_DEFAULT_BLUE;
        }
    } else {
        // An LED with no colour records none.  Filling these in from a request
        // that carried them would have a reader of ora_led_get believe the
        // status LED was lit some colour at some brightness, and it is lit or
        // it is dark.
        ch->brightness = 0;
        ch->red = 0;
        ch->green = 0;
        ch->blue = 0;
    }

    if (hold) {
        // The sum is 32-bit millisecond arithmetic and wraps, and zero is this
        // field's "no hold is running" - so a hold landing exactly there would
        // read as no hold at all, and the frame would never take the LED back.
        // Ending a millisecond later is a value the field can hold.
        ch->hold_expiry_ms = now_ms + hold_ms;
        if (ch->hold_expiry_ms == 0u) {
            ch->hold_expiry_ms = 1u;
        }
    }

    switch (ch->mode) {
        case ORA_LED_MODE_OFF:
            led_show(req->led, ch, 0);
            break;

        case ORA_LED_MODE_ON:
            led_show(req->led, ch, 1);
            break;

        case ORA_LED_MODE_BEACON:
        case ORA_LED_MODE_FLAME:
        case ORA_LED_MODE_CYCLE:
        case ORA_LED_MODE_BREATHE:
        case ORA_LED_MODE_BLINK:
            // Start at the first frame rather than waiting one interval, so a
            // mode is visible the moment it is asked for.
            led_show(req->led, ch, 1);
            ch->next_frame_ms = now_ms + led_frame_interval(ch);
            break;

        default:
            break;
    }

    led_rearm(now_ms);
    led_unlock(primask);

    return ORA_RESULT_OK;
}

void pio_led_boot(void) {
    ora_led_request_t req = {0};

    req.size = sizeof(req);
    req.led  = ORA_LED_STATUS;
    req.mode = RUNTIME->status_led_enabled ? ORA_LED_MODE_ON : ORA_LED_MODE_OFF;

    DEBUG("Status LED %s", RUNTIME->status_led_enabled ? "on" : "off");

    pio_led_set(&req);
}

ora_result_t pio_led_get(uint8_t led, ora_led_state_t *state_out) {
    // Zeroed so the reserved byte, and any padding a later field leaves,
    // reach the caller as zero rather than as whatever the stack held.
    ora_led_state_t state = {0};
    uint32_t primask;
    uint8_t want;
    const led_channel_t *ch;

    if ((state_out == NULL) || (led >= LED_COUNT)) {
        return ORA_RESULT_INVALID_ARG;
    }

    want = state_out->size;
    if (want < LED_STATE_MIN_SIZE) {
        return ORA_RESULT_INVALID_SIZE;
    }
    if (want > (uint8_t)sizeof(state)) {
        want = (uint8_t)sizeof(state);
    }

    ch = &led_channels[led];

    primask = led_lock();

    state.size        = want;
    state.led         = led;
    state.present     = (led_gpio(led) != GPIO_NONE) ? 1u : 0u;
    state.mode        = ch->mode;
    state.brightness  = ch->brightness;
    state.red           = ch->red;
    state.green           = ch->green;
    state.blue           = ch->blue;
    state.gpio        = led_gpio(led);
    state.period_ms   = ch->period_ms;

    led_unlock(primask);

    // Copied out from the snapshot rather than field by field from the
    // channel, so what a caller reads is one instant rather than several.
    memcpy(state_out, &state, want);

    return ORA_RESULT_OK;
}
