// One ROM - Vector table and reset handler.

// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(TEST_BUILD)

#include "include.h"

// Forward declarations
void Reset_Handler(void);
void Default_Handler(void);
void NMI_Handler(void);
void HardFault_Handler(void);
void BusFault_Handler(void);
void UsageFault_Handler(void);

// Default exception/interrupt handlers
#define MemManage_Handler   Default_Handler
#define SVC_Handler         Default_Handler
#define DebugMon_Handler    Default_Handler
#define PendSV_Handler      Default_Handler
#define SysTick_Handler     Default_Handler

// Declare stack section
extern uint32_t _estack;

// Vector table - must be placed at the start of flash
__attribute__ ((section(".isr_vector"), used))
void (* const g_pfnVectors[])(void) = {
    (void (*)(void))&_estack,      // Initial stack pointer
    Reset_Handler,                 // Reset handler
    NMI_Handler,                   // NMI handler
    HardFault_Handler,             // Hard fault handler
    MemManage_Handler,             // MPU fault handler
    BusFault_Handler,              // Bus fault handler
    UsageFault_Handler,            // Usage fault handler
    0, 0, 0, 0,                    // Reserved
    SVC_Handler,                   // SVCall handler
    DebugMon_Handler,              // Debug monitor handler
    0,                             // Reserved
    PendSV_Handler,                // PendSV handler
    SysTick_Handler,               // SysTick handler

    // Peripheral interrupt handlers follow 0-15 above.
    // 16-19
#if defined(STM32F4)
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
#else // RP235X
    irq_handler_timer0_irq_0, Default_Handler, Default_Handler, Default_Handler,
#endif 
    // 20-23
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    // 24-27
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    // 28-31
#if defined(STM32F4)
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
#else // RP235X
    Default_Handler, Default_Handler, irq_handler_usbctrl_irq, Default_Handler,
#endif 
    // 32-35
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    // 36-39
#if defined(STM32F4)
    Default_Handler, Default_Handler, Default_Handler, vbus_connect_handler,
#else // RP235X
    Default_Handler, vbus_connect_handler, Default_Handler, Default_Handler,
#endif 
    // 40-43
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    Default_Handler, Default_Handler, Default_Handler, Default_Handler,
    // Different STM32F4s have different numbers of interrupts.  The maximum
    // appears to be 96 (F446), which is what's included here.  This means
    // that 0x080001C4 onwards is free, but we'll not use anything until
    // 0x08000200 to be safe.
    // Note that the RP2350 has 52, so this is sufficient.  See datasheet
    // S3.2 - Interrupts
};

//
// Variables defined by the linker.
//
// Note these are "labels" that mark memory addresses, not variables that
// store data.  The address of the label IS the address we're interested in.
// Hence we use & below to get the addresses that these labels represent.
extern uint32_t _sidata;    // Start of .data section in FLASH
extern uint32_t _sdata;     // Start of .data section in RAM
extern uint32_t _edata;     // End of .data section in RAM
extern uint32_t _sbss;      // Start of .bss section in RAM
extern uint32_t _ebss;      // End of .bss section in RAM

// Location of RAM reserved for executing the main loop function from, if
// EXECUTE_FROM_RAM is defined.
#if defined(EXECUTE_FROM_RAM)
extern uint32_t _main_loop_start;   // Start of .main_loop section in FLASH
extern uint32_t _main_loop_end;     // End of .main_loop section in FLASH
extern uint32_t _ram_func_start;    // Start of .ram_func section in RAM
extern uint32_t _ram_func_end;      // End of .ram_func section in RAM
#endif

extern uint32_t _onerom_runtime_info_ram[]; // Start of .onerom_runtime_info section in RAM
extern uint32_t _onerom_runtime_info_flash[];   // Start of .onerom_runtime_info section in flash
extern uint32_t _onerom_runtime_info_end[];   // End of .onerom_runtime_info section in RAM

void ora_launch_plugins(void);

