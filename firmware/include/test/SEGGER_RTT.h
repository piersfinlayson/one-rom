// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Dummy header to allow inclusion of SEGGER_RTT.h in test code without the
// real RTT implementation.

// Logging macros use printf and vprintf
#define SEGGER_RTT_printf(X, ...) printf(__VA_ARGS__)
#define SEGGER_RTT_vprintf(X, Y, Z) vprintf(Y, *Z)

#define do_log STUB_LOG

// Dummy RTT control block to allow the test code to compile and run.
extern uint32_t _SEGGER_RTT;
#if defined(TEST_MAIN_C)
uint32_t _SEGGER_RTT; 
#endif // TEST_MAIN_C