// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

#if !defined(USB_LED_H)
#define USB_LED_H

#include <stdint.h>

#include "picobootx.h"
#include "usb_custom_pbx.h"

// Resolve the firmware's LED engine and set the RGB capability bit if it is
// there.  Called once at init, after gpio_init_caps(), which clears the
// capability word.
void led_init_caps(void);

// Hand a SET_LED to the firmware's engine, whichever LED it names, and report
// what the engine made of it.
pb_status_t led_handle_set(const onerom_set_led_args_t *args);

// Whether this device can answer ONEROM_CMD_LED_QUERY at all, which needs
// firmware with an LED engine to read.
uint8_t led_can_query(void);

// Fill in what one LED is doing.  The caller has already checked
// led_can_query().
pb_status_t led_fill_state(uint8_t led_id, onerom_led_state_t *out);

#endif //USB_LED_H