// Reset handler
void Reset_Handler(void) {
    // Enable hard floating point support:
    // - Same on STM32F4 M4 and RP235X Cortex-M33
    // - Enable CP10 and CP11 (FP extension) in the Cortex-M33
    SCB_CPACR |= SCB_CPACR_ENABLE_FP; // Enable CP10 and CP11 full access
    __asm volatile ("dsb");
    __asm volatile ("isb");

    // We use memcpy and memset because it's likely to be faster than anything
    // we could come up with.

    // Copy onerom_runtime_info_t from flash to RAM, magic last.
    // The magic acts as a commit indicator: valid magic means the full
    // struct is visible. Skip the first 4 bytes (magic) in the main copy.
    const size_t runtime_size = (size_t)((char*)_onerom_runtime_info_end - (char*)_onerom_runtime_info_ram);

    memcpy((uint8_t*)_onerom_runtime_info_ram + 4,
        (uint8_t*)_onerom_runtime_info_flash + 4,
        runtime_size - 4);

    // Ensure all struct bytes are visible before the magic is written.
    __asm volatile ("dmb" ::: "memory");

    // Write magic last.
    memcpy(_onerom_runtime_info_ram,
        _onerom_runtime_info_flash,
        4);

    // Copy data section from flash to RAM
    memcpy(&_sdata, &_sidata, (unsigned int)((char*)&_edata - (char*)&_sdata));
    
    // Zero out bss section  
    memset(&_sbss, 0, (unsigned int)((char*)&_ebss - (char*)&_sbss));
    
    // Call the main function
    firmware_main();

    // Main has returned - which means we are doing byte serving with PIOs.
    // We can now launch plugins.
    ora_launch_plugins();

    // Belt and braces - ora_launch_plugins never returns.
    while(1) {
        __asm volatile("wfi");
    }
}

// Default handler for unhandled interrupts - fast continuous blink
void Default_Handler(void) {
    // Halt regardless of the status LED: an unhandled interrupt stays pending,
    // so returning from here just re-enters in a tight spin.  blink_pattern()
    // gates on status_led_enabled, so the LED only blinks when it is enabled.
    setup_status_led();
    while (1) {
        blink_pattern(100000, 100000, 255);
    }
}

// NMI_Handler - single blink pattern
void NMI_Handler(void) {
    // Force the status LED on so the fault is visible even if it was off.
    RUNTIME->status_led_enabled = 1;
    setup_status_led();

    while(1) {
        blink_pattern(100000, 500000, 1); // Single blink
        delay(1000000); // Long pause
    }
}

typedef struct {
    uint32_t r0, r1, r2, r3;
    uint32_t r12;
    uint32_t lr;
    uint32_t pc;
    uint32_t xpsr;
} stacked_frame_t;

// HardFault_Handler - double blink pattern
void HardFault_C(stacked_frame_t *frame) {
    (void)frame;

    // Fault status registers
    volatile uint32_t cfsr  = *(volatile uint32_t *)0xE000ED28; // MMFSR+BFSR+UFSR
    volatile uint32_t hfsr  = *(volatile uint32_t *)0xE000ED2C;
    volatile uint32_t mmfar = *(volatile uint32_t *)0xE000ED34; // Valid if CFSR.MMARVALID
    volatile uint32_t bfar  = *(volatile uint32_t *)0xE000ED38; // Valid if CFSR.BFARVALID
    (void)cfsr;
    (void)hfsr;
    (void)mmfar;
    (void)bfar;

    // Force the status LED on so the fault is visible even if it was off.
    RUNTIME->status_led_enabled = 1;
    setup_status_led();
    while(1) {
        blink_pattern(100000, 200000, 2);
        delay(1000000);
    }
}

void __attribute__((naked)) HardFault_Handler(void) {
    __asm volatile (
        "tst   lr, #4\n"
        "ite   eq\n"
        "mrseq r0, msp\n"
        "mrsne r0, psp\n"
        "b     HardFault_C\n"
    );
}

// BusFault_Handler - triple blink pattern
void BusFault_Handler(void) {
    // Force the status LED on so the fault is visible even if it was off.
    RUNTIME->status_led_enabled = 1;
    setup_status_led();

    while(1) {
        blink_pattern(100000, 200000, 3); // Triple blink
        delay(1000000); // Long pause
    }
}

// UsageFault_Handler - quadruple blink pattern
void UsageFault_Handler(void) {
    // Force the status LED on so the fault is visible even if it was off.
    RUNTIME->status_led_enabled = 1;
    setup_status_led();

    while(1) {
        blink_pattern(100000, 200000, 4); // Quadruple blink
        delay(1000000); // Long pause
    }
}

#endif // !TEST_BUILD
