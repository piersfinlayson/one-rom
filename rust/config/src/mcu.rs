// Copyright (C) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT License

pub const STM32F4_BASE_FLASH: u32 = 0x0800_0000;
pub const RP235X_BASE_FLASH: u32 = 0x1000_0000;
pub const RP235X_END_FLASH: u32 = 0x1FFF_FFFF;
pub const RP235X_SRAM_SIZE_KB: usize = 520;
pub const RP235X_BASE_SRAM: u32 = 0x2000_0000;
pub const RP235X_END_SRAM: u32 = RP235X_BASE_SRAM + (RP235X_SRAM_SIZE_KB as u32 * 1024);

/// MCU family
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Family {
    /// STM32F4 series
    #[serde(rename = "stm32f4")]
    Stm32f4,
    /// Raspberry Pi RP2350
    #[serde(rename = "rp2350")]
    Rp2350,
}

impl Family {
    pub const fn get_flash_base(&self) -> u32 {
        match self {
            Family::Stm32f4 => STM32F4_BASE_FLASH,
            Family::Rp2350 => RP235X_BASE_FLASH,
        }
    }

    pub fn try_from_str(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("stm32f4") {
            Some(Family::Stm32f4)
        } else if s.eq_ignore_ascii_case("rp2350") {
            Some(Family::Rp2350)
        } else {
            None
        }
    }
}

impl core::fmt::Display for Family {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Family::Stm32f4 => write!(f, "STM32F4"),
            Family::Rp2350 => write!(f, "RP2350"),
        }
    }
}

/// GPIO Port designation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum Port {
    /// No port (unused)
    None,
    /// Port 0 (RP2350)
    Zero,
    /// Port A (STM32)
    A,
    /// Port B (STM32)
    B,
    /// Port C (STM32)
    C,
    /// Port D (STM32)
    D,
}

impl core::fmt::Display for Port {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Port::None => write!(f, "PORT_NONE"),
            Port::Zero => write!(f, "PORT_0"),
            Port::A => write!(f, "PORT_A"),
            Port::B => write!(f, "PORT_B"),
            Port::C => write!(f, "PORT_C"),
            Port::D => write!(f, "PORT_D"),
        }
    }
}

// Values match C enum values in core firmware
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum RpVariant {
    Rp235xA = 1,
    Rp235xB = 0,
}

impl core::fmt::Display for RpVariant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RpVariant::Rp235xA => write!(f, "RP235xA"),
            RpVariant::Rp235xB => write!(f, "RP235xB"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Processor {
    F401BC,
    F401DE,
    F405,
    F411,
    F446,
    RP2350,
}

impl Processor {
    pub fn vco_min_mhz(&self) -> u32 {
        match self {
            Processor::F401BC => 192,
            Processor::F401DE => 192,
            Processor::F405 => 100,
            Processor::F411 => 100,
            Processor::F446 => 100,
            Processor::RP2350 => 750,
        }
    }

    pub fn vco_max_mhz(&self, overclock: bool) -> u32 {
        if !overclock {
            match self {
                Processor::RP2350 => 1600,
                Processor::F401BC
                | Processor::F401DE
                | Processor::F405
                | Processor::F411
                | Processor::F446 => 432,
            }
        } else {
            match self {
                Processor::RP2350 => 1600,
                Processor::F401BC
                | Processor::F401DE
                | Processor::F405
                | Processor::F411
                | Processor::F446 => 1000,
            }
        }
    }

    pub fn max_sysclk_mhz(&self) -> u32 {
        match self {
            Processor::F401BC => 84,
            Processor::F401DE => 84,
            Processor::F405 => 168,
            Processor::F411 => 100,
            Processor::F446 => 180,
            Processor::RP2350 => 150,
        }
    }
}

pub const MCU_VARIANTS: &[Variant] = &[
    Variant::F401RB,
    Variant::F401RC,
    Variant::F401RE,
    Variant::F405RG,
    Variant::F411RC,
    Variant::F411RE,
    Variant::F446RC,
    Variant::F446RE,
    Variant::RP2350,
    Variant::RP2350B,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Variant {
    F446RC,  // STM32F446RC (6 or 7), 64-pins, 128KB SRAM, 256KB Flash
    F446RE,  // STM32F446RE (6 or 7), 64-pins, 128KB SRAM, 512KB Flash
    F411RC,  // STM32F411RC (6 or 7), 64-pins, 128KB SRAM, 256KB Flash
    F411RE,  // STM32F411RE (6 or 7), 64-pins, 128KB SRAM, 512KB Flash
    F405RG,  // STM32F405RE (6 or 7), 64-pins, 128KB SRAM, 1024KB Flash (+ 64KB CCM RAM)
    F401RE,  // STM32F401RE (6 or 7), 64-pins, 96KB SRAM, 512KB Flash
    F401RB,  // STM32F401RB (6 or 7), 64-pins, 64KB SRAM, 128KB Flash
    F401RC,  // STM32F401RC (6 or 7), 64-pins, 96KB SRAM, 256KB Flash
    RP2350,  // RP2350A, 60-pin, 2MB flash
    RP2350B, // RP2350B, 80-pin, 2MB flash
}

impl core::fmt::Display for Variant {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Variant::F446RC => write!(f, "F446RC"),
            Variant::F446RE => write!(f, "F446RE"),
            Variant::F411RC => write!(f, "F411RC"),
            Variant::F411RE => write!(f, "F411RE"),
            Variant::F405RG => write!(f, "F405RG"),
            Variant::F401RE => write!(f, "F401RE"),
            Variant::F401RB => write!(f, "F401RB"),
            Variant::F401RC => write!(f, "F401RC"),
            Variant::RP2350 => write!(f, "RP2350"),
            Variant::RP2350B => write!(f, "RP2350B"),
        }
    }
}

