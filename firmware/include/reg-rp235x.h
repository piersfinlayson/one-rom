// RP235X registers header file

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#ifndef REG_RP235X_H
#define REG_RP235X_H

#define MCU_FLASH_SIZE     2097152
#define MCU_FLASH_SIZE_KB  2048
#define MCU_RAM_SIZE       532480
#define MCU_RAM_SIZE_KB    520

// Using Winbond W25Q16JV 2MB (16MBit)
#define MAX_FLASH_CLOCK_FREQ_MHZ 133
#define FLASH_SIZE_KB 2048
#if FLASH_SIZE_KB != MCU_FLASH_SIZE_KB
#error "Flash size mismatch"
#endif

#define RP2350_RAM_SIZE_KB 520
#if MCU_RAM_SIZE_KB != RP2350_RAM_SIZE_KB
#error "RAM size mismatch"
#endif

#define SRAM_BASE 0x20000000u

// Register base addresses
#define FLASH_BASE          0x10000000
#define XIP_CACHE_BASE      0x18000000
#define SYSINFO_BASE        0x40000000
#define SYSCFG_BASE         0x40008000
#define CLOCKS_BASE         0x40010000
#define PSM_BASE            0x40018000
#define RESETS_BASE         0x40020000
#define IO_BANK0_BASE       0x40028000
#define PADS_BANK0_BASE     0x40038000
#define XOSC_BASE           0x40048000  
#define PLL_SYS_BASE        0x40050000
#define PLL_USB_BASE        0x40058000
#define BUSCTRL_BASE        0x40068000
#define ADC_BASE            0x400a0000
#define TIMER0_BASE         0x400b0000
#define XIP_CTRL_BASE       0x400c8000
#define XIP_QMI_BASE        0x400d0000
#define POWMAN_BASE         0x40100000
#define TICKS_BASE          0x40108000
#define OTP_BASE            0x40120000
#define USBCTRL_REGS_BASE   0x50110000
#define SIO_BASE            0xD0000000
#define PBB_BASE            0xE0000000
#define SCB_BASE            0xE000ED00

// SysInfo Registers
#define SYSINFO_CHIP_ID         (*((volatile uint32_t *)(SYSINFO_BASE + 0x00)))
#define SYSINFO_PACKAGE_SEL     (*((volatile uint32_t *)(SYSINFO_BASE + 0x04)))
#define SYSINFO_GITREF_RP2350   (*((volatile uint32_t *)(SYSINFO_BASE + 0x14)))

#define SYSINFO_IS_QFN60()      (SYSINFO_PACKAGE_SEL & 0b1)

// SYSCFG Registers
#define SYSCFG_DBGFORCE             (*((volatile uint32_t *)(SYSCFG_BASE + 0x0C)))
#define SYSCFG_DBGFORCE_ATTACH_BIT  (1 << 3)
#define SYSCFG_DBGFORCE_SWCLK_BIT   (1 << 2)
#define SYSCFG_DBGFORCE_SWDI_BIT    (1 << 1)
#define SYSCFG_DBGFORCE_SWDO_BIT    (1 << 0)

// Clock registers
#define CLOCK_CLK_GPOUT0_CTRL   (*((volatile uint32_t *)(CLOCKS_BASE + 0x00)))
#define CLOCK_CLK_GPOUT0_DIV    (*((volatile uint32_t *)(CLOCKS_BASE + 0x04)))
#define CLOCK_CLK_GPOUT0_SEL    (*((volatile uint32_t *)(CLOCKS_BASE + 0x08)))
#define CLOCK_REF_CTRL          (*((volatile uint32_t *)(CLOCKS_BASE + 0x30)))
#define CLOCK_REF_DIV           (*((volatile uint32_t *)(CLOCKS_BASE + 0x34)))
#define CLOCK_REF_SELECTED      (*((volatile uint32_t *)(CLOCKS_BASE + 0x38)))
#define CLOCK_SYS_CTRL          (*((volatile uint32_t *)(CLOCKS_BASE + 0x3C)))
#define CLOCK_SYS_SELECTED      (*((volatile uint32_t *)(CLOCKS_BASE + 0x44)))
#define CLOCK_ADC_CTRL          (*((volatile uint32_t *)(CLOCKS_BASE + 0x6C)))
#define CLOCK_CLK_USB_CTRL      (*((volatile uint32_t *)(CLOCKS_BASE + 0x60)))

