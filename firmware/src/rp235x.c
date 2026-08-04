// One ROM RP235X Specific Routines

// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#include "include.h"
#include "apio.h"

// Internal function prototypes
uint8_t calculate_pll_settings(
    rp235x_clock_config_t *clock_config,
    uint8_t overclock
);
void get_clock_config(rp235x_clock_config_t *config);
uint8_t get_vreg_from_target_mhz(uint16_t target_mhz);
void setup_xosc(void);
void setup_pll(rp235x_clock_config_t *config);
void setup_qmi(rp235x_clock_config_t *config);
void setup_vreg(rp235x_clock_config_t *config);
void setup_cp(void);
void final_checks(rp235x_clock_config_t *config);
uint16_t get_temp(void);

// RP2350 firmware needs a special boot block so the bootloader will load it.
// See datasheet S5.9.5 and ../include/reg-rp235x.h.
// It must be in the first 4KB of the flash firmware image.  This follows our
// reset vectors, which is fine.  Given we do not include a VECTOR_TABLE
// block, the bootloader assumes it is present at the start of flash - which it
// is.
#if !defined(TEST_BUILD)
__attribute__((section(".rp2350_block")))
#endif // !TEST_BUILD
const rp2350_boot_block_t rp2350_arm_boot_block = {
    .start_marker    = 0xffffded3,
    .image_type_tag  = 0x42,
    .image_type_len  = 0x1,
    .image_type_data = 0b0001000000100001,
    .type            = 0xff,
    .size            = 0x0001,
    .pad             = 0,
    .next_block      = 0,
    .end_marker      = 0xab123579
};

void platform_specific_init(void) {
#if !defined(TEST_BUILD)
    // RP235X needs to reset the JTAG interface to enable SWD (for example for
    // RTT logging)
    RESET_RESET |= RESET_JTAG;
    RESET_RESET &= ~RESET_JTAG;
    while (!(RESET_DONE & RESET_JTAG));
    DEBUG("JTAG reset complete");

    RESET_RESET |= RESET_SYSINFO;
    RESET_RESET &= ~RESET_SYSINFO;
    while (!(RESET_DONE & RESET_SYSINFO));
    DEBUG("SYSINFO reset complete");

    // Retrieve the board type
    if (!SYSINFO_IS_QFN60()) {
        RUNTIME->rp235x = RP235XB;
    } 
#else 
extern uint8_t stub_rp235x_is_b;
    RUNTIME->rp235x = stub_rp235x_is_b ? RP235XB : RP235XA;
#endif
}

#if !defined(TEST_BUILD)
// Set up interrupt to fire when VBUS sensed on PA9
void setup_vbus_interrupt(void) {
    // Check we have the information required to enable DFU
    if (!HW->usb_capable) {
        DEBUG("No USB");
    }
    uint8_t vbus_pin = HW->gpio_vbus;
    if (vbus_pin >= MAX_GPIOS) {
        LOG("No VBUS detect pin");
        return;
    }

    // Enable VBUS detect interrupt
    GPIO_CTRL(vbus_pin) = GPIO_CTRL_RESET;      // Enable SIO
    uint32_t reg_offset = vbus_pin / 8;         // Which INTEx register (0-3)
    uint32_t bit = ((vbus_pin % 8) * 4) + 3;    // Bit within that register
    volatile uint32_t *inte = &IO_BANK0_PROC0_INTE0 + reg_offset;
    volatile uint32_t *intr = &IO_BANK0_INTR0 + reg_offset;
    *inte |= (1 << bit);                        // Enable rising edge interrupt
    *intr = (1 << bit);                         // Clear any pending
    NVIC_ISER0 |= (1 << IO_IRQ_BANK0);          // Enable IO_BANK0 interrupt in NVIC

    // Set as input, pull-down, output disable
    GPIO_PAD(vbus_pin) |= (PAD_PD | PAD_OUTPUT_DISABLE | PAD_INPUT);

    // Wait for pull-down to settle.  Using same delay as STM32 implementation.
    for (volatile int ii = 0; ii < 1000; ii++);

    // Check if VBUS already present
    if (GPIO_READ(vbus_pin)) {
        LOG("VBUS already present - entering bootloader");
        for (volatile int ii = 0; ii < 1000000; ii++);
        enter_bootloader();
    }
}

