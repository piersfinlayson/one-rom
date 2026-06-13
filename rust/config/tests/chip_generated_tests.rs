// config/tests/generated_tests.rs

use onerom_config::chip::{ChipType, ControlLineType, ProgrammingPinState};

#[test]
fn test_chip_2316_specs() {
    let chip = ChipType::Chip2316;
    assert_eq!(chip.name(), "2316");
    assert_eq!(chip.size_bytes(), 2048);
    assert_eq!(chip.chip_pins(), 24);
    assert_eq!(chip.num_addr_lines(), 11);
    assert_eq!(chip.address_pins().len(), 11);
    assert_eq!(chip.data_pins().len(), 8);

    let control = chip.control_lines();
    assert_eq!(control.len(), 3);
    assert!(control.iter().any(|c| c.name == "cs1"));
    assert!(
        control
            .iter()
            .all(|c| c.line_type == ControlLineType::Configurable)
    );
}

#[test]
fn test_chip_2364_specs() {
    let chip = ChipType::Chip2364;
    assert_eq!(chip.name(), "2364");
    assert_eq!(chip.size_bytes(), 8192);
    assert_eq!(chip.chip_pins(), 24);
    assert_eq!(chip.num_addr_lines(), 13);

    let control = chip.control_lines();
    assert_eq!(control.len(), 1);
    assert_eq!(control[0].name, "cs1");
    assert_eq!(control[0].pin, 20);
}

#[test]
fn test_chip_27128_specs() {
    let chip = ChipType::Chip27128;
    assert_eq!(chip.name(), "27128");
    assert_eq!(chip.size_bytes(), 16384);
    assert_eq!(chip.chip_pins(), 28);
    assert_eq!(chip.num_addr_lines(), 14);

    let control = chip.control_lines();
    assert_eq!(control.len(), 2);
    assert!(
        control
            .iter()
            .any(|c| c.name == "ce" && c.line_type == ControlLineType::FixedActiveLow)
    );
    assert!(
        control
            .iter()
            .any(|c| c.name == "oe" && c.line_type == ControlLineType::FixedActiveLow)
    );

    let prog = chip.programming_pins().unwrap();
    assert_eq!(prog.len(), 2);
    let vpp = prog.iter().find(|p| p.name == "vpp").unwrap();
    assert_eq!(vpp.read_state, ProgrammingPinState::Vcc);
}

#[test]
fn test_chip_27512_specs() {
    let chip = ChipType::Chip27512;
    assert_eq!(chip.size_bytes(), 65536);
    assert_eq!(chip.num_addr_lines(), 16);

    // Pin 1 is A15
    assert!(chip.programming_pins().is_some());

    let addr = chip.address_pins();
    assert_eq!(addr[15], 1); // A15 on pin 1
}

#[test]
fn test_try_from_str() {
    assert_eq!(ChipType::try_from_str("2364"), Some(ChipType::Chip2364));
    assert_eq!(ChipType::try_from_str("27128"), Some(ChipType::Chip27128));
    assert_eq!(ChipType::try_from_str("27512"), Some(ChipType::Chip27512));
    assert_eq!(ChipType::try_from_str("invalid"), None);
}

#[test]
fn test_all_chip_types_parse() {
    let types = ["2316", "2332", "2364", "23128", "27128", "27256", "27512"];
    for type_name in types {
        assert!(
            ChipType::try_from_str(type_name).is_some(),
            "Failed to parse {}",
            type_name
        );
    }
}
