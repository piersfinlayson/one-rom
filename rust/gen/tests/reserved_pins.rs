// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Tests for `reserved_pins`.

use onerom_config::fw::{FirmwareProperties, FirmwareVersion, ServeAlg};
use onerom_config::hw::Board;
use onerom_config::mcu::{Family, Variant};
use onerom_config::pin::{Pad, Pin, ReservedPads};
use onerom_gen::{Builder, Error, FileData, MIN_RESERVED_PINS_VERSION};
use onerom_metadata::{
    DeviceMemoryView, METADATA_BASE, ONEROM_METADATA_HEADER_RESERVED_SEL_PINS_OFFSET,
    ONEROM_METADATA_HEADER_RESERVED_X_PINS_OFFSET, metadata_generation_for,
};

const V0_8_0: FirmwareVersion = FirmwareVersion::new(0, 8, 0, 0);

fn config(reserved: &[&str], sets: &[String]) -> String {
    let reserved: Vec<String> = reserved.iter().map(|pin| format!("\"{pin}\"")).collect();
    format!(
        r#"{{ "version": 1, "description": "test", "reserved_pins": [{}], "chip_sets": [{}] }}"#,
        reserved.join(", "),
        sets.join(", ")
    )
}

fn single(chip_type: &str) -> String {
    format!(
        r#"{{ "type": "single", "chips": [{{ "file": "a.bin", "description": "{chip_type}", "type": "{chip_type}", "cs1": "active_low" }}] }}"#
    )
}

fn banked(chips: usize) -> String {
    let chip = r#"{ "file": "a.bin", "type": "2364", "cs1": "active_low" }"#;
    format!(
        r#"{{ "type": "banked", "chips": [{}] }}"#,
        vec![chip; chips].join(", ")
    )
}

/// A 2764 on a 24-pin board has A12 on X1.
fn fly_lead_2764() -> String {
    r#"{ "type": "single", "chips": [{ "file": "a.bin", "type": "2764" }] }"#.to_string()
}

fn build(version: FirmwareVersion, board: Board, json: &str) -> Result<Vec<u8>, Error> {
    let mut builder = Builder::from_json(version, Family::Rp2350, json)?;
    builder.add_file(FileData::new(0, vec![0xEA; 8192]))?;
    let props =
        FirmwareProperties::new(version, board, Variant::RP2350, ServeAlg::Default, false).unwrap();
    builder.build(props).map(|(metadata, _)| metadata)
}

fn reserved_bits(metadata: &[u8]) -> (u8, u8) {
    let view = DeviceMemoryView::new(metadata, METADATA_BASE);
    let at = |offset: usize| view.read_u8(METADATA_BASE + offset as u32).unwrap();
    (
        at(ONEROM_METADATA_HEADER_RESERVED_SEL_PINS_OFFSET),
        at(ONEROM_METADATA_HEADER_RESERVED_X_PINS_OFFSET),
    )
}

#[test]
fn reserved_pins_take_every_pin_spelling() {
    let json = config(&["SEL-C", " x1 ", "gpio24"], &[single("2364")]);
    let builder = Builder::from_json(V0_8_0, Family::Rp2350, &json).unwrap();
    assert_eq!(
        builder.config().reserved_pins,
        [Pin::Pad(Pad::Select(2)), Pin::Pad(Pad::X1), Pin::Gpio(24)]
    );
    let saved = serde_json::to_value(builder.config()).unwrap();
    assert_eq!(
        saved["reserved_pins"],
        serde_json::json!(["sel_c", "x1", "gpio24"])
    );
}

#[test]
fn a_config_without_reserved_pins_writes_none() {
    let builder =
        Builder::from_json(V0_8_0, Family::Rp2350, &config(&[], &[single("2364")])).unwrap();
    let saved = serde_json::to_value(builder.config()).unwrap();
    assert!(saved.get("reserved_pins").is_none(), "{saved}");
    let metadata = build(V0_8_0, Board::Fire24F, &config(&[], &[single("2364")])).unwrap();
    assert_eq!(reserved_bits(&metadata), (0, 0));
}

#[test]
fn an_entry_that_isnt_a_pin_name_fails_to_parse() {
    for entry in ["banana", "a17", "23", ""] {
        let json = config(&[entry], &[single("2364")]);
        match Builder::from_json(V0_8_0, Family::Rp2350, &json) {
            Err(Error::ReservedPinNotAPin { entry: e }) => assert_eq!(e, entry),
            other => panic!("{entry}: {other:?}"),
        }
    }
}