#define CLOCK_REF_SRC_XOSC      0x02
#define CLOCK_REF_SRC_SEL_MASK  0b1111
#define CLOCK_REF_SRC_SEL_XOSC  (1 << 2)

#define CLOCK_SYS_SRC_AUX           (1 << 0)
#define CLOCK_SYS_AUXSRC_PLL_SYS    (0x0 << 5)

#define CLOCK_ADC_ENABLE    (1 << 11)
#define CLOCK_ADC_ENABLED   (1 << 28)

#define CLOCK_USB_CTRL_ENABLE     (1 << 11)
#define CLOCK_USB_CTRL_AUXSRC_PLL_USB (0x0 << 5)

// PSM Registers
#define PSM_FRCE_OFF        (*((volatile uint32_t *)(PSM_BASE + 0x004)))
#define PSM_FRCE_OFF_SET    (*((volatile uint32_t *)(PSM_BASE + 0x004 + 0x2000)))
#define PSM_FRCE_OFF_CLR    (*((volatile uint32_t *)(PSM_BASE + 0x004 + 0x3000)))
#define PSM_PROC1_BIT       (1u << 24)

// Reset registers
#define RESET_RESET     (*((volatile uint32_t *)(RESETS_BASE + 0x00)))
#define RESET_WDSEL     (*((volatile uint32_t *)(RESETS_BASE + 0x04)))
#define RESET_DONE      (*((volatile uint32_t *)(RESETS_BASE + 0x08)))

#define RESET_ADC           (1 << 0)
#define RESET_DMA           (1 << 2)
#define RESET_IOBANK0       (1 << 6)
#define RESET_PADS_BANK0    (1 << 9)
#define RESET_JTAG          (1 << 8)
#define RESET_PIO0          (1 << 11)
#define RESET_PIO1          (1 << 12)
#define RESET_PIO2          (1 << 13)
#define RESET_PLL_SYS       (1 << 14)
#define RESET_PLL_USB       (1 << 15)
#define RESET_SYSCFG        (1 << 20)
#define RESET_SYSINFO       (1 << 21)
#define RESET_TIMER0        (1 << 23)
#define RESET_USBCTRL       (1 << 28)

// GPIO registers
#define GPIO_STATUS_OFFSET  0x000
#define GPIO_CTRL_OFFSET    0x004
#define GPIO_SPACING        0x008

#define GPIO_STATUS_INFROMPAD_BIT  17
#define GPIO_STATUS_OETOPAD_BIT    13

#define GPIO_IS_OUTPUT(pin)  ((GPIO_STATUS(pin) >> GPIO_STATUS_OETOPAD_BIT) & 1)

#define GPIO_STATUS(pin)    (*(volatile uint32_t*)(IO_BANK0_BASE + GPIO_STATUS_OFFSET + pin*GPIO_SPACING))
#define GPIO_CTRL(pin)      (*(volatile uint32_t*)(IO_BANK0_BASE + GPIO_CTRL_OFFSET + pin*GPIO_SPACING))
#define GPIO_READ(pin)      ((GPIO_STATUS(pin) >> GPIO_STATUS_INFROMPAD_BIT) & 1)

#define GPIO_CTRL_FUNC_SIO      0x05
#define GPIO_CTRL_FUNC_PIO0     0x06
#define GPIO_CTRL_FUNC_PIO1     0x07
#define GPIO_CTRL_FUNC_PIO2     0x08
#define GPIO_CTRL_FUNC_MASK     0x1F
#define GPIO_CTRL_INOVER_INVERT (0b01 << 16)
#define GPIO_CTRL_INOVER_LOW    (0b10 << 16)
#define GPIO_CTRL_INOVER_HIGH   (0b11 << 16)
#define GPIO_CTRL_INOVER_MASK   (0b11 << 16)
#define GPIO_CTRL_OEOVER_INVERT (0b01 << 14)
#define GPIO_CTRL_OUTOVER_LOW   (0b10 << 16)
#define GPIO_CTRL_OUTOVER_HIGH  (0b11 << 16)
#define GPIO_CTRL_OEOVER_MASK   (0b11 << 14)
#define GPIO_CTRL_OUTOVER_INVERT  (0b01 << 12)
#define GPIO_CTRL_OUTOVER_MASK    (0b11 << 12)
#define GPIO_CTRL_RESET         GPIO_CTRL_FUNC_SIO

