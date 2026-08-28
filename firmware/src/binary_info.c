// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

// What picotool reports about a One ROM image.  See binary_info.h for the
// format and for why the header's placement matters.

#include "include.h"
#include "binary_info.h"

#define BINARY_INFO_STRING(id_, str_) { \
    .type  = BINARY_INFO_TYPE_ID_AND_STRING, \
    .tag   = BINARY_INFO_TAG_RASPBERRY_PI, \
    .id    = (id_), \
    .value = (str_), \
}

static const binary_info_id_and_string_t bi_name =
    BINARY_INFO_STRING(BINARY_INFO_ID_PROGRAM_NAME, product);
static const binary_info_id_and_string_t bi_version =
    BINARY_INFO_STRING(BINARY_INFO_ID_PROGRAM_VERSION_STRING, version_str);
static const binary_info_id_and_string_t bi_description =
    BINARY_INFO_STRING(BINARY_INFO_ID_PROGRAM_DESCRIPTION, description);
static const binary_info_id_and_string_t bi_url =
    BINARY_INFO_STRING(BINARY_INFO_ID_PROGRAM_URL, project_url);
static const binary_info_id_and_string_t bi_build_date =
    BINARY_INFO_STRING(BINARY_INFO_ID_PROGRAM_BUILD_DATE_STRING, onerom_build_date);

static const void *const bi_entries[] = {
    &bi_name,
    &bi_version,
    &bi_description,
    &bi_url,
    &bi_build_date,
};

// Where to find a string that startup copied to RAM: its RAM address and the
// flash it came from.  Nothing here is copied, so all this holds is the
// all-zero entry that ends the table.
static const uint32_t bi_mapping_table[3] = {0, 0, 0};

// Placed by flash_binary_info.ld, which also asserts it landed where picotool
// looks.
__attribute__((section(".binary_info_header"), used))
const binary_info_header_t binary_info_header = {
    .marker_start  = BINARY_INFO_MARKER_START,
    .entries_start = bi_entries,
    .entries_end   = bi_entries + (sizeof(bi_entries) / sizeof(bi_entries[0])),
    .mapping_table = bi_mapping_table,
    .marker_end    = BINARY_INFO_MARKER_END,
};
