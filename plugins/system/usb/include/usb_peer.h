// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// The USB plugin's peer data format.  See ora_plugin_header_t::peer_data.
//
// Setting USB_PEER_SERIAL_PORT_2 in a user plugin's peer data adds serial
// port 2.  Serial port 2 sends log channel 2 to the host and writes the host's
// input to log channel 3.  USB_PEER_SERIAL_PORT_2 is set in the USB plugin's
// own peer data when serial port 2 is supported.

#ifndef USB_PEER_H
#define USB_PEER_H

// Word 0, "USB1".
#define USB_PEER_WORD_FORMAT    0
#define USB_PEER_FORMAT         0x31425355u

// Word 1, flags.
#define USB_PEER_WORD_FLAGS     1
#define USB_PEER_SERIAL_PORT_2  (1u << 0)

// Word 2, the address of serial port 2's name, or 0 for the default.  The name
// is NUL-terminated ASCII in the user plugin's flash region.  The default is
// used instead if the name:
// - is empty
// - extends outside that region
// - is longer than USB_PEER_SERIAL_PORT_2_NAME_MAX characters
// - contains a character that isn't printable
#define USB_PEER_WORD_SERIAL_PORT_2_NAME    2
#define USB_PEER_SERIAL_PORT_2_NAME_MAX     31
#define USB_PEER_SERIAL_PORT_2_NAME         "One ROM Serial 2"

#endif // USB_PEER_H
