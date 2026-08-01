// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Build the per-slot algorithm config (`OneromAlgConfig`) from
//! already-derived address/CS-data layouts.
//!
//! `build_alg_addr`/`build_alg_data`/`build_alg_dma` build the
//! corresponding sub-configs directly from the layouts. `build_alg_cs`
//! (in `alg_cs.rs`), the CS-polarity overrides (`cs_overrides.rs`), and
//! the GPIO pull config (`gpio_pull_config.rs`) are wired together by
//! `build_alg_config` below.
//!
//! `build_alg_config` takes `&AddrLayout`/`&CsDataLayout` rather than
//! deriving them itself, since `build_rom_slot` needs the same layouts for
//! `build_rom_info`'s pin maps - deriving once and sharing avoids doing the
//! (fallible) derivation twice. As a result `build_alg_config` itself is
//! infallible.
//!
//! `bit_mode` (chip0's *effective* bit mode, from `bit_mode_for`) and
//! `force_16_bit` are both computed once by `build_rom_slot` and passed
//! in - `bit_mode` is also needed by `derive_addr_layout` (for the A-1
//! carve-out), so computing it once and sharing avoids the two
//! disagreeing.

use onerom_config::chip::ChipType;
use onerom_config::hw::Board;

use onerom_metadata::{
    BitModes, GPIO_NONE, GpioOverride, OneromAlgAddrConfig, OneromAlgConfig, OneromAlgDataConfig,
    OneromAlgDmaConfig, OneromAlgOverrideConfig,
};

use crate::image::CsConfig;
use crate::v2::cs_overrides::encode_override;

use super::addr_layout::AddrLayout;
use super::alg_cs::build_alg_cs;
use super::alg_preference::{
    AddrAlgPreference, CombinedAlgPreference, CsAlgPreference, DataAlgPreference, DmaAlgPreference,
};
use super::cs_data_layout::CsDataLayout;
use super::cs_overrides::{
    build_cs_overrides, build_gpio_x_overrides, build_unused_addr_overrides,
};
use super::gpio_pull_config::build_gpio_pull_config;
use super::slot_context::SlotContext;

/// H: clock dividers default to 1.0 for now.
pub const DEFAULT_CLKDIV_INT: u16 = 1;
pub const DEFAULT_CLKDIV_FRAC: u8 = 0;

const ALG_DATA0_NUM_DELAY_CYCLES_8_BIT: u8 = 2;
const ALG_DATA1_NUM_DELAY_CYCLES_16_BIT: u8 = 4;

/// Determine the bit mode (8 vs 16) for serving `chip_type` on `board`.
///
/// 16-bit serving requires both a board /BYTE pin and chip support for it.
///
/// This is independent of `force_16_bit`: for a `BitMode16`-capable chip,
/// `rom_data_buf` is always `2^num_addr_pins * 2` bytes (one 16-bit word
/// per address-PIO table index) regardless of `force_16_bit` - that flag
/// only changes *how* the chip is served (`AlgData0` vs `AlgData1`, see
/// `build_alg_data`), not the table layout/contents.
pub fn bit_mode_for(chip_type: ChipType, board: Board) -> BitModes {
    if board.pin_byte() != GPIO_NONE && chip_type.supports_bit_mode(16) {
        BitModes::BitMode16
    } else {
        BitModes::BitMode8
    }
}

