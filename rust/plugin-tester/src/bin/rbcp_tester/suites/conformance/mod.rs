// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Conformance scenarios: does the device obey the RBCP specification?
//!
//! The oracle is the specification, not this device's implementation of it.
//! Each scenario names the section it asserts, and asserts what that section
//! requires — so a scenario failing means either the device is wrong or the
//! specification has changed, and both are worth knowing.
//!
//! One module per specification section.

use crate::Scenario;

pub mod control;
pub mod framing;
pub mod knock;
pub mod modify;
pub mod nv_storage;
pub mod processing_sequence;
pub mod read_group;
pub mod reset;
pub mod ring;
pub mod rom_types;

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "conformance.knock.required_before_command",
        spec_ref: "Session Initiation — The Knock",
        run: knock::required_before_command,
    },
    Scenario {
        name: "conformance.knock.partial_does_not_open",
        spec_ref: "Session Initiation — The Knock",
        run: knock::partial_does_not_open,
    },
    Scenario {
        name: "conformance.knock.must_be_contiguous",
        spec_ref: "Session Initiation — The Knock",
        run: knock::must_be_contiguous,
    },
    Scenario {
        name: "conformance.knock.required_after_exit",
        spec_ref: "Session Initiation — The Knock (re-entry after exit)",
        run: knock::required_after_exit,
    },
    Scenario {
        name: "conformance.framing.arguments_are_consumed",
        spec_ref: "Command Framing",
        run: framing::arguments_are_consumed,
    },
    Scenario {
        name: "conformance.framing.knock_not_seen_during_argument_collection",
        spec_ref: "Command Framing — Command Mode Constraint",
        run: framing::knock_not_seen_during_argument_collection,
    },
    Scenario {
        name: "conformance.framing.desync_recovers_within_ten_reads",
        spec_ref: "Command Framing — Command Mode Constraint",
        run: framing::desync_recovers_within_ten_reads,
    },
    Scenario {
        name: "conformance.control.exit_silent_writes_no_response_header",
        spec_ref: "Group 0x00 — EXIT_CMD_RESP_SILENT",
        run: control::exit_silent_writes_no_response_header,
    },
    Scenario {
        name: "conformance.control.switch_and_exit_writes_no_response_header",
        spec_ref: "Group 0x00 — SWITCH_AND_EXIT",
        run: control::switch_and_exit_writes_no_response_header,
    },
    Scenario {
        name: "conformance.control.enter_discards_unaligned_back_channel",
        spec_ref: "Group 0x00 — ENTER_CMD_RESP (back-channel start must be 4-byte aligned)",
        run: control::enter_discards_unaligned_back_channel,
    },
    Scenario {
        name: "conformance.control.enter_discards_out_of_range_command_page",
        spec_ref: "Group 0x00 — ENTER_CMD_RESP (command page out of range for the ROM served)",
        run: control::enter_discards_out_of_range_command_page,
    },
    Scenario {
        name: "conformance.control.enter_discards_complete_of_aa",
        spec_ref: "Group 0x00 — ENTER_CMD_RESP (neither A7 nor A8 may be 0xAA)",
        run: control::enter_discards_complete_of_aa,
    },
    Scenario {
        name: "conformance.control.enter_discards_status_ok_of_aa",
        spec_ref: "Group 0x00 — ENTER_CMD_RESP (neither A7 nor A8 may be 0xAA)",
        run: control::enter_discards_status_ok_of_aa,
    },
    Scenario {
        name: "conformance.control.enter_fails_when_back_channel_exceeds_slot",
        spec_ref: "Group 0x00 — ENTER_CMD_RESP (size exceeding the RAM slot returns failure)",
        run: control::enter_fails_when_back_channel_exceeds_slot,
    },
    Scenario {
        name: "conformance.control.enter_fails_when_already_in_command_response_mode",
        spec_ref: "Group 0x00 — ENTER_CMD_RESP (not supported in command-response mode)",
        run: control::enter_fails_when_already_in_command_response_mode,
    },
    Scenario {
        name: "conformance.control.exit_ack_completes_processing_sequence",
        spec_ref: "Group 0x00 — EXIT_CMD_RESP_ACK; Command Processing Sequence",
        run: control::exit_ack_completes_processing_sequence,
    },
    Scenario {
        name: "conformance.control.exit_ack_stops_maintaining_back_channel",
        spec_ref: "Group 0x00 — EXIT_CMD_RESP_ACK (back-channel no longer maintained)",
        run: control::exit_ack_stops_maintaining_back_channel,
    },
    Scenario {
        name: "conformance.control.switch_and_exit_slot_aa_still_exits",
        spec_ref: "Group 0x00 — SWITCH_AND_EXIT (A0 of 0xAA: the exit DOES complete)",
        run: control::switch_and_exit_slot_aa_still_exits,
    },
    Scenario {
        name: "conformance.control.switch_and_exit_slot_aa_does_not_switch",
        spec_ref: "Group 0x00 — SWITCH_AND_EXIT (A0 of 0xAA: the slot is NOT switched)",
        run: control::switch_and_exit_slot_aa_does_not_switch,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_count",
        spec_ref: "Group 0x01 — GET_FLASH_SLOT_COUNT; GET_FLASH_SLOT_COUNT Response Format",
        run: read_group::get_flash_slot_count,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_info",
        spec_ref: "Group 0x01 — GET_FLASH_SLOT_INFO; GET_FLASH_SLOT_INFO Response Format",
        run: read_group::get_flash_slot_info,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_info_rejects_slot_aa",
        spec_ref: "Group 0x01 — GET_FLASH_SLOT_INFO (A0 of 0xAA is invalid and rejected)",
        run: read_group::get_flash_slot_info_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_info_needs_room_for_a_record",
        spec_ref: "Group 0x01 — GET_FLASH_SLOT_INFO (sufficient space)",
        run: read_group::get_flash_slot_info_needs_room_for_a_record,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_info_all",
        spec_ref: "Group 0x01 — GET_FLASH_SLOT_INFO_ALL; GET_FLASH_SLOT_INFO_ALL Response Format",
        run: read_group::get_flash_slot_info_all,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_info_all_partial_record",
        spec_ref: "GET_FLASH_SLOT_INFO_ALL Response Format — Preamble, Records",
        run: read_group::get_flash_slot_info_all_partial_record,
    },
    Scenario {
        name: "conformance.read.get_flash_slot_info_all_one_byte_partial",
        spec_ref: "GET_FLASH_SLOT_INFO_ALL Response Format — Records (one-byte partial)",
        run: read_group::get_flash_slot_info_all_one_byte_partial,
    },
    Scenario {
        name: "conformance.read.get_ram_slot_info_all",
        spec_ref: "Group 0x01 — GET_RAM_SLOT_INFO_ALL; GET_RAM_SLOT_INFO Response Format",
        run: read_group::get_ram_slot_info_all,
    },
    Scenario {
        name: "conformance.read.get_device_type",
        spec_ref: "Group 0x01 — GET_DEVICE_TYPE; GET_DEVICE_TYPE Response Format",
        run: read_group::get_device_type,
    },
    Scenario {
        name: "conformance.read.get_device_version",
        spec_ref: "Group 0x01 — GET_DEVICE_VERSION; GET_DEVICE_VERSION Response Format",
        run: read_group::get_device_version,
    },
    Scenario {
        name: "conformance.read.get_protocol_version",
        spec_ref: "Group 0x01 — GET_PROTOCOL_VERSION; Versioning and Compatibility",
        run: read_group::get_protocol_version,
    },
    Scenario {
        name: "conformance.read.slot_peek",
        spec_ref: "Group 0x01 — SLOT_PEEK",
        run: read_group::slot_peek,
    },
    Scenario {
        name: "conformance.read.slot_peek_count_zero_is_256",
        spec_ref: "Group 0x01 — SLOT_PEEK (a count of zero indicates 256 bytes)",
        run: read_group::slot_peek_count_zero_is_256,
    },
    Scenario {
        name: "conformance.read.slot_peek_exceeding_data_section_fails",
        spec_ref: "Group 0x01 — SLOT_PEEK (insufficient space in the response data section)",
        run: read_group::slot_peek_exceeding_data_section_fails,
    },
    Scenario {
        name: "conformance.read.slot_peek_rejects_slot_aa",
        spec_ref: "Group 0x01 — SLOT_PEEK (A4 of 0xAA is invalid and rejected)",
        run: read_group::slot_peek_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.read.not_valid_in_command_mode",
        spec_ref: "Group 0x01 — Read (command-response mode only); Group 0x00 — EXIT_CMD_RESP_ACK",
        run: read_group::not_valid_in_command_mode,
    },
    Scenario {
        name: "conformance.modify.slot_poke_in_both_modes",
        spec_ref: "Group 0x02 — SLOT_POKE",
        run: modify::slot_poke_in_both_modes,
    },
    Scenario {
        name: "conformance.modify.slot_poke_rejects_slot_aa",
        spec_ref: "Group 0x02 — SLOT_POKE (A4 of 0xAA is invalid and rejected)",
        run: modify::slot_poke_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.modify.slot_poke_patches_inactive_slot",
        spec_ref: "Group 0x02 — SLOT_POKE (the safe pattern for patching vectors)",
        run: modify::slot_poke_patches_inactive_slot,
    },
    Scenario {
        name: "conformance.modify.switch_slot_moves_the_back_channel",
        spec_ref: "Group 0x02 — SWITCH_SLOT; Command-Response Mode — Back-Channel Region",
        run: modify::switch_slot_moves_the_back_channel,
    },
    Scenario {
        name: "conformance.modify.switch_slot_in_command_mode",
        spec_ref: "Group 0x02 — Modify (valid in both modes); SWITCH_SLOT",
        run: modify::switch_slot_in_command_mode,
    },
    Scenario {
        name: "conformance.modify.switch_slot_rejects_slot_aa",
        spec_ref: "Group 0x02 — SWITCH_SLOT (A0 of 0xAA is invalid and rejected)",
        run: modify::switch_slot_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.modify.load_slot_copies_without_activating",
        spec_ref: "Group 0x02 — LOAD_SLOT",
        run: modify::load_slot_copies_without_activating,
    },
    Scenario {
        name: "conformance.modify.load_slot_rejects_slot_aa",
        spec_ref: "Group 0x02 — LOAD_SLOT (A0 or A1 of 0xAA is invalid and rejected)",
        run: modify::load_slot_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.modify.slot_poke_all_byte_fills_the_slot",
        spec_ref: "Group 0x02 — SLOT_POKE_ALL_BYTE",
        run: modify::slot_poke_all_byte_fills_the_slot,
    },
    Scenario {
        name: "conformance.modify.slot_poke_all_byte_rejects_slot_aa",
        spec_ref: "Group 0x02 — SLOT_POKE_ALL_BYTE (A1 of 0xAA is invalid and rejected)",
        run: modify::slot_poke_all_byte_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.nv.get_nv_capability",
        spec_ref: "Group 0x03 — GET_NV_CAPABILITY; GET_NV_CAPABILITY Response Format",
        run: nv_storage::get_nv_capability,
    },
    Scenario {
        name: "conformance.nv.nv_capability_matches_behaviour",
        spec_ref: "Group 0x03 — GET_NV_CAPABILITY; NV_POKE_BEGIN",
        run: nv_storage::nv_capability_matches_behaviour,
    },
    Scenario {
        name: "conformance.nv.erased_storage_reads_as_ff",
        spec_ref: "Group 0x03 — NV Storage (initialized to 0xFF before first write)",
        run: nv_storage::erased_storage_reads_as_ff,
    },
    Scenario {
        name: "conformance.nv.nv_peek_reads_storage",
        spec_ref: "Group 0x03 — NV_PEEK; NV_PEEK Response Format",
        run: nv_storage::nv_peek_reads_storage,
    },
    Scenario {
        name: "conformance.nv.nv_peek_count_zero_is_256",
        spec_ref: "Group 0x03 — NV_PEEK (a count of zero indicates 256 bytes)",
        run: nv_storage::nv_peek_count_zero_is_256,
    },
    Scenario {
        name: "conformance.nv.nv_peek_rejects_location_msb_above_7f",
        spec_ref: "Group 0x03 — NV_PEEK (the location MSB must not exceed 0x7F)",
        run: nv_storage::nv_peek_rejects_location_msb_above_7f,
    },
    Scenario {
        name: "conformance.nv.nv_peek_beyond_storage_fails",
        spec_ref: "Group 0x03 — NV_PEEK (the requested range exceeds the NV storage size)",
        run: nv_storage::nv_peek_beyond_storage_fails,
    },
    Scenario {
        name: "conformance.nv.nv_peek_exceeding_data_section_fails",
        spec_ref: "Group 0x03 — NV_PEEK (insufficient space in the response data section)",
        run: nv_storage::nv_peek_exceeding_data_section_fails,
    },
    Scenario {
        name: "conformance.nv.nv_peek_and_slot_peek_read_different_stores",
        spec_ref: "Group 0x03 — NV_PEEK; Group 0x01 — SLOT_PEEK",
        run: nv_storage::nv_peek_and_slot_peek_read_different_stores,
    },
    Scenario {
        name: "conformance.nv.nv_poke_discard_abandons_the_transaction",
        spec_ref: "Group 0x03 — NV_POKE_BEGIN, NV_POKE, NV_POKE_DISCARD",
        run: nv_storage::nv_poke_discard_abandons_the_transaction,
    },
    Scenario {
        name: "conformance.nv.nv_poke_begin_rejects_a_second_transaction",
        spec_ref: "Group 0x03 — NV Storage (only one write transaction at a time)",
        run: nv_storage::nv_poke_begin_rejects_a_second_transaction,
    },
    Scenario {
        name: "conformance.nv.nv_poke_begin_rejects_bad_slots",
        spec_ref: "Group 0x03 — NV_POKE_BEGIN (invalid, active or 0xAA slot)",
        run: nv_storage::nv_poke_begin_rejects_bad_slots,
    },
    Scenario {
        name: "conformance.nv.nv_poke_and_discard_need_a_transaction",
        spec_ref: "Group 0x03 — NV_POKE, NV_POKE_DISCARD (no transaction in progress)",
        run: nv_storage::nv_poke_and_discard_need_a_transaction,
    },
    Scenario {
        name: "conformance.nv.nv_poke_rejects_bad_locations",
        spec_ref: "Group 0x03 — NV_POKE (location MSB above 0x7F; beyond the NV storage size)",
        run: nv_storage::nv_poke_rejects_bad_locations,
    },
    Scenario {
        name: "conformance.nv.nv_peek_reads_storage_during_a_transaction",
        spec_ref: "Group 0x03 — NV_PEEK (always reads NV storage, transaction or not)",
        run: nv_storage::nv_peek_reads_storage_during_a_transaction,
    },
    Scenario {
        name: "conformance.nv.exiting_command_response_mode_discards_the_transaction",
        spec_ref: "Group 0x03 — NV Storage (exit by any route discards the staging buffer)",
        run: nv_storage::exiting_command_response_mode_discards_the_transaction,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_byte_returns_early_for_an_unchanged_byte",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT_BYTE",
        run: nv_storage::nv_poke_commit_byte_returns_early_for_an_unchanged_byte,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_byte_rejects_slot_aa",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT_BYTE (A3 of 0xAA is invalid and rejected)",
        run: nv_storage::nv_poke_commit_byte_rejects_slot_aa,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_writes_the_staging_buffer",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT",
        run: nv_storage::nv_poke_commit_writes_the_staging_buffer,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_erases_before_programming",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT (device erase/program sequence)",
        run: nv_storage::nv_poke_commit_erases_before_programming,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_needs_a_transaction",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT (no transaction in progress)",
        run: nv_storage::nv_poke_commit_needs_a_transaction,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_byte_performs_the_whole_transaction",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT_BYTE (BEGIN, POKE, COMMIT in one command)",
        run: nv_storage::nv_poke_commit_byte_performs_the_whole_transaction,
    },
    Scenario {
        name: "conformance.nv.a_commit_outlives_the_session",
        spec_ref: "Group 0x03 — NV Storage (dedicated non-volatile storage)",
        run: nv_storage::a_commit_outlives_the_session,
    },
    Scenario {
        name: "conformance.nv.nv_poke_commit_byte_refused_when_read_only",
        spec_ref: "Group 0x03 — NV_POKE_COMMIT_BYTE (fails if NV storage is not writable)",
        run: nv_storage::nv_poke_commit_byte_refused_when_read_only,
    },
    Scenario {
        name: "conformance.nv.not_valid_in_command_mode",
        spec_ref: "Group 0x03 — NV Storage (command-response mode only)",
        run: nv_storage::not_valid_in_command_mode,
    },
    Scenario {
        name: "conformance.reset.group_and_command_bytes_match",
        spec_ref: "Communication Initiation — Resetting the Device",
        run: reset::group_and_command_bytes_match,
    },
    Scenario {
        name: "conformance.reset.exits_without_writing_a_response",
        spec_ref: "Group 0xAA — RBCP_RESET",
        run: reset::exits_without_writing_a_response,
    },
    Scenario {
        name: "conformance.reset.exit_allows_re_entry",
        spec_ref: "Group 0xAA — RBCP_RESET; Group 0x00 — ENTER_CMD_RESP",
        run: reset::exit_allows_re_entry,
    },
    Scenario {
        name: "conformance.reset.off_page_reset_is_filtered",
        spec_ref: "Communication Initiation — Resetting the Device (command page)",
        run: reset::off_page_reset_is_filtered,
    },
    Scenario {
        name: "conformance.reset.recommended_sequence_recovers_from_desync",
        spec_ref: "Communication Initiation — Resetting the Device",
        run: reset::recommended_sequence_recovers_from_desync,
    },
    Scenario {
        name: "conformance.processing_sequence.nop",
        spec_ref: "Command-Response Mode — Command Processing Sequence; Response Header",
        run: processing_sequence::nop,
    },
    Scenario {
        name: "conformance.processing_sequence.token_continues_across_entry",
        spec_ref: "Response Header — Token; Bootstrap — Entering Command-Response Mode",
        run: processing_sequence::token_continues_across_entry,
    },
    Scenario {
        name: "conformance.processing_sequence.token_wraps",
        spec_ref: "Response Header — Token",
        run: processing_sequence::token_wraps,
    },
    Scenario {
        name: "conformance.rom_types.served_type_from_ram_slot_info",
        spec_ref: "ROM Types; GET_RAM_SLOT_INFO Response Format",
        run: rom_types::served_type_from_ram_slot_info,
    },
    Scenario {
        name: "conformance.rom_types.flash_slot_types_from_flash_slot_info",
        spec_ref: "ROM Types; GET_FLASH_SLOT_INFO Response Format",
        run: rom_types::flash_slot_types_from_flash_slot_info,
    },
    Scenario {
        name: "conformance.rom_types.flash_slot_types_from_flash_slot_info_all",
        spec_ref: "ROM Types; GET_FLASH_SLOT_INFO_ALL Response Format",
        run: rom_types::flash_slot_types_from_flash_slot_info_all,
    },
    Scenario {
        name: "conformance.ring.overflow_then_recovers",
        spec_ref: "Device robustness — host outpacing the capture ring",
        run: ring::overflow_then_recovers,
    },
];
