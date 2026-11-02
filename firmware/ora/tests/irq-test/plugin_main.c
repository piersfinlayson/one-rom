// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Exercises how One ROM delivers interrupts to plugins, and exclusive mode
// with both plugins requesting it at once.
//
// One source, flashed as both the system and the user plugin, so an instance
// runs on each core.  The two keep in step using the SIO doorbells.  Flashed
// alone it runs the checks one instance can run and reports SOLO.
//
// The verdict is a repeating count of status LED flashes, so no debug probe is
// needed to read it.  See README.md.

#include "plugin.h"

// ora/plugin.mk defines PLUGIN_IS_SYSTEM to 1 or 0 from the PLUGIN_TYPE passed
// to make.
#ifndef PLUGIN_IS_SYSTEM
#error "PLUGIN_IS_SYSTEM is not defined - build this with ora/plugin.mk"
#endif

static void it_main(ora_lookup_fn_t ora_lookup_fn,
                    ora_plugin_type_t plugin_type,
                    const ora_entry_args_t *entry_args);
static void it_irq(ora_irq_t irq);

// Written out by hand because ORA_SYSTEM_PLUGIN and ORA_USER_PLUGIN don't set
// irq_handler.
ORA_SECTION(".plugin_header")
const ora_plugin_header_t ora_plugin_header = {
    .magic = ORA_PLUGIN_MAGIC,
    .api_version = ORA_PLUGIN_VERSION_1,
    .major_version = 0,
    .minor_version = 1,
    .patch_version = 0,
    .build_version = 0,
    .entry = it_main,
#if PLUGIN_IS_SYSTEM
    .plugin_type = ORA_PLUGIN_TYPE_SYSTEM,
#else
    .plugin_type = ORA_PLUGIN_TYPE_USER,
#endif
    .sam_usage = 255,
    .properties1 = ORA_PROPERTY1_SUPPORTS_YIELD,
    .min_fw_major_version = 0,
    .min_fw_minor_version = 8,
    .min_fw_patch_version = 1,
    .irq_handler = it_irq,
};

// The system plugin runs on core 1 and the user plugin on core 0.
#define IT_OWN_CORE  (PLUGIN_IS_SYSTEM ? 1u : 0u)

// ---------------------------------------------------------------------------
// Registers
//
// A plugin builds against firmware/ora alone, so it defines the registers it
// uses.  From the RP2350 datasheet.
// ---------------------------------------------------------------------------

#define IT_REG(addr)            (*(volatile uint32_t *)(addr))
#define IT_SET_ALIAS            0x2000u
#define IT_CLR_ALIAS            0x3000u

#define IT_SIO_BASE             0xD0000000u
#define IT_SIO_CPUID            IT_REG(IT_SIO_BASE + 0x000u)
#define IT_DOORBELL_OUT_SET     IT_REG(IT_SIO_BASE + 0x180u)
#define IT_DOORBELL_IN_CLR      IT_REG(IT_SIO_BASE + 0x18Cu)

// Each core has its own NVIC at the same addresses.
#define IT_NVIC_ISER(n)         IT_REG(0xE000E100u + (4u * (n)))
#define IT_NVIC_ISPR(n)         IT_REG(0xE000E200u + (4u * (n)))

// Each core's GPIO INTE, INTF and INTS registers start 0x48 bytes after the
// previous core's.
#define IT_IO_BANK0_BASE        0x40028000u
#define IT_GPIO_INTE(n)         (IT_IO_BANK0_BASE + 0x248u + (0x48u * IT_OWN_CORE) + (4u * (n)))
#define IT_GPIO_INTF(n)         (IT_IO_BANK0_BASE + 0x260u + (0x48u * IT_OWN_CORE) + (4u * (n)))
#define IT_GPIO_INTS(n)         (IT_IO_BANK0_BASE + 0x278u + (0x48u * IT_OWN_CORE) + (4u * (n)))