/// Build `OneromAlgDataConfig` from the CS/data layout.
///
/// - `BitMode8`: `AlgData0`, `word_size: 8` - as for any other 8-bit chip.
/// - `BitMode16` with `force_16_bit`: `AlgData0`, `word_size: 16`. `/BYTE`
///   is ignored entirely (not read, not driven) - the chip is served in
///   its native 16-bit/word mode, with whatever 16-bit value DMA provides
///   written straight across `[base_data_pin, base_data_pin+16)`. Faster
///   than `AlgData1` (`build_alg_addr` gives this `num_delay_cycles=2`).
/// - `BitMode16` without `force_16_bit` (default): `AlgData1`. `byte_pin`
///   is `board.pin_byte()`, an *input* read by the data-write PIO each
///   access (driven by the host system, not One ROM) - if low, the chip
///   is in byte/8-bit mode and the host also drives A-1
///   (`a_minus_1_pin`), which the PIO reads to pick the low or high byte
///   of the 16-bit DMA value to write to `D0-D7`; if high, all 16 bits go
///   to `D0-D15` as in the `force_16_bit` case.
///
/// `a_minus_1_pin` is `cs_data_layout.data_pin_gpios[num_data_pins - 1]`
/// (the chip's highest data line, e.g. D15 for 27C400/27C200) rather than
/// independently resolved from `chip0.address_pins()[0]`: on these chips
/// A-1 and D_max are the *same physical pin*, and `derive_cs_data_layout`
/// (data-line contiguity + `fits_pio_window`) is what pins that pin to a
/// single reachable GPIO - re-deriving it via `address_pins()[0]` could in
/// principle disagree (e.g. pick the other half of a dual-bond that's
/// outside this PIO's window).
///
/// Both `byte_pin` and `a_minus_1_pin` are offsets from `layout.gpio_base`,
/// consistent with `base_data_pin` etc.
pub fn build_alg_data(
    layout: &CsDataLayout,
    board: Board,
    bit_mode: BitModes,
    force_16_bit: bool,
) -> OneromAlgDataConfig {
    match bit_mode {
        BitModes::BitMode8 => OneromAlgDataConfig::AlgData0 {
            clkdiv_int: DEFAULT_CLKDIV_INT,
            clkdiv_frac: DEFAULT_CLKDIV_FRAC,
            gpio_base: layout.gpio_base,
            base_data_pin: layout.base_data_pin,
            word_size: 8,
        },
        BitModes::BitMode16 if force_16_bit => OneromAlgDataConfig::AlgData0 {
            clkdiv_int: DEFAULT_CLKDIV_INT,
            clkdiv_frac: DEFAULT_CLKDIV_FRAC,
            gpio_base: layout.gpio_base,
            base_data_pin: layout.base_data_pin,
            word_size: 16,
        },
        BitModes::BitMode16 => {
            let byte_pin = board.pin_byte() - layout.gpio_base;
            let a_minus_1_pin =
                layout.data_pin_gpios[layout.num_data_pins as usize - 1] - layout.gpio_base;

            OneromAlgDataConfig::AlgData1 {
                clkdiv_int: DEFAULT_CLKDIV_INT,
                clkdiv_frac: DEFAULT_CLKDIV_FRAC,
                gpio_base: layout.gpio_base,
                base_data_pin: layout.base_data_pin,
                word_size: 16,
                byte_pin,
                a_minus_1_pin,
            }
        }
    }
}

/// Build `OneromAlgAddrConfig` from the address layout.
///
/// `AddrLayout::gpio_base` is the minimum GPIO of the address range,
/// which is not necessarily a PIO GPIOBASE (0 or 16). The C firmware
/// interprets `gpio_base == 0` as GPIOBASE_0 and any other value as
/// GPIOBASE_16, and uses `base_addr_pin` as IN_BASE (offset within that
/// window). This function converts accordingly:
/// - `pio_base`: 0 if min_gpio < 16, else 16.
/// - `base_addr_pin`: min_gpio - pio_base (offset within the window).
///
/// `num_delay_cycles` is 2 for `AlgData0` (8-bit, or 16-bit with
/// `force_16_bit` - neither needs the extra cycles `AlgData1`'s `/BYTE`+
/// A-1 read takes), 4 for `AlgData1`.
pub fn build_alg_addr(layout: &AddrLayout, alg_data: &OneromAlgDataConfig) -> OneromAlgAddrConfig {
    // The rom table itself uses 1 more address bit than the number of address
    // pins in 16-bit mode, which is AlgData1.  The address read algorithm
    // adds that extra bit manually as a 0.
    // using an extension of the algorithm.
    let (num_delay_cycles, num_rom_table_bits) = match alg_data {
        OneromAlgDataConfig::AlgData0 { word_size, .. } => (
            ALG_DATA0_NUM_DELAY_CYCLES_8_BIT,
            if *word_size == 8u8 {
                layout.num_addr_pins
            } else {
                layout.num_addr_pins + 1
            },
        ),
        OneromAlgDataConfig::AlgData1 { .. } => {
            (ALG_DATA1_NUM_DELAY_CYCLES_16_BIT, layout.num_addr_pins + 1)
        }
    };

    // AddrLayout::gpio_base is the min GPIO of the address range (not
    // necessarily a valid PIO GPIOBASE).  Derive the actual PIO window
    // base (0 or 16) and the IN_BASE offset within it.
    let pio_base: u8 = if layout.gpio_base < 16 { 0 } else { 16 };
    let base_addr_pin = layout.gpio_base - pio_base;

    OneromAlgAddrConfig::AlgAddr0 {
        clkdiv_int: DEFAULT_CLKDIV_INT,
        clkdiv_frac: DEFAULT_CLKDIV_FRAC,
        gpio_base: pio_base,
        num_delay_cycles,
        base_addr_pin,
        num_addr_pins: layout.num_addr_pins,
        num_rom_table_bits,
    }
}