#define IO_BANK0_INTR0 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x230))
#define IO_BANK0_INTR1 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x234))
#define IO_BANK0_INTR2 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x238))
#define IO_BANK0_INTR3 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x23C))
#define IO_BANK0_PROC0_INTE0 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x248))
#define IO_BANK0_PROC0_INTE1 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x24C))
#define IO_BANK0_PROC0_INTE2 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x250))
#define IO_BANK0_PROC0_INTE3 (*(volatile uint32_t *)(IO_BANK0_BASE + 0x254))

// Pads registers
#define PAD_OFFSET_START    0x004
#define PAD_SPACING         0x004
#define GPIO_PAD(pin)       (*(volatile uint32_t*)(PADS_BANK0_BASE + PAD_OFFSET_START + pin*PAD_SPACING))  

// Strictly speaking the SWD pins cannot be used as GPIOs, but they do have
// PAD registers.  SWCLK comes after GPIO47, SWDIO after that.
#define SWCLK_PAD 48
#define SWDIO_PAD 49

// Pad control bits
#define PAD_SLEW_FAST_BIT   0
#define PAD_PDE_BIT         2
#define PAD_PUE_BIT         3
#define PAD_DRIVE_BIT       4
#define PAD_IE_BIT          6
#define PAD_OD_BIT          7
#define PAD_ISO_BIT         8
#define PAD_DRIVE_MASK      0x3
#define PAD_DRIVE_2MA       0x0
#define PAD_DRIVE_4MA       0x1
#define PAD_DRIVE_8MA       0x2
#define PAD_DRIVE_12MA      0x3
#define PAD_DRIVE(X)        ((X & PAD_DRIVE_MASK) << PAD_DRIVE_BIT)
#define PAD_SLEW_FAST       (1 << PAD_SLEW_FAST_BIT)
#define PAD_INPUT           (1 << PAD_IE_BIT)
#define PAD_OUTPUT_DISABLE  (1 << PAD_OD_BIT)
#define PAD_PU              (1 << PAD_PUE_BIT)
#define PAD_PD              (1 << PAD_PDE_BIT)
#define PAD_INPUT_PD        ((1 << PAD_PDE_BIT) | (1 << PAD_IE_BIT))
#define PAD_INPUT_PU        ((1 << PAD_PUE_BIT) | (1 << PAD_IE_BIT))

// Crystal Oscillator Registers
#define XOSC_CTRL       (*((volatile uint32_t *)(XOSC_BASE + 0x00)))
#define XOSC_STATUS     (*((volatile uint32_t *)(XOSC_BASE + 0x04)))
#define XOSC_DORMANT    (*((volatile uint32_t *)(XOSC_BASE + 0x08)))
#define XOSC_STARTUP    (*((volatile uint32_t *)(XOSC_BASE + 0x0C)))
#define XOSC_COUNT      (*((volatile uint32_t *)(XOSC_BASE + 0x10)))

// XOSC Values - See datasheet S8.2
#define XOSC_STARTUP_DELAY_1MS  47
#define XOSC_ENABLE         (0xfab << 12)
#define XOSC_RANGE_1_15MHz  0xaa0
#define XOSC_STATUS_STABLE  (1 << 31)

// PLL Registers
#define PLL_SYS_CS          (*((volatile uint32_t *)(PLL_SYS_BASE + 0x00)))
#define PLL_SYS_PWR         (*((volatile uint32_t *)(PLL_SYS_BASE + 0x04)))
#define PLL_SYS_FBDIV_INT   (*((volatile uint32_t *)(PLL_SYS_BASE + 0x08)))
#define PLL_SYS_PRIM        (*((volatile uint32_t *)(PLL_SYS_BASE + 0x0C)))
#define PLL_SYS_INTR        (*((volatile uint32_t *)(PLL_SYS_BASE + 0x10)))
#define PLL_SYS_INTE        (*((volatile uint32_t *)(PLL_SYS_BASE + 0x14)))
#define PLL_SYS_INTF        (*((volatile uint32_t *)(PLL_SYS_BASE + 0x18)))
#define PLL_SYS_INTS        (*((volatile uint32_t *)(PLL_SYS_BASE + 0x1C)))