// VBUS interrupt Handler
void vbus_connect_handler(void) {
    // Clear the interrupt
    uint8_t vbus_pin = HW->gpio_vbus;
    uint32_t reg_offset = vbus_pin / 8;
    uint32_t bit = ((vbus_pin % 8) * 4) + 3;
    volatile uint32_t *intr = &IO_BANK0_INTR0 + reg_offset;
    *intr = (1 << bit);

    // Disable interrupts before logging
    __asm volatile("cpsid i");

        // Log and pause for log to complete
    LOG("VBUS detected - entering bootloader");
    for (volatile int ii = 0; ii < 1000000; ii++);

    enter_bootloader();
}
#endif // !TEST_BUILD

uint8_t calculate_pll_settings(
    rp235x_clock_config_t *config,
    uint8_t overclock
) {
    const uint32_t XOSC_MHZ = 12;
    const uint8_t REFDIV = 1;

    (void)overclock;
    
    uint32_t target_freq_mhz = config->sys_clock_freq_mhz;

    if ((target_freq_mhz > RP235X_STOCK_CLOCK_SPEED_MHZ) && (!overclock)) {
        ERR("Requested frequency %dMHz exceeds max %dMHz - cannot calculate PLL",
            target_freq_mhz, RP235X_STOCK_CLOCK_SPEED_MHZ);
        return 0;
    }
    
    uint32_t vco_min = 750;
    uint32_t vco_max = 1600;
    
    // Try POSTDIV combinations (prefer higher PD1:PD2 ratios)
    uint32_t best_error = UINT32_MAX;
    uint8_t rc = 0;
    for (uint8_t pd2 = 1; pd2 <= 7; pd2++) {
        for (uint8_t pd1 = 1; pd1 <= 7; pd1++) {
            uint32_t divisor = pd1 * pd2;
            uint32_t vco_mhz = target_freq_mhz * divisor;
            
            uint32_t fbdiv = (vco_mhz + 6) / XOSC_MHZ;  // Round to nearest
            
            if (fbdiv >= 16 && fbdiv <= 320) {
                uint32_t actual_vco = XOSC_MHZ * fbdiv;
                if (actual_vco >= vco_min && actual_vco <= vco_max) {
                    uint32_t target_vco = target_freq_mhz * divisor;
                    uint32_t error = (actual_vco > target_vco) ? 
                                    (actual_vco - target_vco) : 
                                    (target_vco - actual_vco);
                                    
                    if (error < best_error) {
                        best_error = error;
                        config->pll_refdiv = REFDIV;
                        config->pll_sys_fbdiv = (uint16_t)fbdiv;
                        config->pll_sys_postdiv1 = pd1;
                        config->pll_sys_postdiv2 = pd2;
                        rc = 1;
                    }
                }
            }
        }
    }
    
    return rc;
}

uint8_t get_vreg_from_target_mhz(uint16_t target_mhz) {
    uint8_t vreg = FIRE_VREG_1_10V;
    
    // These are conservative values.  The RP235X accepts values up to 3.30V.
    // Higher values may be required for very high overclocks, but may also
    // damage the chip or reduce its lifespan.
    //
    // To use custom VREG settngs, use firmware overrides in the ROM config.
    if (target_mhz >= 500) {
        vreg = FIRE_VREG_1_60V;
    } else if (target_mhz >= 450) {
        vreg = FIRE_VREG_1_50V;
    } else if (target_mhz >= 425) {
        vreg = FIRE_VREG_1_40V;
    } else if (target_mhz >= 400) {
        vreg = FIRE_VREG_1_30V;
    } else if (target_mhz >= 375) {
        vreg = FIRE_VREG_1_25V;
    } else if (target_mhz >= 340) {
        vreg = FIRE_VREG_1_20V;
    } else if (target_mhz > 300) {
        vreg = FIRE_VREG_1_15V;
    }

    return vreg;
}