impl Variant {
    pub fn try_from_str(s: &str) -> Option<Self> {
        if s.eq_ignore_ascii_case("f446rc") {
            Some(Variant::F446RC)
        } else if s.eq_ignore_ascii_case("f446re") {
            Some(Variant::F446RE)
        } else if s.eq_ignore_ascii_case("f411rc") {
            Some(Variant::F411RC)
        } else if s.eq_ignore_ascii_case("f411re") {
            Some(Variant::F411RE)
        } else if s.eq_ignore_ascii_case("f405rg") {
            Some(Variant::F405RG)
        } else if s.eq_ignore_ascii_case("f401re") {
            Some(Variant::F401RE)
        } else if s.eq_ignore_ascii_case("f401rb") {
            Some(Variant::F401RB)
        } else if s.eq_ignore_ascii_case("f401rc") {
            Some(Variant::F401RC)
        } else if s.eq_ignore_ascii_case("rp2350") {
            Some(Variant::RP2350)
        } else if s.eq_ignore_ascii_case("rp2350b") {
            Some(Variant::RP2350B)
        } else {
            None
        }
    }

    pub fn line_enum(&self) -> &str {
        match self {
            Variant::F446RC | Variant::F446RE => "F446",
            Variant::F411RC | Variant::F411RE => "F411",
            Variant::F405RG => "F405",
            Variant::F401RE => "F401DE",
            Variant::F401RB | Variant::F401RC => "F401BC",
            Variant::RP2350 => "RP2350_LINE",
            Variant::RP2350B => "RP2350_LINE",
        }
    }

    pub fn storage_enum(&self) -> &str {
        match self {
            Variant::F446RC => "STORAGE_C",
            Variant::F446RE => "STORAGE_E",
            Variant::F411RC => "STORAGE_C",
            Variant::F411RE => "STORAGE_E",
            Variant::F405RG => "STORAGE_G",
            Variant::F401RE => "STORAGE_E",
            Variant::F401RB => "STORAGE_B",
            Variant::F401RC => "STORAGE_C",
            Variant::RP2350 => "STORAGE_2MB",
            Variant::RP2350B => "STORAGE_2MB",
        }
    }

    pub fn flash_storage_bytes(&self) -> usize {
        self.flash_storage_kb() * 1024
    }

    pub fn flash_storage_kb(&self) -> usize {
        match self {
            Variant::F446RC => 256,
            Variant::F446RE => 512,
            Variant::F411RC => 256,
            Variant::F411RE => 512,
            Variant::F405RG => 1024,
            Variant::F401RB => 128,
            Variant::F401RC => 256,
            Variant::F401RE => 512,
            Variant::RP2350 => 2048,
            Variant::RP2350B => 2048,
        }
    }

    pub fn ram_bytes(&self) -> usize {
        self.ram_kb() * 1024
    }

    pub fn ram_kb(&self) -> usize {
        match self {
            Variant::F446RC | Variant::F446RE => 128,
            Variant::F411RC | Variant::F411RE => 128,
            Variant::F405RG => 128, // +64KB CCM RAM
            Variant::F401RB | Variant::F401RC => 64,
            Variant::F401RE => 96,
            Variant::RP2350 => RP235X_SRAM_SIZE_KB,
            Variant::RP2350B => RP235X_SRAM_SIZE_KB,
        }
    }

    pub fn supports_usb_dfu(&self) -> bool {
        match self.family() {
            Family::Stm32f4 => true,
            Family::Rp2350 => true,
        }
    }