// PLL Control/Status bits
#define PLL_CS_LOCK         (1 << 31)
#define PLL_CS_BYPASS       (1 << 8)
#define PLL_CS_REFDIV_MASK  0b111111
#define PLL_CS_REFDIV(X)    (((X) & PLL_CS_REFDIV_MASK) << PLL_CS_REFDIV_SHIFT)
#define PLL_CS_REFDIV_SHIFT 0

// PLL Power bits  
#define PLL_PWR_PD          (1 << 0)    // Power down
#define PLL_PWR_DSMPD       (1 << 2)    // DSM power down  
#define PLL_PWR_POSTDIVPD   (1 << 3)    // Post divider power down
#define PLL_PWR_VCOPD       (1 << 5)    // VCO power down

// PLL Post divider bits
#define PLL_PRIM_POSTDIV1(X)    (((X) & PLL_PRIM_POSTDIV_MASK) << 16)
#define PLL_PRIM_POSTDIV2(X)    (((X) & PLL_PRIM_POSTDIV_MASK) << 12)
#define PLL_PRIM_POSTDIV_MASK   0x7

// USB PLL Registers
#define PLL_USB_CS          (*((volatile uint32_t *)(PLL_USB_BASE + 0x00)))
#define PLL_USB_PWR         (*((volatile uint32_t *)(PLL_USB_BASE + 0x04)))
#define PLL_USB_FBDIV_INT   (*((volatile uint32_t *)(PLL_USB_BASE + 0x08)))
#define PLL_USB_PRIM        (*((volatile uint32_t *)(PLL_USB_BASE + 0x0C)))

// BUSCTRL Registers
#define BUSCTRL_BUS_PRIORITY    (*((volatile uint32_t *)(BUSCTRL_BASE + 0x00)))

// DMA Read and Write Priority Bits
#define BUSCTRL_BUS_PRIORITY_DMA_R_BIT   (1 << 8)
#define BUSCTRL_BUS_PRIORITY_DMA_W_BIT   (1 << 12)

// ADC Registers
#define ADC_CS              (*((volatile uint32_t *)(ADC_BASE + 0x00)))
#define ADC_RESULT          (*((volatile uint32_t *)(ADC_BASE + 0x04)))

#define ADC_RESULT_MASK       0xFFF

#define ADC_CS_AINSEL_MASK   0b1111
#define ADC_CS_AINSEL_SHIFT  12
#define ADC_CS_AINSEL(X)     (((X) & ADC_CS_AINSEL_MASK) << ADC_CS_AINSEL_SHIFT)
#define ADC_CS_TS           4
#define ADC_CS_READY        (1 << 8)
#define ADC_CS_START_ONCE   (1 << 2)
#define ADC_CS_TS_EN        (1 << 1)
#define ADC_CS_EN           (1 << 0)

// TIMER0 Registers
#define TIMER0_TIMELR       (*((volatile uint32_t *)(TIMER0_BASE + 0x0C)))
#define TIMER0_ALARM0       (*((volatile uint32_t *)(TIMER0_BASE + 0x10)))
#define TIMER0_INTE         (*((volatile uint32_t *)(TIMER0_BASE + 0x40)))
#define TIMER0_INTR         (*((volatile uint32_t *)(TIMER0_BASE + 0x3C)))

// XIP_CTRL Registers
#define XIP_CTRL_CTRL       (*((volatile uint32_t *)(XIP_CTRL_BASE + 0x00)))
#define XIP_CTRL_STATUS     (*((volatile uint32_t *)(XIP_CTRL_BASE + 0x08)))
#define XIP_CTRL_CTR_HIT    (*((volatile uint32_t *)(XIP_CTRL_BASE + 0x0C)))
#define XIP_CTRL_CTR_ACC    (*((volatile uint32_t *)(XIP_CTRL_BASE + 0x10)))

// XIP_QMI Registers
#define XIP_QMI_M0_TIMING   (*((volatile uint32_t *)(XIP_QMI_BASE + 0x0C)))