// Figures out the PLL and VREG configuration based on the combination of
// compile time info and any ROM set overrides.
void get_clock_config(rp235x_clock_config_t *config) {
    if (RUNTIME->fire_freq == FIRE_FREQ_NONE) {
        // Use compile time setting if not overridden
        config->sys_clock_freq_mhz = TARGET_FREQ_MHZ;
    } else if (RUNTIME->fire_freq == FIRE_FREQ_STOCK) {
        // Use stock speed (150MHz) if requested
        config->sys_clock_freq_mhz = RP235X_STOCK_CLOCK_SPEED_MHZ;
    } else if (RUNTIME->fire_freq < RP235X_MAX_CONFIGURABLE_MHZ) {
        config->sys_clock_freq_mhz = RUNTIME->fire_freq;
    } else {
        ERR("Freq too high %d/%dMHz - using default", RUNTIME->fire_freq, RP235X_MAX_CONFIGURABLE_MHZ);
        config->sys_clock_freq_mhz = RP235X_STOCK_CLOCK_SPEED_MHZ;
    }

    // Check for overclocking enabled
    if (config->sys_clock_freq_mhz > RP235X_STOCK_CLOCK_SPEED_MHZ) {
        if (RUNTIME->overclock_enabled) {
            LOG("OC - %dMHz", config->sys_clock_freq_mhz);
        } else {
            ERR("No OC - cap %dMHz", RP235X_STOCK_CLOCK_SPEED_MHZ);
            config->sys_clock_freq_mhz = RP235X_STOCK_CLOCK_SPEED_MHZ;
        }
    }

    // Calculate PLL settings, to get as close to target frequency as possible.
    // This can fail for very low and very high frequencies.
    if (!calculate_pll_settings(
        config,
        RUNTIME->overclock_enabled
    )) {
        ERR("No valid PLL - using CT %dMHz", TARGET_FREQ_MHZ);
        config->sys_clock_freq_mhz = TARGET_FREQ_MHZ;  
        config->pll_refdiv = PLL_SYS_REFDIV;
        config->pll_sys_fbdiv = PLL_SYS_FBDIV;
        config->pll_sys_postdiv1 = PLL_SYS_POSTDIV1;
        config->pll_sys_postdiv2 = PLL_SYS_POSTDIV2;
    }

    // Set VREG
    if ((RUNTIME->fire_vreg != FIRE_VREG_STOCK) && (RUNTIME->fire_vreg != FIRE_VREG_NONE)) {
        // Overriding VREG
        config->vreg = RUNTIME->fire_vreg;
    } else {
        // Using calculated VREG
        config->vreg = get_vreg_from_target_mhz(config->sys_clock_freq_mhz);
    }

    DEBUG("Clock to %dMHz: refdiv=%d, fbdiv=%d, postdiv1=%d, postdiv2=%d, vreg=%d",
        config->sys_clock_freq_mhz,
        config->pll_refdiv,
        config->pll_sys_fbdiv,
        config->pll_sys_postdiv1,
        config->pll_sys_postdiv2,
        config->vreg
    );

    RUNTIME->sysclk_mhz = config->sys_clock_freq_mhz;
}

void setup_clock(void) {
    rp235x_clock_config_t config;
    get_clock_config(&config);

    setup_xosc();
    setup_qmi(&config);
    setup_vreg(&config);
    setup_pll(&config);
    setup_cp();
    final_checks(&config);
}

// Perform initial GPIO setup, which involves the image select pins and the
// status LED pin(s).
//
// The metadata is valid by this popint.
void setup_initial_gpios(void) {
    DEBUG("Init GPIO");

    // Initialize APIO GPIO support.  A no-op on metal, but necessary when
    // emulating in case we repeatedly call firmware_main().
    APIO_GPIO_INIT();

#if REAL_HARDWARE
    // Take IO bank and pads bank out of reset
    RESET_RESET &= ~(RESET_IOBANK0 | RESET_PADS_BANK0);
    while (!(RESET_DONE & (RESET_IOBANK0 | RESET_PADS_BANK0)));
#endif // REAL_HARDWARE

    // Initialize all pins to input only, no pulls
    for (int ii = 0; ii < MAX_GPIOS; ii++) {
        APIO_GPIO_PULL_NONE(ii);
        APIO_GPIO_INPUT_ONLY(ii);
    }

#if REAL_HARDWARE
    // Set up the status LED pin (output, driven high = LED off).
    setup_status_led();

    // If there's a neo-pixel LED, set it up as an output pin, high.  Note
    // the neopixel LED might be the same as the regular status LED, in which
    // case we will have already configured it.
    if ((HW->gpio_neopixel < MAX_GPIOS) && (HW->gpio_neopixel != HW->gpio_status)) {
        uint8_t pin = HW->gpio_neopixel;
        GPIO_PAD(pin) &= ~(PAD_OUTPUT_DISABLE | PAD_INPUT);
        GPIO_PAD(pin) |= PAD_DRIVE(PAD_DRIVE_4MA);
        SIO_GPIO_OE_SET_PIN(pin);
        SIO_GPIO_OUT_SET_PIN(pin);
    }
#endif // REAL_HARDWARE
}

