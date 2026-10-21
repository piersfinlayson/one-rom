// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Extension scenarios: does the device obey host-control's EXTENSIONS.md?
//!
//! EXTENSIONS.md specifies the commands host-control adds in the ranges RBCP
//! reserves for implementation-specific use. Each scenario identifies the
//! section it asserts.

use crate::Scenario;

pub mod device_info;

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "extensions.device_info.version_is_zero_uncommissioned",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_VERSION",
        run: device_info::version_is_zero_uncommissioned,
    },
    Scenario {
        name: "extensions.device_info.version_reports_the_commissioned_version",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_VERSION",
        run: device_info::version_reports_the_commissioned_version,
    },
    Scenario {
        name: "extensions.device_info.value_reads_every_key",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_VALUE",
        run: device_info::value_reads_every_key,
    },
    Scenario {
        name: "extensions.device_info.value_reads_from_an_offset",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_VALUE",
        run: device_info::value_reads_from_an_offset,
    },
    Scenario {
        name: "extensions.device_info.value_fails_where_absent",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_VALUE",
        run: device_info::value_fails_where_absent,
    },
    Scenario {
        name: "extensions.device_info.value_fails_on_an_unsupported_version",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_VERSION",
        run: device_info::value_fails_on_an_unsupported_version,
    },
    Scenario {
        name: "extensions.device_info.instance_reads_the_whole_instance",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_INSTANCE",
        run: device_info::instance_reads_the_whole_instance,
    },
    Scenario {
        name: "extensions.device_info.instance_reads_from_an_offset",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_INSTANCE",
        run: device_info::instance_reads_from_an_offset,
    },
    Scenario {
        name: "extensions.device_info.instance_fails",
        spec_ref: "EXTENSIONS.md: GET_COMMISSIONING_INSTANCE",
        run: device_info::instance_fails,
    },
    Scenario {
        name: "extensions.device_info.metadata_str_reads_a_string",
        spec_ref: "EXTENSIONS.md: GET_METADATA_STR",
        run: device_info::metadata_str_reads_a_string,
    },
    Scenario {
        name: "extensions.device_info.metadata_str_unset_has_length_zero",
        spec_ref: "EXTENSIONS.md: GET_METADATA_STR",
        run: device_info::metadata_str_unset_has_length_zero,
    },
    Scenario {
        name: "extensions.device_info.metadata_str_fails",
        spec_ref: "EXTENSIONS.md: GET_METADATA_STR",
        run: device_info::metadata_str_fails,
    },
    Scenario {
        name: "extensions.device_info.metadata_uint_reads_a_number",
        spec_ref: "EXTENSIONS.md: GET_METADATA_UINT",
        run: device_info::metadata_uint_reads_a_number,
    },
    Scenario {
        name: "extensions.device_info.metadata_uint_fails",
        spec_ref: "EXTENSIONS.md: GET_METADATA_UINT",
        run: device_info::metadata_uint_fails,
    },
    Scenario {
        name: "extensions.device_info.metadata_array_reads_the_chip_id",
        spec_ref: "EXTENSIONS.md: Example — Board Size and Chip ID",
        run: device_info::metadata_array_reads_the_chip_id,
    },
    Scenario {
        name: "extensions.device_info.metadata_array_reads_from_an_index",
        spec_ref: "EXTENSIONS.md: GET_METADATA_ARRAY",
        run: device_info::metadata_array_reads_from_an_index,
    },
    Scenario {
        name: "extensions.device_info.metadata_array_fails",
        spec_ref: "EXTENSIONS.md: GET_METADATA_ARRAY",
        run: device_info::metadata_array_fails,
    },
    Scenario {
        name: "extensions.device_info.metadata_array_fails_on_a_wide_element",
        spec_ref: "EXTENSIONS.md: GET_METADATA_ARRAY",
        run: device_info::metadata_array_fails_on_a_wide_element,
    },
    Scenario {
        name: "extensions.device_info.commands_fail_without_room_for_the_response",
        spec_ref: "EXTENSIONS.md: Group 0x80 — Device Info",
        run: device_info::commands_fail_without_room_for_the_response,
    },
    Scenario {
        name: "extensions.device_info.reserved_key_byte_is_rejected",
        spec_ref: "EXTENSIONS.md: Group 0x80 — Device Info",
        run: device_info::reserved_key_byte_is_rejected,
    },
    Scenario {
        name: "extensions.device_info.not_valid_in_command_mode",
        spec_ref: "EXTENSIONS.md: Implementation Specific Command Groups",
        run: device_info::not_valid_in_command_mode,
    },
    Scenario {
        name: "extensions.device_info.argument_counts_are_consumed_exactly",
        spec_ref: "EXTENSIONS.md: Group 0x80 — Device Info",
        run: device_info::argument_counts_are_consumed_exactly,
    },
];