#define XIP_QMI_M0_CLKDIV_MASK   0xFF
#define XIP_QMI_M0_CLKDIV_SHIFT  0

// Power Manager Registers
#define POWMAN_VREG_CTRL    (*((volatile uint32_t *)(POWMAN_BASE + 0x04)))
#define POWMAN_VREG_STATUS  (*((volatile uint32_t *)(POWMAN_BASE + 0x08)))
#define POWMAN_VREG         (*((volatile uint32_t *)(POWMAN_BASE + 0x0C)))

#define POWMAN_PASSWORD     (0x5AFE << 16)

#define POWMAN_VREG_CTRL_UNLOCK (1 << 13)
#define POWMAN_VREG_CTRL_DISABLE_VOLTAGE_LIMIT (1 << 8)
#define HT_TH_100       0x0
#define HT_TH_105       0x1
#define HT_TH_110       0x2
#define HT_TH_115       0x3
#define HT_TH_120       0x4
#define HT_TH_125       0x5
#define HT_TH_135       0x6
#define HT_TH_150       0x7
#define HT_TH_SHIFT     4
#define HT_TH_MASK      0b111
#define POWMAN_VREG_CTRL_HT_TH(X)   (((X) & HT_TH_MASK) << HT_TH_SHIFT)

#define VREG_MASK       0b11111
#define VREG_SHIFT      4
#define VREG_1_10V      0b01011
#define VREG_1_15V      0b01100
#define VREG_1_20V      0b01101
#define VREG_1_25V      0b01110
#define VREG_1_30V      0b01111
#define VREG_1_35V      0b10000
#define VREG_1_40V      0b10001
#define VREG_1_50V      0b10010
#define VREG_1_60V      0b10011
#define POWMAN_VREG_VOLTAGE(X)   (((X) & VREG_MASK) << VREG_SHIFT)
#define POWMAN_VREG_UPDATE (1 << 15)

// TICKS Registers
#define TICKS_TIMER0_CTRL   (*((volatile uint32_t *)(TICKS_BASE + 0x18)))
#define TICKS_TIMER0_CYCLES (*((volatile uint32_t *)(TICKS_BASE + 0x1C)))

// USB Registers
#define SIE_STATUS         (*((volatile uint32_t *)(USBCTRL_REGS_BASE + 0x50)))

#define SIE_STATUS_VBUS_DETECT_BIT   0
#define SIE_STATUS_VBUS_DETECT_MASK  (1 << SIE_STATUS_VBUS_DETECT_BIT)

// SIO Registers
#define SIO_CPUID           (*((volatile uint32_t *)(SIO_BASE + 0x00)))
#define SIO_GPIO_IN         (*((volatile uint32_t *)(SIO_BASE + 0x04)))
#define SIO_GPIO_HI_IN      (*((volatile uint32_t *)(SIO_BASE + 0x08)))
#define SIO_GPIO_OUT        (*((volatile uint32_t *)(SIO_BASE + 0x10)))
#define SIO_GPIO_HI_OUT     (*((volatile uint32_t *)(SIO_BASE + 0x14)))
#define SIO_GPIO_OUT_SET    (*((volatile uint32_t *)(SIO_BASE + 0x18)))
#define SIO_GPIO_HI_OUT_SET (*((volatile uint32_t *)(SIO_BASE + 0x1C)))
#define SIO_GPIO_OUT_CLR    (*((volatile uint32_t *)(SIO_BASE + 0x20)))
#define SIO_GPIO_HI_OUT_CLR (*((volatile uint32_t *)(SIO_BASE + 0x24)))
#define SIO_GPIO_OE         (*((volatile uint32_t *)(SIO_BASE + 0x30)))
#define SIO_GPIO_HI_OE      (*((volatile uint32_t *)(SIO_BASE + 0x34)))
#define SIO_GPIO_OE_SET     (*((volatile uint32_t *)(SIO_BASE + 0x38)))
#define SIO_GPIO_HI_OE_SET  (*((volatile uint32_t *)(SIO_BASE + 0x3C)))
#define SIO_GPIO_OE_CLR     (*((volatile uint32_t *)(SIO_BASE + 0x40)))
#define SIO_GPIO_HI_OE_CLR  (*((volatile uint32_t *)(SIO_BASE + 0x44)))
#define SIO_FIFO_ST         (*((volatile uint32_t *)(SIO_BASE + 0x50)))
#define SIO_FIFO_WR         (*((volatile uint32_t *)(SIO_BASE + 0x54)))
#define SIO_FIFO_RD         (*((volatile uint32_t *)(SIO_BASE + 0x58)))