#define IT_TIMER0_BASE          0x400B0000u
#define IT_TIMER1_BASE          0x400B8000u
#define IT_TIMER_ALARM(base, n) ((base) + 0x10u + (4u * (n)))
#define IT_TIMER_ARMED(base)    ((base) + 0x20u)
#define IT_TIMER_TIMERAWL(base) ((base) + 0x28u)
#define IT_TIMER_INTR(base)     ((base) + 0x3Cu)
#define IT_TIMER_INTE(base)     ((base) + 0x40u)

// ---------------------------------------------------------------------------
// Doorbells
//
// Each core has 8 incoming doorbells, rung by the other core.
// ---------------------------------------------------------------------------

// Barriers alternate between two doorbells.  An instance can't pass barrier
// N + 1 until the other instance has passed barrier N, so a barrier's doorbell
// is never rung again before it is consumed.
#define IT_BELL_SYNC_EVEN       (1u << 0)
#define IT_BELL_SYNC_ODD        (1u << 1)

// Rung while holding exclusive mode.
#define IT_BELL_HOLD            (1u << 2)

// Doorbells 4 to 7 contain the user instance's verdict, sent in one write.
#define IT_BELL_VERDICT_SHIFT   4u
#define IT_BELL_VERDICT_MASK    0xFu

// ---------------------------------------------------------------------------
// Verdicts, as status LED flash counts
//
// The failures run roughly in the order the checks do.  Not receiving from the
// other instance is numbered after the checks, so a failure that made one
// instance stop early shows ahead of the other instance's wait for it.
// ---------------------------------------------------------------------------

#define IT_PASS                 1u
#define IT_SOLO                 2u
#define IT_F_CLAIM              3u
#define IT_F_DELIVER            4u
#define IT_F_CORE               5u
#define IT_F_UNCLAIMED          6u
#define IT_F_CROSS              7u
#define IT_F_GPIO_UNCLAIMED     8u
#define IT_F_GPIO_CLAIMED       9u
#define IT_F_TIMER              10u
#define IT_F_EXCL_ERR           11u
#define IT_F_EXCL_BOTH          12u
#define IT_F_NO_PEER            13u
#define IT_F_NO_VERDICT         14u

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

// Long enough to cover the other instance's checks before the first barrier.
#define IT_TIMEOUT_US           5000000u

// A pending interrupt is taken within a few cycles.  This leaves room for the
// firmware's dispatch.
#define IT_SETTLE_US            10u

// Exclusive-mode rounds.  With the timer interrupt slowing the firmware's
// delays, this runs for about two minutes.
#define IT_ROUNDS               200u

// The timer alarm interrupt's period.  Short, so an interrupt often arrives
// while the other core is starting to pause.
#define IT_TIMER_PERIOD_US      3u

// Doorbell rings while holding exclusive mode.
#define IT_HOLD_RINGS           2000u

#define IT_FLASH_US             200000u
#define IT_GAP_US               1500000u

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

typedef struct {
    ora_yield_fn_t yield;
    ora_claim_hw_fn_t claim_hw;
    ora_set_irq_enabled_fn_t set_irq_enabled;
    ora_claim_gpio_fn_t claim_gpio;
    ora_enter_exclusive_mode_fn_t enter_exclusive;
    ora_exit_exclusive_mode_fn_t exit_exclusive;
    ora_set_status_led_fn_t set_led;
    ora_get_flash_slot_count_fn_t slot_count;
} it_api_t;

static it_api_t it_api;

// Written by the interrupt handler.
static volatile uint32_t it_bells;
static volatile uint8_t it_spare_calls[6];
static volatile uint32_t it_gpio_calls;
static volatile uint32_t it_timer_calls;
static volatile uint8_t it_wrong_core;
static volatile uint8_t it_stray;

// This instance's GPIO, as its interrupt register and edge-high bit.
static uint32_t it_gpio_reg;
static uint32_t it_gpio_bit;

// This instance's timer alarm.
static volatile uint32_t it_alarm_irq = ORA_IRQ_INVALID;
static uint32_t it_timer_base;
static uint32_t it_alarm;
static uint32_t it_timer_target;

static uint32_t it_barriers;

// The result, for a debug probe.  Its address is in the ELF's symbol table.
typedef struct {
    uint32_t verdict;
    uint32_t own;
    uint32_t peer;
    uint32_t rounds;
    uint32_t timer_calls;
} it_report_t;