/// The metadata has one bit per reserved pin, however the pin is written.
#[test]
fn the_metadata_holds_the_reserved_pads() {
    // gpio8 is wired to X2 on a fire-24-f.
    let json = config(&["sel_c", "gpio8", "x2"], &[single("2364")]);
    let metadata = build(V0_8_0, Board::Fire24F, &json).unwrap();
    assert_eq!(reserved_bits(&metadata), (0b0100, 0b10));

    let view = DeviceMemoryView::new(&metadata, METADATA_BASE);
    assert_eq!(
        view.read_u32_le(METADATA_BASE + 16).unwrap(),
        metadata_generation_for(V0_8_0).unwrap()
    );
}

/// One version per builder, v1 and v2.
#[test]
fn older_firmware_fails_with_reserved_pins() {
    for version in [
        FirmwareVersion::new(0, 7, 3, 0),
        FirmwareVersion::new(0, 6, 0, 0),
    ] {
        let json = config(&["sel_c"], &[single("2364")]);
        match Builder::from_json(version, Family::Rp2350, &json) {
            Err(Error::FirmwareTooOld {
                feat,
                version: v,
                minimum,
            }) => {
                assert_eq!(feat, "reserved_pins");
                assert_eq!(v, version);
                assert_eq!(minimum, MIN_RESERVED_PINS_VERSION);
            }
            other => panic!("{version}: {other:?}"),
        }
        assert!(
            Builder::from_json(version, Family::Rp2350, &config(&[], &[single("2364")])).is_ok()
        );
    }
}

#[test]
fn the_minimum_version_is_where_the_metadata_holds_the_pads() {
    let before = FirmwareVersion::new(0, 7, 3, 0);
    assert!(before < MIN_RESERVED_PINS_VERSION);
    assert!(metadata_generation_for(before) < metadata_generation_for(MIN_RESERVED_PINS_VERSION));
    let json = config(&["x1"], &[single("2364")]);
    let metadata = build(MIN_RESERVED_PINS_VERSION, Board::Fire24F, &json).unwrap();
    assert_eq!(reserved_bits(&metadata), (0, 0b01));
}

/// A pin the board doesn't have and a GPIO not wired to a reservable pin are
/// different errors.
#[test]
fn only_a_pin_the_board_has_can_be_reserved() {
    let c27c400 = r#"{ "type": "single", "chips": [{ "file": "a.bin", "type": "27C400" }] }"#;
    for (board, entry, set, on_board) in [
        // gpio23 isn't wired to a reservable pin on a fire-24-f.
        (Board::Fire24F, "gpio23", single("2364"), true),
        // A fire-24-f doesn't have a SEL_E pin.
        (Board::Fire24F, "sel_e", single("2364"), false),
        // A fire-40-a doesn't have X pins.
        (Board::Fire40A, "x1", c27c400.to_string(), false),
    ] {
        match build(V0_8_0, board, &config(&[entry], &[set])) {
            Err(Error::ReservedPinNotAPad { pin, board: b }) if on_board => {
                assert_eq!((pin.to_string(), b), (entry.to_string(), board));
            }
            Err(Error::ReservedPinNotOnBoard { pin, board: b }) if !on_board => {
                assert_eq!((pin.to_string(), b), (entry.to_string(), board));
            }
            other => panic!("{board} {entry}: {other:?}"),
        }
    }
}

/// A banked set uses X1 for two banks and also X2 for three or four.
#[test]
fn a_banked_set_fails_where_it_uses_a_reserved_x_pad() {
    let in_use = |reserved: &str, set: String| match build(
        V0_8_0,
        Board::Fire24F,
        &config(&[reserved], &[single("2364"), set]),
    ) {
        Err(Error::ReservedPinInUse { slot, pad }) => Some((slot, pad)),
        Ok(_) => None,
        Err(e) => panic!("{e:?}"),
    };

    assert_eq!(in_use("x1", banked(2)), Some((1, "X1".to_string())));
    assert_eq!(in_use("x2", banked(2)), None);
    assert_eq!(in_use("x2", banked(3)), Some((1, "X2".to_string())));
    assert_eq!(in_use("x2", banked(4)), Some((1, "X2".to_string())));
}