    pub fn supports_banked_roms(&self) -> bool {
        // 72 KB RAM as requires:
        // - 64KB for total of 4 16KB banked images
        // - 4KB for logging buffer
        // - 4KB for everything else
        //
        // 96KB flash as requires:
        // - 64KB for total of 1 set of 4x16KB banked images
        // - 32KB for firmware
        self.ram_kb() > 72 && self.flash_storage_kb() >= 96
    }

    pub fn supports_multi_rom_sets(&self) -> bool {
        // Same criteria as banked roms
        self.supports_banked_roms()
    }

    pub fn ccm_ram_kb(&self) -> Option<usize> {
        // F405 has 64KB of CCM RAM, others don't
        match self {
            Variant::F405RG => Some(64),
            _ => None,
        }
    }

    pub fn define_var_sub_fam(&self) -> &str {
        match self {
            Variant::F446RC | Variant::F446RE => "#define STM32F446      1",
            Variant::F411RC | Variant::F411RE => "#define STM32F411      1",
            Variant::F405RG => "#define STM32F405      1",
            Variant::F401RE => "#define STM32F401DE    1",
            Variant::F401RB | Variant::F401RC => "#define STM32F401BC    1",
            Variant::RP2350 => "#define RP2350A        1",
            Variant::RP2350B => "#define RP2350B        1",
        }
    }

    pub fn family(&self) -> Family {
        match self {
            Variant::F446RC
            | Variant::F446RE
            | Variant::F411RC
            | Variant::F411RE
            | Variant::F405RG
            | Variant::F401RE
            | Variant::F401RB
            | Variant::F401RC => Family::Stm32f4,
            Variant::RP2350 => Family::Rp2350,
            Variant::RP2350B => Family::Rp2350,
        }
    }

    pub fn processor(&self) -> Processor {
        match self {
            Variant::F446RC | Variant::F446RE => Processor::F446,
            Variant::F411RC | Variant::F411RE => Processor::F411,
            Variant::F405RG => Processor::F405,
            Variant::F401RE => Processor::F401DE,
            Variant::F401RB | Variant::F401RC => Processor::F401BC,
            Variant::RP2350 => Processor::RP2350,
            Variant::RP2350B => Processor::RP2350,
        }
    }

    pub fn define_var_fam(&self) -> &str {
        match self.family() {
            Family::Stm32f4 => "#define STM32F4        1",
            Family::Rp2350 => "#define RP235X         1",
        }
    }

    pub fn define_var_str(&self) -> &str {
        match self {
            Variant::F446RC => "#define MCU_VARIANT    \"F446RC\"",
            Variant::F446RE => "#define MCU_VARIANT    \"F446RE\"",
            Variant::F411RC => "#define MCU_VARIANT    \"F411RC\"",
            Variant::F411RE => "#define MCU_VARIANT    \"F411RE\"",
            Variant::F405RG => "#define MCU_VARIANT    \"F405RG\"",
            Variant::F401RE => "#define MCU_VARIANT    \"F401RE\"",
            Variant::F401RB => "#define MCU_VARIANT    \"F401RB\"",
            Variant::F401RC => "#define MCU_VARIANT    \"F401RC\"",
            Variant::RP2350 => "#define MCU_VARIANT    \"RP2350\"",
            Variant::RP2350B => "#define MCU_VARIANT    \"RP2350B\"",
        }
    }

    /// Used to pass into sdrr Makefile as VARIANT
    pub fn makefile_var(&self) -> &str {
        match self {
            Variant::F446RC => "stm32f446rc",
            Variant::F446RE => "stm32f446re",
            Variant::F411RC => "stm32f411rc",
            Variant::F411RE => "stm32f411re",
            Variant::F405RG => "stm32f405rg",
            Variant::F401RE => "stm32f401re",
            Variant::F401RB => "stm32f401rb",
            Variant::F401RC => "stm32f401rc",
            Variant::RP2350 => "rp2350",
            Variant::RP2350B => "rp2350",
        }
    }

    /// Used to pass to probe-rs
    pub fn chip_id(&self) -> &str {
        match self {
            Variant::F446RC => "STM32F446RCTx",
            Variant::F446RE => "STM32F446RETx",
            Variant::F411RC => "STM32F411RCTx",
            Variant::F411RE => "STM32F411RETx",
            Variant::F405RG => "STM32F405RGTx",
            Variant::F401RE => "STM32F401RETx",
            Variant::F401RB => "STM32F401RBTx",
            Variant::F401RC => "STM32F401RCTx",
            Variant::RP2350 => "RP235X",
            Variant::RP2350B => "RP235X",
        }
    }
}