#if REAL_HARDWARE
// Reconfigure flash (QMI) speed if required
void setup_qmi(rp235x_clock_config_t *config) {
#if TARGET_FREQ_MHZ > (MAX_FLASH_CLOCK_FREQ_MHZ * 256)
#error "Flash divider > 256 not supported by the hardware"
#endif
    uint16_t target_flash_freq_mhz = config->sys_clock_freq_mhz;
    if (target_flash_freq_mhz > MAX_FLASH_CLOCK_FREQ_MHZ) {
        DEBUG("Target freq > max flash %dv%dMHz", target_flash_freq_mhz, MAX_FLASH_CLOCK_FREQ_MHZ);

        // Calculate the divider
        uint8_t divider = target_flash_freq_mhz / MAX_FLASH_CLOCK_FREQ_MHZ;
        if (target_flash_freq_mhz % MAX_FLASH_CLOCK_FREQ_MHZ) {
            divider += 1;
        }

        uint32_t m0 = XIP_QMI_M0_TIMING;
        DEBUG("Current QMI M0: 0x%08X", m0);

        m0 &= ~XIP_QMI_M0_CLKDIV_MASK;
        m0 |= (divider & XIP_QMI_M0_CLKDIV_MASK) << XIP_QMI_M0_CLKDIV_SHIFT;

        DEBUG("Update M0 clkdiv: %d", divider);
        DEBUG("Update QMI M0: 0x%08X", m0);

        XIP_QMI_M0_TIMING = m0;
    }
}

void setup_vreg(rp235x_clock_config_t *config) {
    uint32_t vreg_ctrl = POWMAN_VREG_CTRL;
    uint32_t vreg = POWMAN_VREG;
    uint8_t voltage = config->vreg;
    DEBUG("Current VREG_CTRL: 0x%08X", vreg_ctrl);
    DEBUG("Current VREG_STATUS: 0x%08X", POWMAN_VREG_STATUS);
    DEBUG("Current VREG: 0x%08X", vreg);
    DEBUG("Target VREG setting: %d", voltage);

    if (voltage > 0b11111) {
        ERR("Invalid VREG %d - ignore", voltage);
        return;
    }

    if (config->vreg != FIRE_VREG_1_10V) {
        uint8_t high_temp = HT_TH_100;
        uint8_t unlimited_voltage = 0;
        if (config->vreg > FIRE_VREG_1_30V) {
            unlimited_voltage = 1;
        }

        DEBUG("Unlock VREG");
        vreg_ctrl |= POWMAN_PASSWORD |
                POWMAN_VREG_CTRL_UNLOCK;
        POWMAN_VREG_CTRL = vreg_ctrl;
        while (!(POWMAN_VREG_CTRL & POWMAN_VREG_CTRL_UNLOCK));

        if (unlimited_voltage) {
            ERR("Disable voltage limit");
            vreg_ctrl |= POWMAN_VREG_CTRL_DISABLE_VOLTAGE_LIMIT;
            POWMAN_VREG_CTRL = vreg_ctrl;
            while (!(POWMAN_VREG_CTRL & POWMAN_VREG_CTRL_DISABLE_VOLTAGE_LIMIT));
        }

        DEBUG("Set VREG high temp %d", high_temp);
        vreg_ctrl &= ~(HT_TH_MASK << HT_TH_SHIFT);
        vreg_ctrl |= POWMAN_PASSWORD |
                        POWMAN_VREG_CTRL_HT_TH(high_temp);
        POWMAN_VREG_CTRL = vreg_ctrl;
        DEBUG("Current VREG_CTRL: 0x%08X", POWMAN_VREG_CTRL);

        DEBUG("Set VREG to %d", voltage);
        while (POWMAN_VREG & POWMAN_VREG_UPDATE);
        vreg &= ~(VREG_MASK << VREG_SHIFT);
        vreg |= POWMAN_VREG_VOLTAGE(voltage) | POWMAN_PASSWORD;
        POWMAN_VREG = vreg;
        while (POWMAN_VREG & POWMAN_VREG_UPDATE);

        DEBUG("POWMAN_VREG: 0x%08X", POWMAN_VREG);

        for (volatile int ii = 0; ii < 5000; ii++) {
            // Wait a bit for the voltage to stabilise
            // 2,000 loops is too few at 540MHz, 5,000 seems like enough
            // Probabyl not required if DEBUG logging is on
        }
    } 
}