#[test]
fn a_reserved_x1_is_not_replaced_by_x2() {
    let json = config(&["x1"], &[banked(2)]);
    assert!(matches!(
        build(V0_8_0, Board::Fire24F, &json),
        Err(Error::ReservedPinInUse { slot: 0, .. })
    ));
}

/// A chip larger than the socket has its extra address lines on X1 and X2.
#[test]
fn a_fly_lead_set_fails_where_it_uses_a_reserved_x_pad() {
    let json = config(&["x1"], &[fly_lead_2764()]);
    match build(V0_8_0, Board::Fire24F, &json) {
        Err(Error::ReservedPinInUse { slot: 0, pad }) => assert_eq!(pad, "X1"),
        other => panic!("{other:?}"),
    }
    // A 2764 has one extra address line, on X1.
    assert!(build(V0_8_0, Board::Fire24F, &config(&["x2"], &[fly_lead_2764()])).is_ok());
}

/// A slot never uses an image select pin.
#[test]
fn reserving_select_pads_builds_every_set() {
    let json = config(
        &["sel_a", "sel_b", "sel_c", "sel_d"],
        &[single("2364"), banked(4), fly_lead_2764()],
    );
    let metadata = build(V0_8_0, Board::Fire24F, &json).unwrap();
    assert_eq!(reserved_bits(&metadata), (0b1111, 0));
}

/// On these boards a 2364's address window includes X1 and X2, whose inputs
/// are forced to 0 while serving.
#[test]
fn a_reserved_x_pin_inside_the_address_window_builds() {
    for board in [Board::Fire24A, Board::Fire24Eadb01, Board::Fire24UsbB] {
        let metadata = build(V0_8_0, board, &config(&["x1", "x2"], &[single("2364")]))
            .unwrap_or_else(|e| panic!("{board}: {e:?}"));
        assert_eq!(reserved_bits(&metadata), (0, 0b11), "{board}");
    }
}

#[test]
fn a_slot_using_a_reserved_x_pin_fails_on_every_board() {
    for board in [Board::Fire24A, Board::Fire24Eadb01, Board::Fire24UsbB] {
        for (reserved, set) in [
            ("x1", banked(2)),
            ("x2", banked(3)),
            ("x1", fly_lead_2764()),
        ] {
            match build(V0_8_0, board, &config(&[reserved], &[set])) {
                Err(Error::ReservedPinInUse { slot: 0, pad }) => {
                    assert_eq!(pad, reserved.to_uppercase(), "{board}")
                }
                other => panic!("{board} {reserved}: {other:?}"),
            }
        }
    }
}

