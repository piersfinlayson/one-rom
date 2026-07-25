// This test file uses std - it's separate from the no_std library
include!("../build/chip/validation.rs");

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_chip_type_2316() -> ChipType {
        let mut control = BTreeMap::new();
        control.insert(
            "cs1".to_string(),
            ControlLine {
                pin: 20,
                line_type: ControlLineType::Configurable,
                allow_ignore: false,
            },
        );

        ChipType {
            description: "Test 2316".to_string(),
            function: ChipFunction::Rom,
            supported: Some("0.2.0".to_string()),
            aliases: None,
            bit_modes: vec![8],
            pins: 24,
            size: 2048,
            address: vec![8, 7, 6, 5, 4, 3, 2, 1, 23, 22, 19],
            data: vec![9, 10, 11, 13, 14, 15, 16, 17],
            control,
            programming: None,
            power: None,
            deselect_when_address_all_high: None,
            allow_mixed_control: false,
            rbcp_chip_type: 0,
        }
    }

    fn create_test_chip_type_27128() -> ChipType {
        let mut control = BTreeMap::new();
        control.insert(
            "ce".to_string(),
            ControlLine {
                pin: 20,
                line_type: ControlLineType::FixedActiveLow,
                allow_ignore: false,
            },
        );
        control.insert(
            "oe".to_string(),
            ControlLine {
                pin: 22,
                line_type: ControlLineType::FixedActiveLow,
                allow_ignore: false,
            },
        );

        ChipType {
            description: "Test 27128".to_string(),
            function: ChipFunction::Rom,
            supported: Some("0.2.0".to_string()),
            aliases: None,
            pins: 28,
            bit_modes: vec![8, 16],
            size: 16384,
            address: vec![10, 9, 8, 7, 6, 5, 4, 3, 25, 24, 21, 23, 2, 26],
            data: vec![11, 12, 13, 15, 16, 17, 18, 19],
            control,
            power: None,
            programming: Some(ProgrammingConfig {
                vpp: Some(ProgrammingPin {
                    pin: 1,
                    read_state: "vcc".to_string(),
                }),
                pgm: Some(ProgrammingPin {
                    pin: 27,
                    read_state: "high".to_string(),
                }),
                pe: None,
            }),
            deselect_when_address_all_high: None,
            allow_mixed_control: false,
            rbcp_chip_type: 11,
        }
    }

    #[test]
    fn test_valid_2316_config() {
        let chip_type = create_test_chip_type_2316();
        assert!(chip_type.validate("2316").is_ok());
    }

    #[test]
    fn test_valid_27128_config() {
        let chip_type = create_test_chip_type_27128();
        assert!(chip_type.validate("27128").is_ok());
    }

    #[test]
    fn test_address_size_mismatch() {
        let mut chip_type = create_test_chip_type_2316();
        chip_type.size = 3000;
        let result = chip_type.validate("test");
        assert!(matches!(
            result,
            Err(ValidationError::AddressSizeMismatch { .. })
        ));
    }

    #[test]
    fn test_invalid_pin_number() {
        let mut chip_type = create_test_chip_type_2316();
        chip_type.address[0] = 25;
        let result = chip_type.validate("test");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPinNumber { .. })
        ));
    }

    #[test]
    fn test_duplicate_pin() {
        let mut chip_type = create_test_chip_type_2316();
        chip_type.data[0] = chip_type.address[0];
        let result = chip_type.validate("test");
        assert!(matches!(result, Err(ValidationError::DuplicatePin { .. })));
    }

    #[test]
    fn test_invalid_data_line_count() {
        let mut chip_type = create_test_chip_type_2316();
        chip_type.data = vec![9, 10, 11];
        let result = chip_type.validate("test");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidDataLineCount { .. })
        ));
    }

    #[test]
    fn test_invalid_read_state() {
        let mut chip_type = create_test_chip_type_27128();
        chip_type
            .programming
            .as_mut()
            .unwrap()
            .vpp
            .as_mut()
            .unwrap()
            .read_state = "invalid".to_string();
        let result = chip_type.validate("test");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidReadState { .. })
        ));
    }

    #[test]
    fn test_invalid_package_pin_count() {
        let mut chip_type = create_test_chip_type_2316();
        chip_type.pins = 41;
        let result = chip_type.validate("test");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPackagePinCount { .. })
        ));
    }

    #[test]
    fn test_parse_real_json_config() {
        let json = include_bytes!("../json/chip-types.json");

        let config = ChipTypesConfig::from_json(&String::from_utf8_lossy(json))
            .expect("Failed to parse JSON config");

        let chip_2364 = config.chip_types.get("2364").unwrap();
        assert_eq!(chip_2364.pins, 24);
        assert_eq!(chip_2364.size, 8192);
        assert_eq!(chip_2364.address.len(), 13);
        assert_eq!(chip_2364.control.len(), 1);
        assert!(chip_2364.control.contains_key("cs1"));
        assert_eq!(
            chip_2364.control.get("cs1").unwrap().line_type,
            ControlLineType::Configurable
        );

        let chip_27512 = config.chip_types.get("27512").unwrap();
        assert_eq!(chip_27512.size, 65536);
        assert_eq!(chip_27512.address.len(), 16);
        assert_eq!(chip_27512.address[15], 1);
        assert_eq!(chip_27512.control.len(), 2);
        assert!(chip_27512.programming.is_some());
    }
}