// Set up the PLL with the generated values
void setup_pll(rp235x_clock_config_t *config) {
    // Release PLL_SYS from reset
    RESET_RESET &= ~RESET_PLL_SYS;
    while (!(RESET_DONE & RESET_PLL_SYS));

    // Power down the PLL, set the feedback divider
    PLL_SYS_PWR = PLL_PWR_PD | PLL_PWR_VCOPD;

    // Set feedback divider and reference divider
    PLL_SYS_FBDIV_INT = config->pll_sys_fbdiv;
    PLL_SYS_CS = PLL_CS_REFDIV(config->pll_refdiv);

    // Power up VCO (keep post-dividers powered down)
    PLL_SYS_PWR = PLL_PWR_POSTDIVPD;

    // Wait for PLL to lock
    while (!(PLL_SYS_CS & PLL_CS_LOCK));

    // Set post dividers and power up everything
    PLL_SYS_PRIM = PLL_PRIM_POSTDIV1(config->pll_sys_postdiv1) |
                     PLL_PRIM_POSTDIV2(config->pll_sys_postdiv2);

    // Power up post dividers
    PLL_SYS_PWR = 0;

    // Switch to the PLL
    CLOCK_SYS_CTRL = CLOCK_SYS_SRC_AUX | CLOCK_SYS_AUXSRC_PLL_SYS;
    while ((CLOCK_SYS_SELECTED & (1 << 1)) == 0);
}

void setup_usb_controller(void) {
    // Route USB clock to PLL_USB
    CLOCK_CLK_USB_CTRL = CLOCK_USB_CTRL_ENABLE | CLOCK_USB_CTRL_AUXSRC_PLL_USB;

    // Release USB controller from reset
    RESET_RESET &= ~RESET_USBCTRL;
    while (!(RESET_DONE & RESET_USBCTRL)) {}
}

void setup_usb_pll(void) {
    if (RUNTIME->peri_en & 1) {
        DEBUG("USB PLL already enabled");
        return;
    }

    DEBUG("Setting up USB PLL");

    // Release PLL_USB from reset
    RESET_RESET &= ~RESET_PLL_USB;
    while (!(RESET_DONE & RESET_PLL_USB));

    // Power down the PLL, set the feedback divider
    PLL_USB_PWR = PLL_PWR_PD | PLL_PWR_VCOPD;

    // For 48MHz: 12MHz × 40 ÷ 5 ÷ 2 = 48MHz
    PLL_USB_FBDIV_INT = 40;
    PLL_USB_CS = PLL_CS_REFDIV(1);

    // Power up VCO (keep post-dividers powered down)
    PLL_USB_PWR = PLL_PWR_POSTDIVPD;

    // Wait for lock
    while (!(PLL_USB_CS & PLL_CS_LOCK));

    // Set post dividers: 40 × 12MHz = 480MHz → ÷5 ÷2 = 48MHz
    PLL_USB_PRIM = PLL_PRIM_POSTDIV1(5) | PLL_PRIM_POSTDIV2(2);

    // Power up
    PLL_USB_PWR = 0;
    RUNTIME->peri_en |= 1;
}

void setup_adc(void) {
    if (RUNTIME->peri_en & (1 << 1)) {
        DEBUG("ADC already enabled");
        return;
    }

    DEBUG("Setting up ADC");

    // Route USB PLL to ADC (USB is the default source so no need to set)
    CLOCK_ADC_CTRL |= CLOCK_ADC_ENABLE;
    while (!(CLOCK_ADC_CTRL & CLOCK_ADC_ENABLED));
    DEBUG("ADC clock enabled");

    // Take ADC out of reset
    RESET_RESET &= ~(RESET_ADC);
    while (!(RESET_DONE & RESET_ADC));

    // Enable ADC and temperature sensor
    DEBUG("ADC out of reset");
    ADC_CS |= ADC_CS_TS_EN | ADC_CS_EN;
    while (!(ADC_CS & ADC_CS_READY));          
    RUNTIME->peri_en |= (1 << 1);

    DEBUG("ADC ready");
}

uint16_t get_temp(void) {
    // Start a conversion
    ADC_CS |= ADC_CS_AINSEL(ADC_CS_TS);
    ADC_CS |= ADC_CS_START_ONCE;

    // Wait for it to complete
    while (!(ADC_CS & ADC_CS_READY));

    // Return the result
    return (uint16_t)(ADC_RESULT & ADC_RESULT_MASK);
}
#endif // REAL_HARDWARE