volatile it_report_t it_report __attribute__((used));

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static uint32_t it_now(void) {
    return IT_REG(IT_TIMER_TIMERAWL(IT_TIMER0_BASE));
}

static void it_wait_us(uint32_t us) {
    uint32_t start = it_now();
    while ((it_now() - start) < us) {
        it_api.yield(NULL);
    }
}

static uint32_t it_first(uint32_t verdict, uint32_t next) {
    return (verdict != 0u) ? verdict : next;
}

static uint8_t it_nvic_enabled(uint32_t irq) {
    return (IT_NVIC_ISER(irq >> 5) >> (irq & 31u)) & 1u;
}

static void it_nvic_enable(uint32_t irq) {
    IT_NVIC_ISER(irq >> 5) = 1u << (irq & 31u);
}

static void it_pend(uint32_t irq) {
    IT_NVIC_ISPR(irq >> 5) = 1u << (irq & 31u);
    __asm volatile ("dsb\n\tisb" : : : "memory");
    it_wait_us(IT_SETTLE_US);
}

static uint8_t it_claim_irq(uint32_t irq) {
    return it_api.claim_hw(ORA_HW_IRQ, &irq, 0u) == ORA_RESULT_OK;
}

static void it_park(void) __attribute__((noreturn));
static void it_park(void) {
    while (1) {
        if (it_api.yield != NULL) {
            it_api.yield(NULL);
        }
    }
}