/// Build `OneromAlgDmaConfig`.
///
/// G: always continuous for now (IRQ-based DMA later).
pub fn build_alg_dma(bit_mode: BitModes) -> OneromAlgDmaConfig {
    OneromAlgDmaConfig::AlgDma0 {
        bit_mode,
        continuous: 1,
    }
}

/// Compute the [`CombinedAlgPreference`] for an already-built
/// [`OneromAlgConfig`], using the `From` impls on the per-family preference
/// enums.
///
/// Useful for logging, post-derivation validation, and future joint
/// algorithm-selection passes where the combined preference of a candidate
/// layout needs to be compared against alternatives.
pub fn combined_alg_preference(alg: &OneromAlgConfig) -> CombinedAlgPreference {
    (
        CsAlgPreference::from(&alg.alg_cs),
        AddrAlgPreference::from(&alg.alg_addr),
        DataAlgPreference::from(&alg.alg_data),
        DmaAlgPreference::from(&alg.alg_dma),
    )
}

/// Assemble the full `OneromAlgConfig` for a chip set, from already-derived
/// layouts.
///
/// Builds each of `alg_addr`/`alg_data`/`alg_cs`/`alg_dma` from
/// `addr_layout`/`cs_data_layout`, and wires in any CS-polarity overrides
/// (`cs_overrides`), Banked X1/X2 address-pin inversion overrides
/// (`gpio_x_overrides`), and Banked X1/X2 pulls (`gpio_pull_config`).
///
/// `bit_mode`/`force_16_bit` determine `alg_data`/`alg_dma` (see
/// `build_alg_data`/`bit_mode_for`) - both computed once by the caller
/// (`build_rom_slot`), since `bit_mode` is also needed by
/// `derive_addr_layout`. `num_chips` is `chip_types.len()` (==
/// `chips.len()`) for the set.
///
/// `secondary_cs_configs` carries the CS configs for `chips[1..]` in a
/// Multi set (empty for Single/Banked). X1/X2 override decisions use the
/// respective secondary chip's `cs1_logic` rather than `chip[0]`'s.
#[allow(clippy::too_many_arguments)]
pub fn build_alg_config(
    ctx: &SlotContext,
    addr_layout: &AddrLayout,
    cs_data_layout: &CsDataLayout,
    secondary_cs_configs: &[CsConfig],
) -> OneromAlgConfig {
    let board = ctx.board;
    let set_type = ctx.set_type;
    let bit_mode = ctx.bit_mode;
    let force_16_bit = ctx.force_16_bit;
    let num_chips = ctx.chip_types.len();
    let cs_config = &ctx.cs_config;

    let alg_data = build_alg_data(cs_data_layout, board, bit_mode, force_16_bit);
    let alg_addr = build_alg_addr(addr_layout, &alg_data);
    let alg_cs = build_alg_cs(cs_data_layout, set_type, &alg_data);
    let alg_dma = build_alg_dma(bit_mode);

    let mut overrides =
        build_cs_overrides(cs_data_layout, set_type, cs_config, secondary_cs_configs);

    // AlgData1 provides 8-bit serving when /BYTE is read high by the PIO.
    // The RP2350 GPIO input level for /BYTE is high when the pin is not
    // asserted, so we need to invert it so the PIO sees 1 = byte mode
    // asserted.
    if let OneromAlgDataConfig::AlgData1 { byte_pin, .. } = &alg_data {
        let abs_gpio = byte_pin + cs_data_layout.gpio_base;
        overrides.push(encode_override(abs_gpio, GpioOverride::GpioOverInvert));
    }

    // Banked sets on boards where x_jumper_pull==0: X1/X2 address GPIOs
    // read 0 when the jumper is fitted and 1 when not — the opposite of the
    // expected convention (fitted = selected = 1 in the table index).
    // GpioOverInvert corrects this so bank 0 is always the "no jumper"
    // default regardless of board x_jumper_pull direction.
    overrides.extend(build_gpio_x_overrides(
        addr_layout,
        set_type,
        num_chips,
        board,
    ));

    // Force every GPIO inside the address-read window that serves no purpose
    // for this ROM type (gaps, padding, or emulated-NC socket pins) to read
    // 0, so a host driving such a pin can't perturb the ROM-table index.
    // Disjoint from the overrides above - those all target used GPIOs.
    overrides.extend(build_unused_addr_overrides(
        addr_layout,
        cs_data_layout,
        &alg_data,
    ));

    let gpio_override_config = if overrides.is_empty() {
        None
    } else {
        Some(OneromAlgOverrideConfig { params: overrides })
    };

    let gpio_pull_config = build_gpio_pull_config(addr_layout, set_type, num_chips, board);

    OneromAlgConfig {
        alg_cs,
        alg_addr,
        alg_data,
        alg_dma,
        gpio_pull_config,
        gpio_override_config,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::super::addr_layout::derive_addr_layout;
    use super::super::alg_preference::{
        AddrAlgPreference, CombinedAlgPreference, CsAlgPreference, DataAlgPreference,
        DmaAlgPreference,
    };
    use super::super::cs_data_layout::derive_cs_data_layout;
    use super::super::slot_context::SlotContext;
    use super::*;
    use crate::image::{ChipSetType, CsConfig, CsLogic};
    use onerom_metadata::OneromAlgCsConfig;

    #[test]
    fn fire24a_2364_single() {
        let addr_layout = AddrLayout {
            gpio_base: 0,
            num_addr_pins: 16,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![7, 6, 5, 4, 3, 2, 1, 0, 10, 11, 14, 15, 12],
            excess_addr_pin_gpios: alloc::vec![],
        };
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 16,
            num_data_pins: 8,
            data_pin_gpios: alloc::vec![16, 17, 18, 19, 20, 21, 22, 23],
            base_cs_pin: 13,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: alloc::vec![super::super::cs_data_layout::SelectLine {
                role: super::super::cs_data_layout::SelectRole::Cs1,
                gpio: 13,
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };

        let alg_data = build_alg_data(&cs_data_layout, Board::Fire24A, BitModes::BitMode8, false);
        assert_eq!(
            alg_data,
            OneromAlgDataConfig::AlgData0 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                base_data_pin: 16,
                word_size: 8,
            }
        );

        let alg_addr = build_alg_addr(&addr_layout, &alg_data);
        assert_eq!(
            alg_addr,
            OneromAlgAddrConfig::AlgAddr0 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                num_delay_cycles: 2,
                base_addr_pin: 0,
                num_addr_pins: 16,
                num_rom_table_bits: 16,
            }
        );

        let alg_dma = build_alg_dma(BitModes::BitMode8);
        assert_eq!(
            alg_dma,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: BitModes::BitMode8,
                continuous: 1,
            }
        );
    }

    /// End-to-end sentinel: Fire24A, single 2364, CS1 ActiveLow. 2364 has
    /// no `deselect_when_address_all_high`, so `alg_cs` must be `AlgCs0`.
    /// Single set → no CS/X-inversion override, but GPIO8 (X2) and GPIO9
    /// (X1) sit unused inside the [0,16) address window, so both are forced
    /// low → `gpio_override_config` carries two `GpioOverLow` entries.
    #[test]
    fn fire24a_2364_single_full_config() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let ctx = SlotContext {
            board: Board::Fire24A,
            set_type: ChipSetType::Single,
            chip_types: alloc::vec![ChipType::Chip2364],
            cs_config,
            bit_mode: bit_mode_for(ChipType::Chip2364, Board::Fire24A),
            pin_offset: 0,
            force_16_bit: false,
            multi_cs_config: None,
        };
        assert_eq!(ctx.bit_mode, BitModes::BitMode8);

        let addr_layout = derive_addr_layout(&ctx).expect("addr layout derivation should succeed");
        let cs_data_layout = derive_cs_data_layout(&ctx, Some(&addr_layout))
            .expect("cs/data layout derivation should succeed");

        let config = build_alg_config(&ctx, &addr_layout, &cs_data_layout, &[]);

        assert_eq!(
            config,
            OneromAlgConfig {
                alg_cs: OneromAlgCsConfig::AlgCs0 {
                    clkdiv_int: 1,
                    clkdiv_frac: 0,
                    gpio_base: 0,
                    base_cs_pin: 13,
                    num_cs_pins: 1,
                    base_data_pin: 16,
                    num_data_pins: 8,
                    cs_active_delay: 0,
                    cs_inactive_delay: 0,
                    serve_cs_low_0: 0,
                    byte_pin: GPIO_NONE,
                    first_rom_cs_base: 13,
                    first_rom_num_cs_pins: 1,
                },
                alg_addr: OneromAlgAddrConfig::AlgAddr0 {
                    clkdiv_int: 1,
                    clkdiv_frac: 0,
                    gpio_base: 0,
                    num_delay_cycles: 2,
                    base_addr_pin: 0,
                    num_addr_pins: 16,
                    num_rom_table_bits: 16,
                },
                alg_data: OneromAlgDataConfig::AlgData0 {
                    clkdiv_int: 1,
                    clkdiv_frac: 0,
                    gpio_base: 0,
                    base_data_pin: 16,
                    word_size: 8,
                },
                alg_dma: OneromAlgDmaConfig::AlgDma0 {
                    bit_mode: BitModes::BitMode8,
                    continuous: 1,
                },
                gpio_pull_config: None,
                gpio_override_config: Some(OneromAlgOverrideConfig {
                    params: alloc::vec![
                        encode_override(8, GpioOverride::GpioOverLow),
                        encode_override(9, GpioOverride::GpioOverLow),
                    ],
                }),
            }
        );
    }

    /// Fire24A, 2-chip Banked 2364, CS1 ActiveLow: verifies that
    /// `gpio_override_config` is populated with exactly one `GpioOverInvert`
    /// entry for X1. Fire24A has `x_jumper_pull()==0`, so fitting the X1
    /// jumper drives the GPIO low — the inversion corrects this so the
    /// address PIO sees 1 (bank 1 selected) when the jumper is fitted and
    /// 0 (bank 0, default) when it is not.
    #[test]
    fn fire24a_2364_banked_2chip_has_x_override() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let ctx = SlotContext {
            board: Board::Fire24A,
            set_type: ChipSetType::Banked,
            chip_types: alloc::vec![ChipType::Chip2364, ChipType::Chip2364],
            cs_config,
            bit_mode: bit_mode_for(ChipType::Chip2364, Board::Fire24A),
            pin_offset: 0,
            force_16_bit: false,
            multi_cs_config: None,
        };

        let addr_layout = derive_addr_layout(&ctx).expect("addr layout derivation should succeed");
        let cs_data_layout = derive_cs_data_layout(&ctx, Some(&addr_layout))
            .expect("cs/data layout derivation should succeed");

        let config = build_alg_config(&ctx, &addr_layout, &cs_data_layout, &[]);

        // CS1 active_low matches required active_low → no CS override.
        // AlgData0 (8-bit) → no byte_pin override.
        // Banked + x_jumper_pull=0 → GpioOverInvert for X1 (2-chip: X1 only).
        // GPIO8 (X2) is unused on a 2-chip set and sits in the [0,16)
        // address window → one GpioOverLow entry.
        let ov = config
            .gpio_override_config
            .expect("banked on x_jumper_pull=0 board must have gpio_override_config");
        assert_eq!(
            ov.params.len(),
            2,
            "2-chip banked: X1 invert + X2 unused-low"
        );

        let x1_gpio = addr_layout
            .x1_gpio
            .expect("banked addr_layout must have x1_gpio");

        // Entry 0: GpioOverInvert on X1.
        assert_eq!(ov.params[0] >> 6, 1, "entry 0 must be GpioOverInvert type");
        assert_eq!(
            ov.params[0] & 0x3F,
            x1_gpio,
            "override GPIO must match addr_layout.x1_gpio"
        );

        // Entry 1: GpioOverLow on GPIO8 (Fire24A X2, unused on a 2-chip set).
        assert_eq!(ov.params[1] >> 6, 2, "entry 1 must be GpioOverLow type");
        assert_eq!(ov.params[1] & 0x3F, 8, "unused-low GPIO must be X2 (GPIO8)");
    }

    /// End-to-end sentinel: Fire28A, single 23QL384, CS1 ActiveLow.
    ///
    /// 23QL384 has `deselect_when_address_all_high() = Some(&[14, 15])`,
    /// so `alg_cs` must be `AlgCs2` with `num_qualifier_pins=2` and
    /// `qualifier_inactive_pattern=0b11` (A14 and A15 both high =
    /// deselected). `alg_dma` must be `BitMode8` (23QL384 is 8-bit only).
    /// Exact GPIO values are board-specific and tested separately in
    /// `cs_data_layout::tests`.
    #[test]
    #[allow(clippy::wildcard_enum_match_arm)]
    fn fire28a_23ql384_single_full_config_alg_cs2() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let ctx = SlotContext {
            board: Board::Fire28A,
            set_type: ChipSetType::Single,
            chip_types: alloc::vec![ChipType::Chip23QL384],
            cs_config,
            bit_mode: bit_mode_for(ChipType::Chip23QL384, Board::Fire28A),
            pin_offset: 0,
            force_16_bit: false,
            multi_cs_config: None,
        };
        assert_eq!(ctx.bit_mode, BitModes::BitMode8);

        let addr_layout = derive_addr_layout(&ctx).expect("addr layout derivation should succeed");
        let cs_data_layout = derive_cs_data_layout(&ctx, Some(&addr_layout))
            .expect("cs/data layout derivation should succeed");

        let config = build_alg_config(&ctx, &addr_layout, &cs_data_layout, &[]);

        assert_eq!(
            config.alg_dma,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: BitModes::BitMode8,
                continuous: 1
            }
        );

        match &config.alg_cs {
            OneromAlgCsConfig::AlgCs2 {
                clkdiv_int: _,
                clkdiv_frac: _,
                gpio_base: _,
                base_cs_pin: _,
                num_cs_pins: _,
                base_data_pin: _,
                num_data_pins: _,
                cs_active_delay: _,
                cs_inactive_delay: _,
                base_qualifier_pin,
                num_qualifier_pins,
                qualifier_inactive_pattern,
            } => {
                assert_eq!(*num_qualifier_pins, 2);
                assert_eq!(*qualifier_inactive_pattern, 0b11);
                assert!(
                    *base_qualifier_pin < 32,
                    "base_qualifier_pin {base_qualifier_pin} out of PIO window"
                );
            }
            other => panic!("expected AlgCs2 for 23QL384, got {other:?}"),
        }
    }

    /// Fire40A, single 27C400, BitMode16, `force_16_bit=false` (default):
    /// `AlgData1` with `byte_pin`/`a_minus_1_pin` set, `num_delay_cycles=4`.
    #[test]
    fn fire40a_27c400_bitmode16_algdata1() {
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 16,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            base_cs_pin: 17,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: alloc::vec![super::super::cs_data_layout::SelectLine {
                role: super::super::cs_data_layout::SelectRole::Ce,
                gpio: 17,
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };
        let addr_layout = AddrLayout {
            gpio_base: 19,
            num_addr_pins: 18,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![
                36, 35, 34, 33, 32, 31, 30, 29, 27, 26, 25, 24, 23, 22, 21, 20, 19, 28
            ],
            excess_addr_pin_gpios: alloc::vec![],
        };

        let alg_data = build_alg_data(&cs_data_layout, Board::Fire40A, BitModes::BitMode16, false);
        assert_eq!(
            alg_data,
            OneromAlgDataConfig::AlgData1 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                base_data_pin: 0,
                word_size: 16,
                byte_pin: 18,
                a_minus_1_pin: 15,
            }
        );

        let alg_addr = build_alg_addr(&addr_layout, &alg_data);
        assert_eq!(
            alg_addr,
            OneromAlgAddrConfig::AlgAddr0 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 16,
                num_delay_cycles: 4,
                base_addr_pin: 3,
                num_addr_pins: 18,
                num_rom_table_bits: 19,
            }
        );

        let alg_dma = build_alg_dma(BitModes::BitMode16);
        assert_eq!(
            alg_dma,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: BitModes::BitMode16,
                continuous: 1,
            }
        );
    }

    /// Same Fire40A/27C400 layouts, `force_16_bit=true`: `AlgData0` with
    /// `word_size=16` (`/BYTE` ignored entirely), `num_delay_cycles=2`.
    #[test]
    fn fire40a_27c400_force_16bit_algdata0() {
        let cs_data_layout = CsDataLayout {
            gpio_base: 0,
            base_data_pin: 0,
            num_data_pins: 16,
            data_pin_gpios: alloc::vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
            base_cs_pin: 17,
            num_cs_pins: 1,
            cs_ignore_index: None,
            select_lines: alloc::vec![super::super::cs_data_layout::SelectLine {
                role: super::super::cs_data_layout::SelectRole::Ce,
                gpio: 17,
            }],
            commoned_lines: alloc::vec![],
            alg_cs2: None,
        };
        let addr_layout = AddrLayout {
            gpio_base: 19,
            num_addr_pins: 18,
            x1_gpio: None,
            x2_gpio: None,
            addr_pin_gpios: alloc::vec![
                36, 35, 34, 33, 32, 31, 30, 29, 27, 26, 25, 24, 23, 22, 21, 20, 19, 28
            ],
            excess_addr_pin_gpios: alloc::vec![],
        };

        let alg_data = build_alg_data(&cs_data_layout, Board::Fire40A, BitModes::BitMode16, true);
        assert_eq!(
            alg_data,
            OneromAlgDataConfig::AlgData0 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 0,
                base_data_pin: 0,
                word_size: 16,
            }
        );

        let alg_addr = build_alg_addr(&addr_layout, &alg_data);
        assert_eq!(
            alg_addr,
            OneromAlgAddrConfig::AlgAddr0 {
                clkdiv_int: 1,
                clkdiv_frac: 0,
                gpio_base: 16,
                num_delay_cycles: 2,
                base_addr_pin: 3,
                num_addr_pins: 18,
                num_rom_table_bits: 19,
            }
        );

        let alg_dma = build_alg_dma(BitModes::BitMode16);
        assert_eq!(
            alg_dma,
            OneromAlgDmaConfig::AlgDma0 {
                bit_mode: BitModes::BitMode16,
                continuous: 1,
            }
        );
    }

    /// `combined_alg_preference` maps a built `OneromAlgConfig` back to its
    /// `CombinedAlgPreference` tuple via the `From` impls. For Fire24A/2364
    /// (8-bit, contiguous CS, single addr algorithm) the expected result is
    /// the simplest possible combination.
    #[test]
    fn combined_alg_preference_fire24a_2364() {
        let cs_config = CsConfig::new(Some(CsLogic::ActiveLow), None, None, None);

        let ctx = SlotContext {
            board: Board::Fire24A,
            set_type: ChipSetType::Single,
            chip_types: alloc::vec![ChipType::Chip2364],
            cs_config,
            bit_mode: bit_mode_for(ChipType::Chip2364, Board::Fire24A),
            pin_offset: 0,
            force_16_bit: false,
            multi_cs_config: None,
        };

        let addr_layout = derive_addr_layout(&ctx).expect("addr layout should succeed");
        let cs_data_layout =
            derive_cs_data_layout(&ctx, Some(&addr_layout)).expect("cs layout should succeed");
        let config = build_alg_config(&ctx, &addr_layout, &cs_data_layout, &[]);

        let pref: CombinedAlgPreference = combined_alg_preference(&config);
        assert_eq!(
            pref,
            (
                CsAlgPreference::AlgCs0,
                AddrAlgPreference::AlgAddr0,
                DataAlgPreference::AlgData0,
                DmaAlgPreference::AlgDma0,
            )
        );
    }
}