void final_checks(rp235x_clock_config_t *config) {
    if (config->sys_clock_freq_mhz > 300) {
        DEBUG("!!!Extreme overlocking - enabling and reading temp sensor");

        // USB clock required for ADC
        setup_usb_pll();

        // Set up ADC
        setup_adc();

        // Take a reading
        uint16_t temp = get_temp();
        (void)temp;  // In case not logged

        ERR("Temperature sensor reading: 0x%03X", temp);
    }
}

#if !defined(TEST_BUILD)
void setup_cp(void) {
#if defined(RP_USE_CP)
    // Enable Coprocessor 0 to enable MCR instructions
    SCB_CPACR &= ~(0b11 << 0);
    SCB_CPACR |= SCB_CPACR_CP0_FULL;
    __asm volatile ("dsb");
    __asm volatile ("isb");
    DEBUG("CP0 enabled");
#endif // RP_USE_CP
}
#endif // !TEST_BUILD

void setup_mco(void) {
    ERR("MCO not supported on RP235X");
}

#if !defined(TEST_BUILD)
// Set up the image select pins to be inputs with the appropriate pulls.
//
// As of 0.6.0 sel_jumper_pulls is a bit field indicating whether the
// jumper pulls up (1) or down (0) each sel pin individually.
//
// As of 0.6.2 moved to uint64_t to cope with RP2350B.
uint32_t setup_sel_pins(uint64_t *sel_mask, uint64_t *flip_bits) {
    uint32_t num;
    uint32_t pad;

    // Initialize outputs
    *sel_mask = 0;
    *flip_bits = 0;

    num = 0;
    for (int ii = 0; (ii < MAX_IMG_SEL_PINS); ii++) {
        uint8_t pin = HW->gpio_sel[ii];
        
        if (pin >= MAX_GPIOS) {
            // Ignore invalid pins
            continue;
        }
        
        if ((pin == HW->gpio_swclk) ||
            (pin == HW->gpio_swdio)) {
            DEBUG("Pin %d = SWD, disable", pin);

            SYSCFG_DBGFORCE |= SYSCFG_DBGFORCE_ATTACH_BIT;
            
            if (pin == HW->gpio_swclk) {
                GPIO_PAD(SWCLK_PAD) = (1 << PAD_ISO_BIT);
            }
            if (pin == HW->gpio_swdio) {
                GPIO_PAD(SWDIO_PAD) = (1 << PAD_ISO_BIT);
            }
        }
        
        if (pin < MAX_GPIOS) {
            // Set the appropriate pad value based on the bit field
            if (HW->sel_jumper_pull & (1 << ii)) {
                // This pin pulls up, so we pull down
                DEBUG("Pin %d PD", pin);
                pad = PAD_INPUT_PD;
            } else {
                // This pin pulls down, so we pull up
                DEBUG("Pin %d PU", pin);
                pad = PAD_INPUT_PU;

                // Flip this bit when reading the SEL pins, as closing will
                // pull the pin low, but that should read a
                *flip_bits |= (1ULL << pin);
            }

            // Enable pull
            GPIO_PAD(pin) = pad;

            // Set the pin in our bit mask
            *sel_mask |= (1ULL << pin);

            num += 1;
        } else if (pin != 0xFF) {
            ERR("Pin %d >= %d - ignore", pin, MAX_GPIOS);
        }
    }

    // Short delay to allow the pulls to settle.
    for(volatile int ii = 0; ii < 10; ii++);

    return num;
}

// Get the value of the sel pins.
// 
// As of 0.6.0, we support sel_jumper_pulls as a bit field indicating whether
// each individual sel pin's jumper pulls up (1) or down (0).
//
// If a pull is low (i.e. closing the jumpers pulls them up) we return the
// value as is, as closed should indicate 1.  In the other case, where MCU
// pulls are high (closing jumpers) pulls the pins low, we invert - so closed
// still indicates 1.
uint64_t get_sel_value(uint64_t sel_mask, uint64_t flip_bits) {
    uint64_t gpio_value = 0;

    // Read GPIO input register.  We read multiple times to allow for any
    // spurious "highs", as some pins that the sel pin connected to might
    // ocassionally glitch high.  A case in point is BOOT, which is shared
    // with QSPI_SS.  This will mostly be low, as it is the main external
    // flash chip select, and seems to always read low, but could go high
    // if, for some reason, flash isn't busy.
    //
    // The logic below is as it is because in this case the spurious high
    // ends up being a spuripous low after flipping (cos closing that jumper
    // pulls the pin low).
    //
    // This isn't totally robust.  Scoping One ROM during this stage shows
    // that QSPI_SS is almost always low, but it does glitch high every 40us,
    // for perhaps 100ns, so there is a change of misreading.  If this turns
    // out to be a problem, we should run this from RAM, disable XIP and
    // isolate the QSPI_SS pad (like we do SWD pads).  Or, force some explicit
    // flash reads, or even just take more votes.  I'm hoping that's not
    // necessary.

    // Take 10-20 samples spread over ~1us to avoid any single glitch.
    // At 150MHz, this is negligible cost (<150 cycles total).
    for (int i = 0; i < 15; i++) {
        uint32_t low_gpios = SIO_GPIO_IN;
        uint32_t high_gpios = SIO_GPIO_HI_IN;
        uint64_t gpios = ((uint64_t)high_gpios << 32) | low_gpios;
        gpio_value |= (gpios ^ flip_bits);
    }

    // Mask to just the sel pins
    gpio_value &= sel_mask;

    return gpio_value;
}

