// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// Types and controls the USB host shim shares with the Rust harness.

#if !defined(ORA_USB_SHIM_H)
#define ORA_USB_SHIM_H

#include <stdint.h>

// ---------------------------------------------------------------------------
// The CDC endpoint
//
// The plugin's log drain writes the banner and the forwarded log through six
// tinyusb calls, and every one of its interesting behaviours is a response to
// what the endpoint will take: a line resumed across passes, a banner regenerated
// from a cursor, a short write reported rather than swallowed.  So the endpoint
// is modelled with a settable capacity rather than one that always accepts, and
// what is flushed is kept for the harness to read.
// ---------------------------------------------------------------------------

// Most bytes the modelled endpoint can hold before a flush.  64 is a full speed
// CDC IN endpoint, which is what a device has.
#define USB_SHIM_TX_CAPACITY 64u

// How much the endpoint holds before it is full.  Set below the capacity to make
// the plugin resume a line it could not finish.  Room comes back as the harness
// takes bytes, so a scenario that never reads sees the endpoint fill and stay
// full, which is what a host that has stopped reading looks like.
void usb_host_test_set_tx_capacity(uint32_t capacity);

// Whether the bus can carry data, which is what tud_cdc_n_connected reports.
// False models a suspended or unconfigured bus, where a terminal is still
// attached but nothing can be sent.
void usb_host_test_set_connected(uint8_t connected);

// Raise or drop DTR, which is a terminal opening or closing the port.  Calls
// the plugin's own tud_cdc_line_state_cb, so the transition is seen exactly as
// tinyusb would report it.
void usb_host_test_set_dtr(uint8_t dtr);

// Everything flushed to the endpoint since the last call, copied into buf.
// Returns the number of bytes written, and drops what it returned.
uint32_t usb_host_test_take_tx(uint8_t *buf, uint32_t max_len);

// Bytes waiting to be taken.
uint32_t usb_host_test_tx_pending(void);

// ---------------------------------------------------------------------------
// picoboot
//
// picobootx is not compiled here — it has its own conformance suite, in its own
// repository.  What the shim keeps is the registration: the tables the plugin
// hands picoboot_init, so a scenario can call the plugin's handlers through the
// same pointers picoboot would, and assert what was registered as well as what
// the handlers do.
// ---------------------------------------------------------------------------

// The picoboot_ops_t the plugin registered, or NULL before usb_init has run.
const void *usb_host_test_picoboot_ops(void);

// The picoboot_custom_ops_t the plugin registered.
const void *usb_host_test_custom_ops(void);

// The context pointer the plugin registered, which its handlers are called with.
void *usb_host_test_picoboot_ctx(void);

// The chip ID the shim's picoboot_get_serial reports, as 16 hex digits.  A
// device reads this out of OTP, which does not exist here.
#define USB_SHIM_CHIP_ID 0x0123456789ABCDEFull

#endif // ORA_USB_SHIM_H
