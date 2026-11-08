// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// The USB plugin's peer data block formats.  See ora_peer_data_header_t.
//
// Another plugin's USBc block sets which of its serial ports the USB plugin
// enables and can also configure their descriptions.
//
// Serial port 1 sends log channel 0 to the host and writes the host's input to
// log channel 1.  Serial port 2 does the same with channels 2 and 3.  The
// defaults are used when the user plugin doesn't have a valid USBc block.
//
// USBi is the format of the USB plugin's own block, used by this plugin to
// advertise its capabilities to other plugins.
//
// A host numbers the enabled serial ports from 1.  If only serial port 2 is
// enabled it is the host's port 1.

#ifndef USB_PEER_H
#define USB_PEER_H

#include <plugin.h>

// The USBc format word, "USBc", and version.
#define USB_PEER_USBC_FORMAT    0x63425355u
#define USB_PEER_USBC_VERSION   1u

// USBc flags, one for each serial port to enable.
#define USB_PEER_USBC_SERIAL_PORT_1 (1u << 0)
#define USB_PEER_USBC_SERIAL_PORT_2 (1u << 1)

// A serial port description is NUL-terminated ASCII in the user plugin's flash
// region.  The default is used instead if the description:
// - is empty
// - extends outside that region
// - is longer than USB_PEER_SERIAL_PORT_DESC_MAX_LEN characters
// - contains a character that isn't printable
#define USB_PEER_SERIAL_PORT_DESC_MAX_LEN   31
#define USB_PEER_SERIAL_PORT_1_DESC         "One ROM Serial"
#define USB_PEER_SERIAL_PORT_2_DESC         "One ROM Serial 2"

// A USBc block.  The USB plugin ignores a block whose version is 0 or whose
// size is less than a version 1 block's.
typedef struct {
    ora_peer_data_header_t hdr;

    // USB_PEER_USBC_SERIAL_PORT_* bits.  Without a valid block only serial
    // port 1 is enabled.
    uint32_t flags;

    // The addresses of serial port 1's and serial port 2's descriptions, or 0
    // for the default descriptions.
    uint32_t serial_port_1_desc;
    uint32_t serial_port_2_desc;
} usb_peer_usbc_t;
_Static_assert((sizeof(usb_peer_usbc_t) % 4u) == 0u,
               "usb_peer_usbc_t must be a multiple of 4 bytes");

// The USBi format word, "USBi", and version.
#define USB_PEER_USBI_FORMAT    0x69425355u
#define USB_PEER_USBI_VERSION   1u

// USBi serial port bits, one for each serial port the USB plugin supports.
#define USB_PEER_USBI_SERIAL_PORT_1 (1u << 0)
#define USB_PEER_USBI_SERIAL_PORT_2 (1u << 1)

typedef struct {
    ora_peer_data_header_t hdr;

    // USB_PEER_USBI_SERIAL_PORT_* bits.
    uint32_t serial_ports;
} usb_peer_usbi_t;
_Static_assert((sizeof(usb_peer_usbi_t) % 4u) == 0u,
               "usb_peer_usbi_t must be a multiple of 4 bytes");

#endif // USB_PEER_H