// Arms the timer alarm for it_timer_target, or for IT_TIMER_PERIOD_US from now
// if that has passed.  An alarm fires only when the timer equals its target,
// so one armed for a time already gone never fires.  If the timer passed the
// target while it was being armed, and the alarm didn't fire, it is armed
// again.
static void it_timer_arm(void) {
    uint32_t rawl = IT_TIMER_TIMERAWL(it_timer_base);
    uint32_t bit = 1u << it_alarm;
    while (1) {
        if ((int32_t)(it_timer_target - IT_REG(rawl)) <= 0) {
            it_timer_target = IT_REG(rawl) + IT_TIMER_PERIOD_US;
        }
        IT_REG(IT_TIMER_ALARM(it_timer_base, it_alarm)) = it_timer_target;
        if (((int32_t)(IT_REG(rawl) - it_timer_target) < 0)
            || !(IT_REG(IT_TIMER_ARMED(it_timer_base)) & bit)) {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Interrupt handler
// ---------------------------------------------------------------------------

static void it_irq(ora_irq_t irq) {
    if (IT_SIO_CPUID != IT_OWN_CORE) {
        it_wrong_core = 1u;
    }

    if (irq == ORA_IRQ_SIO_IRQ_BELL) {
        uint32_t bells = IT_DOORBELL_IN_CLR;
        IT_DOORBELL_IN_CLR = bells;
        __atomic_fetch_or(&it_bells, bells, __ATOMIC_RELAXED);
    } else if (irq == ORA_IRQ_IO_IRQ_BANK0) {
        // Only this instance's GPIO.  The firmware and the other plugin use
        // others.
        if (IT_REG(IT_GPIO_INTS(it_gpio_reg)) & it_gpio_bit) {
            IT_REG(IT_GPIO_INTF(it_gpio_reg) + IT_CLR_ALIAS) = it_gpio_bit;
            it_gpio_calls++;
        }
    } else if (irq == it_alarm_irq) {
        IT_REG(IT_TIMER_INTR(it_timer_base)) = 1u << it_alarm;
        it_timer_target += IT_TIMER_PERIOD_US;
        it_timer_arm();
        it_timer_calls++;
    } else if ((irq >= ORA_IRQ_SPARE_IRQ_0) && (irq <= ORA_IRQ_SPARE_IRQ_5)) {
        it_spare_calls[irq - ORA_IRQ_SPARE_IRQ_0]++;
    } else {
        it_stray = 1u;
    }
}

// ---------------------------------------------------------------------------
// Doorbell exchange
// ---------------------------------------------------------------------------

// Rings the other instance's barrier doorbell and waits for it to ring this
// one's.  Returns 0 on timing out.
static uint8_t it_barrier(void) {
    uint32_t bell = (it_barriers++ & 1u) ? IT_BELL_SYNC_ODD : IT_BELL_SYNC_EVEN;
    IT_DOORBELL_OUT_SET = bell;
    uint32_t start = it_now();
    while (!(it_bells & bell)) {
        it_api.yield(NULL);
        if ((it_now() - start) > IT_TIMEOUT_US) {
            return 0u;
        }
    }
    __atomic_fetch_and(&it_bells, ~bell, __ATOMIC_RELAXED);
    return 1u;
}

#if PLUGIN_IS_SYSTEM
// Waits for the user instance's verdict.  Returns 0 on timing out.  The user
// instance may be timing out on a barrier first, so this waits for longer.
static uint32_t it_collect_verdict(void) {
    uint32_t start = it_now();
    uint32_t verdict;
    while ((verdict = (it_bells >> IT_BELL_VERDICT_SHIFT) & IT_BELL_VERDICT_MASK) == 0u) {
        it_api.yield(NULL);
        if ((it_now() - start) > (2u * IT_TIMEOUT_US)) {
            return 0u;
        }
    }
    return verdict;
}
#endif

// ---------------------------------------------------------------------------
// Checks
//
// Each returns 0 on success, or a failure verdict.
// ---------------------------------------------------------------------------

// Claims and enables the doorbell interrupt.  The barriers depend on it.
static uint32_t it_setup_bells(void) {
    if (!it_claim_irq(ORA_IRQ_SIO_IRQ_BELL)
        || (it_api.set_irq_enabled(ORA_IRQ_SIO_IRQ_BELL, 1u, 0u) != ORA_RESULT_OK)) {
        return IT_F_CLAIM;
    }
    return 0u;
}

// A claimed and enabled interrupt reaches the handler once, and
// ORA_ID_SET_IRQ_ENABLED enables and disables it in this core's NVIC.
static uint32_t it_check_delivery(void) {
    uint32_t irq = PLUGIN_IS_SYSTEM ? ORA_IRQ_SPARE_IRQ_0 : ORA_IRQ_SPARE_IRQ_1;
    if (!it_claim_irq(irq)) {
        return IT_F_CLAIM;
    }
    if ((it_api.set_irq_enabled(irq, 1u, 0u) != ORA_RESULT_OK) || !it_nvic_enabled(irq)) {
        return IT_F_DELIVER;
    }
    it_pend(irq);
    if (it_spare_calls[irq - ORA_IRQ_SPARE_IRQ_0] != 1u) {
        return IT_F_DELIVER;
    }
    if ((it_api.set_irq_enabled(irq, 0u, 0u) != ORA_RESULT_OK) || it_nvic_enabled(irq)) {
        return IT_F_DELIVER;
    }
    return 0u;
}

// An interrupt enabled without a claim doesn't reach the handler, and the
// firmware disables it.
static uint32_t it_check_unclaimed(void) {
    uint32_t irq = PLUGIN_IS_SYSTEM ? ORA_IRQ_SPARE_IRQ_2 : ORA_IRQ_SPARE_IRQ_3;
    it_nvic_enable(irq);
    it_pend(irq);
    if ((it_spare_calls[irq - ORA_IRQ_SPARE_IRQ_0] != 0u) || it_nvic_enabled(irq)) {
        return IT_F_UNCLAIMED;
    }
    return 0u;
}

// A GPIO interrupt that fires with IO_IRQ_BANK0 unclaimed is disabled, and its
// force bit cleared, by the firmware.  Once claimed, it reaches the handler and the firmware leaves it
// enabled.  The interrupt is forced, so the GPIO is never driven.
static uint32_t it_check_gpio(void) {
    uint32_t gpio;
    ora_result_t result = ORA_RESULT_INVALID_ARG;
    for (gpio = 0u; gpio < 64u; gpio++) {
        result = it_api.claim_gpio(gpio, 0u);
        if (result != ORA_RESULT_IN_USE) {
            break;
        }
    }
    if (result != ORA_RESULT_OK) {
        return IT_F_CLAIM;
    }
    it_gpio_reg = gpio / 8u;
    it_gpio_bit = 1u << (((gpio % 8u) * 4u) + 3u);

    uint32_t inte = IT_GPIO_INTE(it_gpio_reg);
    uint32_t intf = IT_GPIO_INTF(it_gpio_reg);

    // Unclaimed.  The firmware already enables IO_IRQ_BANK0 on core 0, for
    // VBUS detection.
    it_nvic_enable(ORA_IRQ_IO_IRQ_BANK0);
    IT_REG(inte + IT_SET_ALIAS) = it_gpio_bit;
    IT_REG(intf + IT_SET_ALIAS) = it_gpio_bit;
    it_wait_us(IT_SETTLE_US);
    uint8_t left_enabled = (IT_REG(inte) & it_gpio_bit) != 0u;
    uint8_t left_forced = (IT_REG(intf) & it_gpio_bit) != 0u;
    IT_REG(intf + IT_CLR_ALIAS) = it_gpio_bit;
    IT_REG(inte + IT_CLR_ALIAS) = it_gpio_bit;
    if (left_enabled || left_forced || (it_gpio_calls != 0u)) {
        return IT_F_GPIO_UNCLAIMED;
    }

    // Claimed.  The handler clears the forced interrupt.
    if (!it_claim_irq(ORA_IRQ_IO_IRQ_BANK0)) {
        return IT_F_CLAIM;
    }
    if (it_api.set_irq_enabled(ORA_IRQ_IO_IRQ_BANK0, 1u, 0u) != ORA_RESULT_OK) {
        return IT_F_GPIO_CLAIMED;
    }
    IT_REG(inte + IT_SET_ALIAS) = it_gpio_bit;
    IT_REG(intf + IT_SET_ALIAS) = it_gpio_bit;
    it_wait_us(IT_SETTLE_US);
    left_enabled = (IT_REG(inte) & it_gpio_bit) != 0u;
    left_forced = (IT_REG(intf) & it_gpio_bit) != 0u;
    IT_REG(intf + IT_CLR_ALIAS) = it_gpio_bit;
    IT_REG(inte + IT_CLR_ALIAS) = it_gpio_bit;
    if (!left_enabled || left_forced || (it_gpio_calls != 1u)) {
        return IT_F_GPIO_CLAIMED;
    }

    // IO_IRQ_BANK0 stays claimed and enabled, so the README's VBUS step runs
    // with a plugin handling IO_IRQ_BANK0 on core 0.
    return 0u;
}

// An interrupt claimed by the system instance, enabled and fired on core 0,
// reaches neither handler and is disabled on core 0 only.
static uint32_t it_check_cross_core(void) {
    uint32_t irq = ORA_IRQ_SPARE_IRQ_4;
    uint32_t result = 0u;
#if PLUGIN_IS_SYSTEM
    if (!it_claim_irq(irq) || (it_api.set_irq_enabled(irq, 1u, 0u) != ORA_RESULT_OK)) {
        result = IT_F_CLAIM;
    }
#endif
    if (!it_barrier()) {
        return IT_F_NO_PEER;
    }
#if !PLUGIN_IS_SYSTEM
    it_nvic_enable(irq);
    it_pend(irq);
    if ((it_spare_calls[irq - ORA_IRQ_SPARE_IRQ_0] != 0u) || it_nvic_enabled(irq)) {
        result = IT_F_CROSS;
    }
#endif
    if (!it_barrier()) {
        return IT_F_NO_PEER;
    }
#if PLUGIN_IS_SYSTEM
    if ((result == 0u)
        && (!it_nvic_enabled(irq) || (it_spare_calls[irq - ORA_IRQ_SPARE_IRQ_0] != 0u))) {
        result = IT_F_CROSS;
    }
    it_api.set_irq_enabled(irq, 0u, 0u);
#endif
    return result;
}

// Claims a timer alarm and its interrupt, and fires it every
// IT_TIMER_PERIOD_US.
static uint32_t it_timer_start(void) {
    uint32_t alarm = ORA_HW_ANY;
    if (it_api.claim_hw(ORA_HW_TIMER_ALARM, &alarm, 0u) != ORA_RESULT_OK) {
        return IT_F_CLAIM;
    }
    // TIMER0_IRQ_0 to TIMER1_IRQ_3 are interrupts 0 to 7, in alarm order.
    if (!it_claim_irq(alarm)) {
        return IT_F_CLAIM;
    }
    it_timer_base = (alarm < 4u) ? IT_TIMER0_BASE : IT_TIMER1_BASE;
    it_alarm = alarm & 3u;
    it_alarm_irq = alarm;

    IT_REG(IT_TIMER_INTE(it_timer_base) + IT_SET_ALIAS) = 1u << it_alarm;
    it_timer_target = IT_REG(IT_TIMER_TIMERAWL(it_timer_base)) + IT_TIMER_PERIOD_US;
    it_timer_arm();
    if (it_api.set_irq_enabled(alarm, 1u, 0u) != ORA_RESULT_OK) {
        return IT_F_TIMER;
    }

    it_wait_us(1000u);
    if (it_timer_calls < 10u) {
        return IT_F_TIMER;
    }
    return 0u;
}

static void it_timer_stop(void) {
    if (it_alarm_irq == ORA_IRQ_INVALID) {
        return;
    }
    it_api.set_irq_enabled(it_alarm_irq, 0u, 0u);
    IT_REG(IT_TIMER_INTE(it_timer_base) + IT_CLR_ALIAS) = 1u << it_alarm;
    IT_REG(IT_TIMER_ARMED(it_timer_base)) = 1u << it_alarm;
    IT_REG(IT_TIMER_INTR(it_timer_base)) = 1u << it_alarm;
}

// Holding exclusive mode, rings the other instance's HOLD doorbell and
// watches for it to ring this one's.  The other instance should be paused
// with interrupts disabled, so a HOLD arriving means both instances are in
// exclusive mode at once.  Returns 1 if so.
static uint8_t it_hold(void) {
    // A HOLD rung during the other instance's turn arrived once this one
    // resumed.
    __atomic_fetch_and(&it_bells, ~IT_BELL_HOLD, __ATOMIC_RELAXED);
    for (uint32_t i = 0u; i < IT_HOLD_RINGS; i++) {
        IT_DOORBELL_OUT_SET = IT_BELL_HOLD;
        if (it_bells & IT_BELL_HOLD) {
            return 1u;
        }
    }
    return 0u;
}

// Both instances request exclusive mode at the same moment, IT_ROUNDS times,
// with the timer alarm interrupt firing on both cores throughout.
static uint32_t it_check_exclusive(void) {
    uint32_t timer_calls = it_timer_calls;
    for (uint32_t round = 0u; round < IT_ROUNDS; round++) {
        if (!it_barrier()) {
            return IT_F_NO_PEER;
        }
        // Delays this instance's request by a varying number of cycles, on
        // alternate rounds for each instance, so the requests meet across the
        // whole window in which they collide.
        if ((round & 1u) == IT_OWN_CORE) {
            for (volatile uint32_t i = 0u; i < (((round >> 1) % 32u) * 4u); i++) {
            }
        }
        if (it_api.enter_exclusive() != ORA_RESULT_OK) {
            return IT_F_EXCL_ERR;
        }
        uint8_t both = it_hold();
        if (it_api.exit_exclusive() != ORA_RESULT_OK) {
            return IT_F_EXCL_ERR;
        }
        if (both) {
            return IT_F_EXCL_BOTH;
        }
        // Waits, yielding, for the other instance's turn.
        if (!it_barrier()) {
            return IT_F_NO_PEER;
        }
        it_report.rounds = round + 1u;
    }
    if ((it_alarm_irq != ORA_IRQ_INVALID) && ((it_timer_calls - timer_calls) < IT_ROUNDS)) {
        return IT_F_TIMER;
    }
    return 0u;
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

#if PLUGIN_IS_SYSTEM
// Combines the two instances' verdicts.  The lower failure wins, as it is more
// likely to be the cause of the other.
static uint32_t it_combine(uint32_t own, uint32_t peer) {
    if (peer == 0u) {
        return (own > IT_SOLO) ? own : IT_F_NO_VERDICT;
    }
    uint32_t own_fail = (own > IT_SOLO) ? own : 0u;
    uint32_t peer_fail = (peer > IT_SOLO) ? peer : 0u;
    if (own_fail && peer_fail) {
        return (own_fail < peer_fail) ? own_fail : peer_fail;
    }
    if (own_fail || peer_fail) {
        return own_fail | peer_fail;
    }
    if ((own == IT_PASS) && (peer == IT_PASS)) {
        return IT_PASS;
    }
    // One instance reported SOLO with two plugin slots installed.
    return IT_F_NO_PEER;
}

static void it_blink(uint32_t verdict) __attribute__((noreturn));
static void it_blink(uint32_t verdict) {
    while (1) {
        for (uint32_t i = 0u; i < verdict; i++) {
            it_api.set_led(1u);
            it_wait_us(IT_FLASH_US);
            it_api.set_led(0u);
            it_wait_us(IT_FLASH_US);
        }
        it_wait_us(IT_GAP_US);
    }
}
#endif

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

static void it_main(ora_lookup_fn_t ora_lookup_fn,
                    ora_plugin_type_t plugin_type,
                    const ora_entry_args_t *entry_args) {
    (void)plugin_type;
    (void)entry_args;

    ORA_PLUGIN_INIT_STATIC_RAM();

    it_api.yield           = ora_lookup_fn(ORA_ID_YIELD);
    it_api.claim_hw        = ora_lookup_fn(ORA_ID_CLAIM_HW);
    it_api.set_irq_enabled = ora_lookup_fn(ORA_ID_SET_IRQ_ENABLED);
    it_api.claim_gpio      = ora_lookup_fn(ORA_ID_CLAIM_GPIO);
    it_api.enter_exclusive = ora_lookup_fn(ORA_ID_ENTER_EXCLUSIVE_MODE);
    it_api.exit_exclusive  = ora_lookup_fn(ORA_ID_EXIT_EXCLUSIVE_MODE);
    it_api.set_led         = ora_lookup_fn(ORA_ID_SET_STATUS_LED);
    it_api.slot_count      = ora_lookup_fn(ORA_ID_GET_FLASH_SLOT_COUNT);

    // min_fw_version keeps this plugin off firmware without these calls.  If
    // a lookup fails anyway, the LED stays on, as the firmware lit it at boot.
    if ((it_api.yield == NULL) || (it_api.claim_hw == NULL)
        || (it_api.set_irq_enabled == NULL) || (it_api.claim_gpio == NULL)
        || (it_api.enter_exclusive == NULL) || (it_api.exit_exclusive == NULL)
        || (it_api.set_led == NULL) || (it_api.slot_count == NULL)) {
        it_park();
    }

#if PLUGIN_IS_SYSTEM
    // Off while the checks run, so a hang shows as an unlit LED.
    it_api.set_led(0u);
#endif

    uint8_t paired = it_api.slot_count(ORA_FLASH_SLOT_FLAG_EXCLUDE_NON_PLUGINS) > 1u;

    uint32_t own = it_setup_bells();
    own = it_first(own, it_check_delivery());
    own = it_first(own, it_check_unclaimed());
    own = it_first(own, it_check_gpio());
    if (paired) {
        own = it_first(own, it_check_cross_core());
    }
    own = it_first(own, it_timer_start());
    if (paired) {
        own = it_first(own, it_check_exclusive());
    }
    it_timer_stop();
    if (it_wrong_core || it_stray) {
        own = it_first(own, IT_F_CORE);
    }
    if (own == 0u) {
        own = paired ? IT_PASS : IT_SOLO;
    }

    it_report.own = own;
    it_report.timer_calls = it_timer_calls;

#if PLUGIN_IS_SYSTEM
    uint32_t verdict = own;
    if (paired) {
        it_report.peer = it_collect_verdict();
        verdict = it_combine(own, it_report.peer);
    }
    it_report.verdict = verdict;
    it_blink(verdict);
#else
    // The system instance owns the LED.
    IT_DOORBELL_OUT_SET = own << IT_BELL_VERDICT_SHIFT;
    it_report.verdict = own;
    it_park();
#endif
}