/// A multi-chip set uses X1 for a second chip and X2 for a third.
#[test]
fn a_multi_chip_set_fails_where_it_uses_a_reserved_x_pin() {
    let chip = r#"{ "file": "a.bin", "type": "2364", "cs1": "active_low" }"#;
    let multi = format!(r#"{{ "type": "multi", "chips": [{chip}, {chip}] }}"#);
    match build(
        V0_8_0,
        Board::Fire24F,
        &config(&["x1"], std::slice::from_ref(&multi)),
    ) {
        Err(Error::ReservedPinInUse { slot: 0, pad }) => assert_eq!(pad, "X1"),
        other => panic!("{other:?}"),
    }
    assert!(build(V0_8_0, Board::Fire24F, &config(&["x2"], &[multi])).is_ok());
}

#[test]
fn a_slot_after_a_plugin_is_numbered_without_it() {
    let plugin =
        r#"{ "type": "single", "chips": [{ "file": "usb.bin", "type": "system_plugin" }] }"#;
    let json = config(&["x1"], &[plugin.to_string(), single("2364"), banked(2)]);
    let mut builder = Builder::from_json(V0_8_0, Family::Rp2350, &json).unwrap();
    // A minimal valid plugin image.
    let mut plugin_image = vec![0u8; 256];
    plugin_image[0..4].copy_from_slice(b"ORA ");
    plugin_image[4..8].copy_from_slice(&1u32.to_le_bytes());
    builder.add_file(FileData::new(0, plugin_image)).unwrap();
    builder
        .add_file(FileData::new(1, vec![0xEA; 8192]))
        .unwrap();
    let props = FirmwareProperties::new(
        V0_8_0,
        Board::Fire24F,
        Variant::RP2350,
        ServeAlg::Default,
        false,
    )
    .unwrap();
    match builder.build(props) {
        Err(Error::ReservedPinInUse { slot, pad }) => assert_eq!((slot, pad.as_str()), (1, "X1")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_names_for_one_pad_reserve_it_once() {
    let json = config(&["sel_c", "gpio25", "x1", "gpio9"], &[single("2364")]);
    let builder = Builder::from_json(V0_8_0, Family::Rp2350, &json).unwrap();
    let reserved = builder.config().reserved_pads(Board::Fire24F).unwrap();
    assert_eq!(reserved, ReservedPads::from_bits(0b100, 0b01));
}

fn description(board: Board, json: &str) -> String {
    Builder::from_json(V0_8_0, Family::Rp2350, json)
        .unwrap()
        .description_for_board(board)
        .unwrap()
}

fn image_lines(description: &str) -> Vec<String> {
    description
        .lines()
        .skip_while(|line| *line != "Images:")
        .skip(1)
        .map(str::to_string)
        .collect()
}

/// With SEL_C reserved on a fire-24-f, SEL_D is image select bit 2.
#[test]
fn each_image_line_marks_the_jumpers_that_select_it() {
    let sets: Vec<String> = (0..5).map(|_| single("2364")).collect();
    let desc = description(Board::Fire24F, &config(&["sel_c"], &sets));
    assert!(desc.contains("Reserved pins: SEL_C\n\nImages:"), "{desc}");
    let lines = image_lines(&desc);
    assert_eq!(lines[0], "      D C B A");
    let marks: Vec<String> = lines[1..]
        .iter()
        .map(|line| line.chars().skip(3).take(11).collect())
        .collect();
    assert_eq!(
        marks,
        [
            "[· · · · ·]",
            "[· · · · ▪]",
            "[· · · ▪ ·]",
            "[· · · ▪ ▪]",
            "[· ▪ · · ·]"
        ]
    );
    assert!(lines[4].ends_with("SEL_A and SEL_B jumpered"), "{desc}");
    assert!(lines[5].ends_with("SEL_D jumpered"), "{desc}");
    assert!(lines[1].ends_with("no image select jumpers"), "{desc}");
}

/// The first image after a plugin set is selected with every jumper open.
#[test]
fn plugin_lines_have_no_marks() {
    let plugin =
        r#"{ "type": "single", "chips": [{ "file": "usb.bin", "type": "system_plugin" }] }"#;
    let json = config(&[], &[plugin.to_string(), single("2364"), single("2364")]);
    let lines = image_lines(&description(Board::Fire24F, &json));
    assert_eq!(lines[1], "0: usb.bin");
    assert!(lines[2].starts_with("1: [· · · · ·]"), "{lines:?}");
    assert!(lines[3].starts_with("2: [· · · · ▪]"), "{lines:?}");
}

/// With two image select pins read, a fifth image can't be selected.
#[test]
fn an_image_beyond_the_jumpers_cannot_be_selected() {
    let sets: Vec<String> = (0..5).map(|_| single("2364")).collect();
    let json = config(&["sel_c", "sel_d"], &sets);
    let lines = image_lines(&description(Board::Fire24F, &json));
    assert!(lines[4].starts_with("3: [· · · ▪ ▪]"), "{lines:?}");
    assert_eq!(lines[5], "4: 2364  cannot be selected");
}

/// A line after the images replaces the marks in both cases.
#[test]
fn turbo_boot_and_one_image_have_no_marks() {
    let plugin =
        r#"{ "type": "single", "chips": [{ "file": "usb.bin", "type": "system_plugin" }] }"#;
    let mut json: serde_json::Value =
        serde_json::from_str(&config(&[], &[plugin.to_string(), single("2364")])).unwrap();
    json["turbo_boot"] = true.into();
    let lines = image_lines(&description(Board::Fire24F, &json.to_string()));
    assert_eq!(
        lines,
        [
            "0: usb.bin",
            "1: 2364",
            "Image select jumpers not read - turbo boot enabled. Image 1 served."
        ]
    );

    let lines = image_lines(&description(
        Board::Fire24F,
        &config(&[], &[single("2364")]),
    ));
    assert_eq!(
        lines,
        [
            "0: 2364",
            "Single image so any image select jumper may be in any position."
        ]
    );
}