void disable_sel_pins(void) {
    for (int ii = 0; (ii < MAX_IMG_SEL_PINS); ii++) {
        uint8_t pin = HW->gpio_sel[ii];
        if (pin < MAX_GPIOS) {
            // Disable pulls
            GPIO_PAD(pin) &= ~(PAD_PU | PAD_PD);

            SYSCFG_DBGFORCE &= ~SYSCFG_DBGFORCE_ATTACH_BIT;

            if ((pin == HW->gpio_swclk) ||
                (pin == HW->gpio_swdio)) {
                DEBUG("Restore pin %d", pin);

                GPIO_CTRL(pin) = GPIO_CTRL_RESET;
                // Use measured value to restore function
                if (pin == HW->gpio_swclk) {
                    GPIO_PAD(SWCLK_PAD) = 0x5A;
                } else {
                    GPIO_PAD(SWDIO_PAD) = 0x5A;
                }
            }
        }
    }
}

// Shut SWD down for the remainder of this power cycle.
//
// Called just before we start serving, so a probe is usable for the whole of
// boot (including boot logging, which rides RTT over SWD) and only goes away
// once serving starts.  There is deliberately no path back - the pads stay
// isolated until the next reset.
//
// The point is to stop the debug port's SRAM accesses stealing cycles from
// the serving DMAs.  It is not a debug lockout: the boot ROM runs before we
// do, and BOOTSEL/PICOBOOT are unaffected.
//
// Same mechanism as the shared image select pin handling in setup_sel_pins()
// - force the debug port to attach internally, and isolate both SWD pads so
// an external probe can no longer clock the port - but applied to both pads
// unconditionally, as SWD may not be shared with a sel pin on this board.
void disable_swd(void) {
    SYSCFG_DBGFORCE |= SYSCFG_DBGFORCE_ATTACH_BIT;
    GPIO_PAD(SWCLK_PAD) = (1 << PAD_ISO_BIT);
    GPIO_PAD(SWDIO_PAD) = (1 << PAD_ISO_BIT);
}
#endif // !TEST_BUILD

void setup_status_led(void) {
#if REAL_HARDWARE
    // Configure the status LED GPIO as an SIO push-pull output, driven high so
    // the LED is off (active-low wiring). Idempotent and self-contained, so it
    // is safe to call repeatedly - e.g. a fault handler calls it to reclaim the
    // pin (funcsel/drive/OE) before forcing the LED on, in case a plugin such
    // as the neopixel driver had reconfigured it.
    if (HW->gpio_status < MAX_GPIOS) {
        uint8_t pin = HW->gpio_status;
        GPIO_CTRL(pin) = GPIO_CTRL_RESET;   // SIO function
        GPIO_PAD(pin) &= ~(PAD_OUTPUT_DISABLE | PAD_INPUT);
        GPIO_PAD(pin) |= PAD_DRIVE(PAD_DRIVE_4MA);
        SIO_GPIO_OE_SET_PIN(pin);
        SIO_GPIO_OUT_SET_PIN(pin);
    }
#endif // REAL_HARDWARE
}

void blink_pattern(uint32_t on_time, uint32_t off_time, uint8_t repeats) {
    if (RUNTIME->status_led_enabled && HW->gpio_status < MAX_GPIOS) {
        uint8_t pin = HW->gpio_status;
        for(uint8_t i = 0; i < repeats; i++) {
            status_led_on(pin);
            delay(on_time);
            status_led_off(pin);
            delay(off_time);
        }   
    }
}

