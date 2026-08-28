// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// The binary info block picotool reads out of a One ROM image, so that
// `picotool info` names the firmware and its version instead of reporting only
// the chip.
//
// The format is Raspberry Pi's, and these values come from it.  picotool scans
// the first 256 words of the image for the start marker and expects the end
// marker four words later, so the header has to lie in that window - the
// linker script asserts it does.  Everything the header points at can sit
// anywhere, because picotool follows the pointers.
//
// This is a second place the firmware states its version, alongside
// onerom_info_t.  Both are compiled from ONEROM_VERSION_*, and the entry here
// points at constants.c's version_str rather than spelling a version out
// again, so there is nothing to keep in step by hand.

#if !defined(BINARY_INFO_H)
#define BINARY_INFO_H

#include <stdint.h>

#define BINARY_INFO_MARKER_START 0x7188ebf2u
#define BINARY_INFO_MARKER_END   0xe71aa390u

// The only entry type One ROM uses: an identifier and a string.
#define BINARY_INFO_TYPE_ID_AND_STRING 6u

// Raspberry Pi's tag, which is what marks the identifiers below as theirs
// rather than a vendor's own.  'R' | 'P' << 8.
#define BINARY_INFO_TAG_RASPBERRY_PI 0x5052u

#define BINARY_INFO_ID_PROGRAM_NAME           0x02031c86u
#define BINARY_INFO_ID_PROGRAM_VERSION_STRING 0x11a9bc3au
#define BINARY_INFO_ID_PROGRAM_DESCRIPTION    0xb6a07c19u
#define BINARY_INFO_ID_PROGRAM_BUILD_DATE_STRING 0x9da22254u
#define BINARY_INFO_ID_PROGRAM_URL            0x1856239au

// One entry.  The string is not copied - picotool reads it from the image at
// the address this points to.
typedef struct {
    uint16_t    type;   // BINARY_INFO_TYPE_ID_AND_STRING
    uint16_t    tag;    // BINARY_INFO_TAG_RASPBERRY_PI
    uint32_t    id;     // which string this is
    const char *value;  // NUL terminated
} binary_info_id_and_string_t;

// What picotool scans for.  entries_start and entries_end bound a table of
// pointers to entries, and mapping_table translates addresses for data the
// image copies from flash to RAM.
typedef struct {
    uint32_t           marker_start;
    const void *const *entries_start;
    const void *const *entries_end;
    const uint32_t    *mapping_table;
    uint32_t           marker_end;
} binary_info_header_t;

#endif // BINARY_INFO_H