#define SIO_GPIO_READ(pin)  ((pin < 32) ? \
                            (((*(volatile uint32_t*)(SIO_BASE + 0x004)) >> pin) & 1) : \
                            (((*(volatile uint32_t*)(SIO_BASE + 0x008)) >> (pin - 32)) & 1))

#define SIO_GPIO_OE_SET_PIN(pin)    if (pin < 32) \
                                        (*(volatile uint32_t*)(SIO_BASE + 0x038) = (1 << pin)); \
                                    else \
                                        (*(volatile uint32_t*)(SIO_BASE + 0x03C) = (1 << (pin - 32)));
#define SIO_GPIO_OUT_SET_PIN(pin)   if (pin < 32) \
                                        (*(volatile uint32_t*)(SIO_BASE + 0x018) = (1 << pin)); \
                                    else \
                                        (*(volatile uint32_t*)(SIO_BASE + 0x01C) = (1 << (pin - 32)));
#define SIO_GPIO_OUT_CLR_PIN(pin)   if (pin < 32) \
                                        (*(volatile uint32_t*)(SIO_BASE + 0x020) = (1 << pin)); \
                                    else \
                                        (*(volatile uint32_t*)(SIO_BASE + 0x024) = (1 << (pin - 32)));

// PPB Registers
#define NVIC_ISER0          (*((volatile uint32_t *)(PBB_BASE + 0x0E100)))
#define NVIC_ISER1          (*((volatile uint32_t *)(PBB_BASE + 0x0E104)))
#define NVIC_ICER0          (*((volatile uint32_t *)(PBB_BASE + 0x0E180)))
#define NVIC_ICER1          (*((volatile uint32_t *)(PBB_BASE + 0x0E184)))
#define IRQ_USBCTRL      14
#define IO_IRQ_BANK0        21

// SCB Registers
#define SCB_VTOR            (*((volatile uint32_t *)(SCB_BASE + 0x08)))
#define SCB_CPACR           (*((volatile uint32_t *)(SCB_BASE + 0x88)))
#define SCB_CPACR_CP0_FULL  (0b11 << 0)
#define SCB_CPACR_ENABLE_FP (0xF << 20)
#define SAU_CTRL            (*((volatile uint32_t *)(SCB_BASE + 0xD0)))

// Used by assembly
#define VAL_SIO_GPIO_IN         (SIO_BASE + 0x04)
#define VAL_SIO_GPIO_OUT        (SIO_BASE + 0x10)
#define VAL_SIO_GPIO_OE         (SIO_BASE + 0x30)

// RAM Size
#define RP2350_RAM_SIZE_KB  520

// Boot block structure
typedef struct {
    uint32_t start_marker;          // 0xffffded3, start marker
    uint8_t  image_type_tag;        // 0x42, image type
    uint8_t  image_type_len;        // 0x1, item is one word in size
    uint16_t image_type_data;       // 0b0001000000100001, RP2350, ARM, Secure, EXE
    uint8_t  type;                  // 0xff, size type, last item
    uint16_t size;                  // 0x0001, size
    uint8_t  pad;                   // 0
    uint32_t next_block;            // 0, link to self, no next block
    uint32_t end_marker;            // 0xab123579, end marker
} __attribute__((packed)) rp2350_boot_block_t;

typedef struct {
    uint16_t sys_clock_freq_mhz;
    uint16_t pll_sys_fbdiv;
    uint8_t pll_refdiv;
    uint8_t pll_sys_postdiv1;
    uint8_t pll_sys_postdiv2;
    fire_vreg_t vreg;
} rp235x_clock_config_t;

#define RP235X_STOCK_CLOCK_SPEED_MHZ 150
#define RP235X_MAX_CONFIGURABLE_MHZ 800

#endif // REG_RP235X_H