#if !defined(TEST_BUILD)
// Enters bootloader mode.
void enter_bootloader(void) {
    // Look up the reboot function from ROM
    typedef int (*reboot_fn_t)(uint32_t flags, uint32_t delay_ms, uint32_t p0, uint32_t p1);
    typedef void *(*rom_table_lookup_fn)(uint32_t code, uint32_t mask);
    
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Warray-bounds"
    rom_table_lookup_fn rom_table_lookup = 
        (rom_table_lookup_fn)(uintptr_t)*(uint16_t*)(0x00000016);
#pragma GCC diagnostic pop
    
    // 0x0004 is ARM secure mode
    uint32_t reboot_code = ('B' << 8) | 'R';
    reboot_fn_t reboot = (reboot_fn_t)rom_table_lookup(reboot_code, 0x0004);

    if (reboot == NULL) {
        ERR("Unable to find reboot function in ROM - cannot enter bootloader");
        return;
    }

    // Reboot into BOOTSEL mode with status LED as activity indicator (active low)
    uint32_t flags = 0x0100 | 0x0002;   // No return on success | BOOTSEL mode
    uint32_t ms_delay = 10; // 10ms delay before reboot, needs to be non-zero
    uint32_t p0 = 0;
    uint32_t p1 = 0;

    // There is a bug in the Pico SDK and RP2350 datasheet defining p0 and p1
    // for reboot() when using REBOOT_TYPE_BOOTSEL (0x0002).  p0 and p1 have
    // been transposed.  p1 is the status LED pin, p0 the flags.  We don't want
    // to enable the status LED, because it looks too much like One ROM is
    // ready to serve bytes.  Hence we leave it disabled.  This makes it light
    // up dimly, just like during initial power-on.
    // 
    // However, we do want to explicitly disable mass storage mode, so we set
    // bit 0 of p0 (not p1!).  If you want mass storage mode, jump BOOTSEL to
    // GND when plugging in.
    p0 |= 0x01;     // Disable mass storage mode
    reboot(flags, ms_delay, p0, p1);
}
#endif // !TEST_BUILD

#if !defined(TEST_BUILD)
void platform_logging(void) {
    if (BOOT_LOGGING_EN) {
        if (RUNTIME->rp235x == RP235XA) {
            LOG("RP235XA");
        } else {
            LOG("RP235XB");
        }
        DEBUG("Chip ID: 0x%08X", SYSINFO_CHIP_ID);
        DEBUG("Chip commit: 0x%08X", SYSINFO_GITREF_RP2350);
        if ((MCU_RAM_SIZE_KB != RP2350_RAM_SIZE_KB) || (MCU_RAM_SIZE != (RP2350_RAM_SIZE_KB * 1024))) {
            ERR("RAM error: actual %dKB, expected: %dKB",
                MCU_RAM_SIZE_KB,
                RP2350_RAM_SIZE_KB);
            limp_mode(LIMP_MODE_INVALID_BUILD);
        } else {
            LOG("RAM: %dKB", MCU_RAM_SIZE_KB);
        }
        LOG("Flash: %dKB", MCU_FLASH_SIZE_KB);
        LOG("Freq: %dMHz", TARGET_FREQ_MHZ);
        LOG("PLL: %d/%d/%d/%d", PLL_SYS_REFDIV, PLL_SYS_FBDIV, PLL_SYS_POSTDIV1, PLL_SYS_POSTDIV2);
    }
}

void setup_xosc(void) {
    // Initialize XOSC peripheral.  We are using the 12MHz xtal from the
    // reference hardware design, so we can use values from the datasheet.
    // See S8.2 for more details.
    //
    // Specifically:
    // - Set the startup delay to 1ms
    // - Enable the XOSC giving it the appropriate frequency range (1-15MHz)
    // - Wait for the XOSC to be enabled and stable
    XOSC_STARTUP = XOSC_STARTUP_DELAY_1MS;
    XOSC_CTRL = XOSC_ENABLE | XOSC_RANGE_1_15MHz;
    while (!(XOSC_STATUS & XOSC_STATUS_STABLE));
    DEBUG("XOSC enabled");

    // Switch CLK_REF to use XOSC instead of the ROSC
    CLOCK_REF_CTRL = CLOCK_REF_SRC_XOSC;
    while ((CLOCK_REF_SELECTED & CLOCK_REF_SRC_SEL_XOSC) != CLOCK_REF_SRC_SEL_XOSC);
}
#endif // !TEST_BUILD