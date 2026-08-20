// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! One ROM picoboot extensions
//!
//! This module is the host-side mirror of
//! `plugins/system/usb/include/usb_custom_pbx.h`. It owns the wire layout of
//! every One ROM custom picoboot command and nothing else - there is no device
//! I/O here.
//!
//! Keeping the layout here, rather than hand-packing byte offsets at each call
//! site, means the byte-level tests at the bottom of this file are the one
//! place that can catch the C and the Rust drifting apart. Those tests assert
//! literal byte arrays rather than round-tripping through the encoders, so an
//! encoder that is wrong in the same way as its decoder still fails them.
//!
//! ## Reserved fields
//!
//! Every reserved field is zeroed on send and ignored on receive. Because a
//! reserved field is ignored rather than rejected, a stale host's garbage is
//! indistinguishable from a new host's deliberate value, so neither side can
//! infer intent from one. Any future use of reserved space must be gated by a
//! [`ONEROM_FEAT_GPIO_SET`]-style capability bit, never by sniffing the field.

/// picoboot magic identifying a One ROM extension command, `"ONER"`.
pub const ONEROM_MAGIC: u32 =
    b'O' as u32 | (b'N' as u32) << 8 | (b'E' as u32) << 16 | (b'R' as u32) << 24;

/// Direction bit in a picoboot `cmd_id`: set means the host reads data back.
///
/// Mirrors picobootx's `PICOBOOT_DIR_IN`. [`ONEROM_CMD_GET_CAPS`] and
/// [`ONEROM_CMD_GPIO_QUERY`] must be sent with this bit set; the commands with
/// no data phase are sent with it clear.
pub const PICOBOOT_DIR_IN: u8 = 0x80;

/// Set the status LED. No data phase.
pub const ONEROM_CMD_SET_LED: u8 = 0x01;

/// Read what this device's picobootx extension supports. Data IN.
pub const ONEROM_CMD_GET_CAPS: u8 = 0x02;

/// Drive a GPIO, optionally for a bounded period. No data phase.
pub const ONEROM_CMD_GPIO_SET: u8 = 0x03;

/// Read what One ROM is using a run of GPIOs for. Data IN.
pub const ONEROM_CMD_GPIO_QUERY: u8 = 0x04;

/// Read what one LED is doing. Data IN, so send it with [`PICOBOOT_DIR_IN`].
///
/// No capability bit gates this. A plugin that predates the command refuses it
/// outright, unlike [`ONEROM_CMD_SET_LED`]'s extended arguments, which an older
/// plugin accepts and ignores.
pub const ONEROM_CMD_LED_QUERY: u8 = 0x05;

/// Bytes of argument every picoboot command carries inline.
pub const ONEROM_CMD_ARGS_LEN: usize = 16;

/// `transfer_len` the host requests for [`ONEROM_CMD_GET_CAPS`].
///
/// The host always asks for exactly this many bytes and the device zero-fills
/// all of them. [`Caps::struct_len`] says how many are meaningful, which is
/// what lets the structure grow without a protocol change - so a host must
/// accept any `struct_len` and must never require it to equal this.
pub const ONEROM_CAPS_LEN: u32 = 32;

/// `transfer_len` the host requests for [`ONEROM_CMD_LED_QUERY`].
pub const ONEROM_LED_STATE_LEN: u32 = 16;

/// Bytes per [`GpioEntry`] in an [`ONEROM_CMD_GPIO_QUERY`] response.
pub const ONEROM_GPIO_ENTRY_LEN: usize = 4;

/// [`ONEROM_CMD_GPIO_SET`] is available.
pub const ONEROM_FEAT_GPIO_SET: u32 = 1 << 0;

/// [`ONEROM_CMD_GPIO_QUERY`] is available.
pub const ONEROM_FEAT_GPIO_QUERY: u32 = 1 << 1;

/// [`GpioSetArgs::duration_ms`] and [`GpioSetArgs::after_state`] are honoured.
pub const ONEROM_FEAT_GPIO_HOLD: u32 = 1 << 2;

/// Every [`ONEROM_CMD_SET_LED`] argument after `sub_cmd` is honoured: the
/// colour, the brightness, the period and the hold.
///
/// A plugin that predates them reads `led_id` and `sub_cmd` and ignores the
/// rest, so a host that sends them regardless is told the command succeeded
/// and gets none of what it asked for. Check this before sending any of them,
/// on either LED.
///
/// The bit says the arguments reach the firmware's LED engine. It does not say
/// this board has an RGB LED - one without refuses [`LedId::Rgb`], which is a
/// different answer from a device that cannot be asked at all.
pub const ONEROM_FEAT_LED_ARGS: u32 = 1 << 3;

/// Drive the GPIO even though One ROM is using it.
///
/// Mirrors the firmware's `ORA_GPIO_FLAG_FORCE`.
pub const ONEROM_GPIO_FLAG_FORCE: u8 = 1 << 0;

/// A wire response that could not be decoded.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("Capabilities response is too short to decode: {0} bytes")]
    CapsTooShort(usize),

    /// An LED query response too short to carry its own `struct_len`.
    #[error("the device's LED state response was {0} bytes, too short to decode")]
    LedStateTooShort(usize),

    #[error("GPIO query response is {0} bytes, not a whole number of 4-byte entries")]
    GpioEntriesMisaligned(usize),
}

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum LedSubCmd {
    Off = 0x00,
    On = 0x01,
    Beacon = 0x02,
    Flame = 0x03,

    /// Rotates through the hues. RGB LED only.
    Cycle = 0x04,

    /// Fades the colour up and down. RGB LED only.
    Breathe = 0x05,

    /// Alternates the LED with dark, at its colour where it has one.
    Blink = 0x06,
}

/// Which LED a [`ONEROM_CMD_SET_LED`] addresses.
///
/// A channel number rather than a fixed set, so a One ROM that gains further
/// LEDs numbers them from 2 upwards. A device refuses a channel it does not
/// have rather than driving another one.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedId {
    /// The discrete status LED.
    Status = 0,

    /// The RGB LED. Requires [`ONEROM_FEAT_LED_ARGS`], and a board that has one.
    Rgb = 1,
}

/// Arguments to [`ONEROM_CMD_SET_LED`], laid out as `onerom_set_led_args_t` in
/// the plugin's `usb_custom_pbx.h`.
///
/// Every field after `sub_cmd` reads as "the device chooses" when zero, which
/// is what lets a host that predates them send zeros and get the behaviour it
/// always got.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SetLedArgs {
    /// Which LED.
    pub led_id: LedId,

    /// What to do with it.
    pub sub_cmd: LedSubCmd,

    /// Red, for the LEDs and modes that take a colour.
    pub red: u8,

    /// Green, for the LEDs and modes that take a colour.
    pub green: u8,

    /// Blue, for the LEDs and modes that take a colour.
    pub blue: u8,

    /// Brightness as a percentage, 1 to 100. Zero is the device's default.
    pub brightness: u8,

    /// How long one repetition of the mode takes, in milliseconds. Zero is the
    /// mode's own default.
    pub period_ms: u16,

    /// How long to stay in this mode before returning to the one before it, in
    /// milliseconds. Zero holds it until something changes it.
    pub hold_ms: u32,
}

impl SetLedArgs {
    /// A request for the status LED, which takes no colour.
    pub fn status(sub_cmd: LedSubCmd) -> Self {
        Self {
            led_id: LedId::Status,
            sub_cmd,
            red: 0,
            green: 0,
            blue: 0,
            brightness: 0,
            period_ms: 0,
            hold_ms: 0,
        }
    }

    /// A request for the RGB LED at a colour, with everything else left to the
    /// device.
    ///
    /// An all-zero colour is read by the device as red rather than leaving the
    /// LED dark, so a caller with no colour in mind still gets light.
    pub fn rgb(sub_cmd: LedSubCmd, red: u8, green: u8, blue: u8) -> Self {
        Self {
            led_id: LedId::Rgb,
            sub_cmd,
            red,
            green,
            blue,
            brightness: 0,
            period_ms: 0,
            hold_ms: 0,
        }
    }

    /// Whether this request needs [`ONEROM_FEAT_LED_ARGS`] to mean anything.
    ///
    /// The colour and brightness are not counted. Those only ever accompany
    /// the RGB LED, whose own channel byte an old plugin ignores too, so
    /// [`ONEROM_FEAT_LED_ARGS`] is already checked before one is sent. A period
    /// or a hold is the case a plain status LED request can carry.
    pub fn needs_led_args(&self) -> bool {
        self.period_ms != 0 || self.hold_ms != 0
    }

    /// Pack into the 16 inline argument bytes of a picoboot command.
    pub fn encode(&self) -> [u8; ONEROM_CMD_ARGS_LEN] {
        let mut args = [0u8; ONEROM_CMD_ARGS_LEN];
        args[0] = self.led_id as u8;
        args[1] = self.sub_cmd as u8;
        // args[2..4] are reserved and stay zero.
        args[4] = self.red;
        args[5] = self.green;
        args[6] = self.blue;
        args[7] = self.brightness;
        args[8..10].copy_from_slice(&self.period_ms.to_le_bytes());
        args[10..14].copy_from_slice(&self.hold_ms.to_le_bytes());
        // args[14..16] are reserved and stay zero.
        args
    }
}

/// Arguments to [`ONEROM_CMD_LED_QUERY`], laid out as `onerom_led_query_args_t`
/// in the plugin's `usb_custom_pbx.h`.
#[derive(Debug, Clone, Copy)]
pub struct LedQueryArgs {
    /// Which LED to describe.
    pub led_id: LedId,
}

impl LedQueryArgs {
    /// Pack into the 16 inline argument bytes of a picoboot command.
    pub fn encode(&self) -> [u8; ONEROM_CMD_ARGS_LEN] {
        let mut args = [0u8; ONEROM_CMD_ARGS_LEN];
        args[0] = self.led_id as u8;
        // args[1..16] are reserved and stay zero.
        args
    }
}

/// What one LED is doing, from an [`ONEROM_CMD_LED_QUERY`] response.
///
/// Mirrors `onerom_led_state_t` in the plugin's `usb_custom_pbx.h`, which in
/// turn mirrors the firmware's own `ora_led_state_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LedState {
    /// Bytes of the response the device says are meaningful. Never assume this
    /// equals [`ONEROM_LED_STATE_LEN`].
    pub struct_len: u16,

    /// Which LED this describes, echoing what was asked for.
    pub led_id: u8,

    /// Whether this board has this LED. A board without one still answers,
    /// which is what tells a host "this board has no RGB LED" apart from "this
    /// device cannot be asked".
    pub present: bool,

    /// What it is doing now, numbered as [`LedSubCmd`].
    pub mode: u8,

    /// Brightness as a percentage. Zero on an LED that has no brightness.
    pub brightness: u8,

    /// Red of the colour in force.
    pub red: u8,
    /// Green of the colour in force.
    pub green: u8,
    /// Blue of the colour in force.
    pub blue: u8,

    /// The GPIO this LED is on. Meaningless when `present` is false.
    pub gpio: u8,

    /// How long one repetition of the mode takes, in milliseconds.
    pub period_ms: u16,
}

impl LedState {
    /// Decode an LED query response, on the same terms as [`Caps::decode`].
    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        if buf.len() < 2 {
            return Err(DecodeError::LedStateTooShort(buf.len()));
        }

        let struct_len = u16::from_le_bytes([buf[0], buf[1]]);
        let usable = (struct_len as usize).min(buf.len());
        let u8_at = |off: usize| -> u8 { if off < usable { buf[off] } else { 0 } };
        let u16_at = |off: usize| -> u16 {
            if off + 2 <= usable {
                u16::from_le_bytes([buf[off], buf[off + 1]])
            } else {
                0
            }
        };

        Ok(Self {
            struct_len,
            led_id: u8_at(2),
            present: u8_at(3) != 0,
            mode: u8_at(4),
            brightness: u8_at(5),
            red: u8_at(6),
            green: u8_at(7),
            blue: u8_at(8),
            gpio: u8_at(9),
            // Byte 10 is reserved0, bytes 13..16 reserved1.
            period_ms: u16_at(11),
        })
    }

    /// The mode as the word the CLI uses for it, or `None` where this host does
    /// not know the mode the device reported.
    pub fn mode_name(&self) -> Option<&'static str> {
        Some(match self.mode {
            0 => "off",
            1 => "on",
            2 => "beacon",
            3 => "flame",
            4 => "cycle",
            5 => "breathe",
            6 => "blink",
            _ => return None,
        })
    }
}

/// State a GPIO can be placed in. Mirrors the firmware's `ora_gpio_state_t`
/// value for value.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioState {
    /// Drive low.
    Low = 0,

    /// Drive high.
    High = 1,

    /// Release - output driver off, high impedance. The CLI spells this `z`.
    Input = 2,
}

/// What One ROM itself is using a GPIO for. Mirrors the firmware's
/// `ora_gpio_use_t` value for value.
///
/// This describes only what the firmware has claimed the pin for. It says
/// nothing about what is wired to the pad.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioUse {
    /// Not used by One ROM.
    Free = 0,

    /// Serving reads this GPIO. Driving it is reversible - set it back to
    /// [`GpioState::Input`] and serving reads the pin as before.
    ServingRead = 1,

    /// Serving drives this GPIO. Driving it breaks serving until the device
    /// reboots.
    ServingDriven = 2,

    /// A board system pin - status LED, neopixel, VBUS or external flash CS.
    SystemPin = 3,
}

impl GpioUse {
    /// Decode a wire `use` byte, or `None` if the device reported a value this
    /// build does not know.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Free),
            1 => Some(Self::ServingRead),
            2 => Some(Self::ServingDriven),
            3 => Some(Self::SystemPin),
            _ => None,
        }
    }
}

/// Arguments to [`ONEROM_CMD_GPIO_SET`].
#[derive(Debug, Clone, Copy)]
pub struct GpioSetArgs {
    /// GPIO to drive. Valid range is `0..num_gpios` from [`Caps`]; the device
    /// rejects anything else.
    pub gpio: u8,

    /// State to apply immediately.
    pub state: GpioState,

    /// State to revert to once `duration_ms` expires. Unused when
    /// `duration_ms` is 0.
    pub after_state: GpioState,

    /// [`ONEROM_GPIO_FLAG_FORCE`] and friends. Kept as the raw wire byte so
    /// later flags need no change here.
    pub flags: u8,

    /// How long to hold `state`, in milliseconds. 0 latches indefinitely.
    pub duration_ms: u32,
}

impl GpioSetArgs {
    /// Pack into the 16 inline argument bytes of a picoboot command.
    pub fn encode(&self) -> [u8; ONEROM_CMD_ARGS_LEN] {
        let mut args = [0u8; ONEROM_CMD_ARGS_LEN];
        args[0] = self.gpio;
        args[1] = self.state as u8;
        args[2] = self.after_state as u8;
        args[3] = self.flags;
        args[4..8].copy_from_slice(&self.duration_ms.to_le_bytes());
        // args[8..16] are reserved0/reserved1 and stay zero.
        args
    }
}

/// Arguments to [`ONEROM_CMD_GPIO_QUERY`].
#[derive(Debug, Clone, Copy)]
pub struct GpioQueryArgs {
    /// First GPIO in the run.
    pub first_gpio: u8,

    /// GPIOs to report. The device rejects `first_gpio + count > num_gpios`.
    pub count: u8,
}

impl GpioQueryArgs {
    /// Pack into the 16 inline argument bytes of a picoboot command.
    pub fn encode(&self) -> [u8; ONEROM_CMD_ARGS_LEN] {
        let mut args = [0u8; ONEROM_CMD_ARGS_LEN];
        args[0] = self.first_gpio;
        args[1] = self.count;
        // args[2..16] are reserved and stay zero.
        args
    }

    /// `transfer_len` for the command's IN data phase.
    ///
    /// picoboot's transfer-length convention is a multiple of 4, at most 256
    /// bytes. An entry is 4 bytes, so a whole device fits in one command on
    /// either variant - 192 bytes for an RP2350B's 48 GPIOs, 120 for an
    /// RP2350A's 30. That
    /// headroom is the budget for any future growth of [`GpioEntry`]: at 5
    /// bytes per entry the multiple-of-4 rule breaks, and at 6 the RP2350B no
    /// longer fits in one command.
    pub fn transfer_len(&self) -> u32 {
        self.count as u32 * ONEROM_GPIO_ENTRY_LEN as u32
    }
}

/// One GPIO's worth of an [`ONEROM_CMD_GPIO_QUERY`] response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpioEntry {
    /// Raw `use` byte. Kept raw so a value from a newer device survives the
    /// decode; interpret it with [`GpioEntry::gpio_use`].
    pub gpio_use_raw: u8,

    /// Level currently on the pad, 0 or 1.
    pub level: u8,

    /// 1 if the pin's output driver is enabled, 0 if not.
    pub is_output: u8,
}

impl GpioEntry {
    /// Decode a whole response - a run of 4-byte entries.
    pub fn decode_all(buf: &[u8]) -> Result<Vec<Self>, DecodeError> {
        if !buf.len().is_multiple_of(ONEROM_GPIO_ENTRY_LEN) {
            return Err(DecodeError::GpioEntriesMisaligned(buf.len()));
        }

        Ok(buf
            .as_chunks::<ONEROM_GPIO_ENTRY_LEN>()
            .0
            .iter()
            .map(|e| Self {
                gpio_use_raw: e[0],
                level: e[1],
                is_output: e[2],
                // e[3] is reserved and ignored.
            })
            .collect())
    }

    /// What One ROM is using this GPIO for, or `None` if the device reported a
    /// use this build does not know.
    pub fn gpio_use(&self) -> Option<GpioUse> {
        GpioUse::from_u8(self.gpio_use_raw)
    }
}

/// Response to [`ONEROM_CMD_GET_CAPS`] - what this device's picobootx
/// extension supports.
#[derive(Debug, Clone, Copy, Default)]
pub struct Caps {
    /// Bytes of the response the device says are meaningful. Recorded as
    /// received, so it may exceed the bytes actually transferred if the device
    /// is newer than this host.
    pub struct_len: u16,

    /// One ROM picobootx extension version, independent of the plugin's own.
    pub ext_major: u8,

    /// See [`Caps::ext_major`].
    pub ext_minor: u8,

    /// `ONEROM_FEAT_*` bitmap. Test it with [`Caps::has_feature`].
    pub features: u32,

    /// GPIOs this device has: 30 on an RP2350A, 48 on an RP2350B. Never assume
    /// a value - size a [`GpioQueryArgs`] run from this.
    pub num_gpios: u8,

    /// Longest bounded hold [`ONEROM_CMD_GPIO_SET`] will accept, in
    /// milliseconds.
    pub max_hold_ms: u32,
}

impl Caps {
    /// Decode a capabilities response.
    ///
    /// The device zero-fills the whole response, and any field not wholly
    /// covered by `struct_len` - or not actually transferred - decodes as
    /// zero. A `struct_len` larger than the response is therefore fine, which
    /// is how a device newer than this host stays readable: the extra fields
    /// are simply not decoded. Reserved bytes are ignored, not validated.
    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        // Two bytes is the least that can be decoded, since struct_len itself
        // governs everything after it.
        if buf.len() < 2 {
            return Err(DecodeError::CapsTooShort(buf.len()));
        }

        let struct_len = u16::from_le_bytes([buf[0], buf[1]]);

        // Bytes that are both meaningful and present.
        let usable = (struct_len as usize).min(buf.len());
        let u8_at = |off: usize| -> u8 { if off < usable { buf[off] } else { 0 } };
        let u32_at = |off: usize| -> u32 {
            if off + 4 <= usable {
                u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
            } else {
                0
            }
        };

        Ok(Self {
            struct_len,
            ext_major: u8_at(2),
            ext_minor: u8_at(3),
            features: u32_at(4),
            num_gpios: u8_at(8),
            // Bytes 9..12 are reserved0.
            max_hold_ms: u32_at(12),
            // Bytes 16..32 are reserved1.
        })
    }

    /// Test one of the `ONEROM_FEAT_*` bits.
    pub fn has_feature(&self, feature: u32) -> bool {
        self.features & feature != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests assert literal bytes against
    // plugins/system/usb/include/usb_custom_pbx.h. Nothing else compares the
    // two definitions, so do not weaken them into round-trips through the
    // encoders in this file - those would pass even if both sides were wrong
    // in the same way.

    #[test]
    fn command_ids_match_the_header() {
        assert_eq!(ONEROM_MAGIC, 0x5245_4E4F); // "ONER", little-endian
        assert_eq!(ONEROM_CMD_SET_LED, 0x01);
        assert_eq!(ONEROM_CMD_GET_CAPS, 0x02);
        assert_eq!(ONEROM_CMD_GPIO_SET, 0x03);
        assert_eq!(ONEROM_CMD_GPIO_QUERY, 0x04);

        // The two IN commands travel with the direction bit set.
        assert_eq!(ONEROM_CMD_GET_CAPS | PICOBOOT_DIR_IN, 0x82);
        assert_eq!(ONEROM_CMD_GPIO_QUERY | PICOBOOT_DIR_IN, 0x84);
    }

    #[test]
    fn enum_discriminants_match_the_firmware() {
        assert_eq!(GpioState::Low as u8, 0);
        assert_eq!(GpioState::High as u8, 1);
        assert_eq!(GpioState::Input as u8, 2);

        assert_eq!(GpioUse::from_u8(0), Some(GpioUse::Free));
        assert_eq!(GpioUse::from_u8(1), Some(GpioUse::ServingRead));
        assert_eq!(GpioUse::from_u8(2), Some(GpioUse::ServingDriven));
        assert_eq!(GpioUse::from_u8(3), Some(GpioUse::SystemPin));
        assert_eq!(GpioUse::from_u8(4), None);
        assert_eq!(GpioUse::from_u8(0xFF), None);

        assert_eq!(ONEROM_GPIO_FLAG_FORCE, 0x01);
        assert_eq!(ONEROM_FEAT_GPIO_SET, 0x0000_0001);
        assert_eq!(ONEROM_FEAT_GPIO_QUERY, 0x0000_0002);
        assert_eq!(ONEROM_FEAT_GPIO_HOLD, 0x0000_0004);
        assert_eq!(ONEROM_FEAT_LED_ARGS, 0x0000_0008);

        assert_eq!(LedId::Status as u8, 0);
        assert_eq!(LedId::Rgb as u8, 1);

        assert_eq!(LedSubCmd::Off as u8, 0);
        assert_eq!(LedSubCmd::On as u8, 1);
        assert_eq!(LedSubCmd::Beacon as u8, 2);
        assert_eq!(LedSubCmd::Flame as u8, 3);
        assert_eq!(LedSubCmd::Cycle as u8, 4);
        assert_eq!(LedSubCmd::Breathe as u8, 5);
        assert_eq!(LedSubCmd::Blink as u8, 6);
    }

    #[test]
    fn led_query_args_name_the_channel_and_nothing_else() {
        assert_eq!(
            LedQueryArgs {
                led_id: LedId::Status
            }
            .encode(),
            [0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        assert_eq!(
            LedQueryArgs { led_id: LedId::Rgb }.encode(),
            [0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
    }

    #[test]
    fn led_state_decodes_the_layout_the_header_says() {
        // A full response: every field in its own byte, so a field that moves
        // fails here rather than on a device.
        let buf = [
            16, 0,    // struct_len
            0x01, // led_id
            1,    // present
            0x05, // mode: breathe
            40,   // brightness
            0x12, 0x34, 0x56, // colour
            29,   // gpio
            0xAA, // reserved0, set to catch a host reading it
            0x02, 0x01, // period_ms, little-endian
            0xAA, 0xAA, 0xAA, // reserved1, likewise
        ];
        let state = LedState::decode(&buf).expect("a full response decodes");
        assert_eq!(state.struct_len, 16);
        assert_eq!(state.led_id, 1);
        assert!(state.present);
        assert_eq!(state.mode, 5);
        assert_eq!(state.mode_name(), Some("breathe"));
        assert_eq!(state.brightness, 40);
        assert_eq!((state.red, state.green, state.blue), (0x12, 0x34, 0x56));
        assert_eq!(state.gpio, 29);
        // The reserved bytes carry 0xAA, so a field decoded from one of them
        // would show it rather than the value its own bytes hold.
        assert_eq!(state.period_ms, 0x0102);
    }

    #[test]
    fn a_led_state_shorter_than_this_host_expects_still_decodes() {
        // What a device older than this host sends: struct_len says only the
        // first six bytes mean anything, and everything past it reads as zero
        // rather than as whatever happened to be transferred.
        let buf = [
            6, 0,    // struct_len
            0x00, // led_id
            1,    // present
            0x02, // mode: beacon
            0,    // brightness
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        ];
        let state = LedState::decode(&buf).expect("a short response decodes");
        assert_eq!(state.mode_name(), Some("beacon"));
        assert_eq!((state.red, state.green, state.blue), (0, 0, 0));
        assert_eq!(state.gpio, 0);
        assert_eq!(state.period_ms, 0);
    }

    #[test]
    fn a_led_state_too_short_to_carry_its_length_is_refused() {
        assert!(matches!(
            LedState::decode(&[16]),
            Err(DecodeError::LedStateTooShort(1))
        ));
    }

    #[test]
    fn a_mode_this_host_does_not_know_has_no_name() {
        // A device newer than this CLI can be in a mode it has no word for.
        // Answering None is what lets the caller say so rather than guess.
        let buf = [16, 0, 0x01, 1, 0x63, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let state = LedState::decode(&buf).expect("decodes");
        assert_eq!(state.mode_name(), None);
    }

    #[test]
    fn only_a_period_or_a_hold_needs_the_capability() {
        // What every plugin has always understood: a mode and nothing else.
        // Checking capabilities for one of these would cost an exchange with
        // the device to learn nothing.
        assert!(!SetLedArgs::status(LedSubCmd::Beacon).needs_led_args());
        assert!(!SetLedArgs::status(LedSubCmd::Off).needs_led_args());

        // Either one on its own is enough, since an old plugin honours
        // neither.
        let mut held = SetLedArgs::status(LedSubCmd::On);
        held.hold_ms = 5_000;
        assert!(held.needs_led_args());

        let mut timed = SetLedArgs::status(LedSubCmd::Flame);
        timed.period_ms = 1_200;
        assert!(timed.needs_led_args());

        // A status request carrying a period and a hold puts them where the
        // RGB one does - the wire does not care which LED is addressed.
        let mut both = SetLedArgs::status(LedSubCmd::Beacon);
        both.period_ms = 0x0102;
        both.hold_ms = 0x0A0B_0C0D;
        assert_eq!(
            both.encode(),
            [
                0x00, 0x02, 0, 0, // status channel, beacon, reserved
                0, 0, 0, // no colour
                0, // no brightness
                0x02, 0x01, // period_ms, little-endian
                0x0D, 0x0C, 0x0B, 0x0A, // hold_ms, little-endian
                0, 0, // reserved
            ],
        );
    }

    #[test]
    fn set_led_args_are_laid_out_as_the_header_says() {
        // A status LED request is what a host that predates the RGB LED sends:
        // the channel, the mode, and fourteen zero bytes.
        assert_eq!(
            SetLedArgs::status(LedSubCmd::Beacon).encode(),
            [0x00, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // Every field of an RGB request, each in its own byte, so a field that
        // moves fails here rather than on a device.
        let mut args = SetLedArgs::rgb(LedSubCmd::On, 0x12, 0x34, 0x56);
        args.brightness = 40;
        args.period_ms = 0x0102;
        args.hold_ms = 0x0A0B_0C0D;
        assert_eq!(
            args.encode(),
            [
                0x01, 0x01, 0, 0, // channel, mode, reserved
                0x12, 0x34, 0x56, // colour
                40,   // brightness
                0x02, 0x01, // period_ms, little-endian
                0x0D, 0x0C, 0x0B, 0x0A, // hold_ms, little-endian
                0, 0, // reserved
            ],
        );
    }

    #[test]
    fn gpio_set_args_encode_to_the_header_layout() {
        let args = GpioSetArgs {
            gpio: 23,
            state: GpioState::Low,
            after_state: GpioState::Input,
            flags: ONEROM_GPIO_FLAG_FORCE,
            duration_ms: 100,
        };

        assert_eq!(
            args.encode(),
            [
                23, // gpio
                0,  // state = LOW
                2,  // after_state = INPUT
                1,  // flags = FORCE
                100, 0, 0, 0, // duration_ms, little-endian
                0, 0, 0, 0, // reserved0
                0, 0, 0, 0, // reserved1
            ]
        );
    }

    #[test]
    fn gpio_set_args_duration_is_little_endian() {
        let args = GpioSetArgs {
            gpio: 0,
            state: GpioState::High,
            after_state: GpioState::Low,
            flags: 0,
            duration_ms: 0x0102_0304,
        };

        assert_eq!(
            args.encode(),
            [0, 1, 0, 0, 0x04, 0x03, 0x02, 0x01, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn gpio_query_args_encode_to_the_header_layout() {
        let args = GpioQueryArgs {
            first_gpio: 4,
            count: 30,
        };

        assert_eq!(
            args.encode(),
            [4, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn gpio_query_transfer_len_fits_picoboot_limits() {
        // A whole device must fit in one command on either RP2350 variant.
        for count in [1u8, 30, 48] {
            let len = GpioQueryArgs {
                first_gpio: 0,
                count,
            }
            .transfer_len();
            assert_eq!(len, count as u32 * 4);
            assert_eq!(len % 4, 0, "picoboot expects a multiple of 4");
            assert!(len <= 256, "picoboot expects at most 256 bytes");
        }
        assert_eq!(
            GpioQueryArgs {
                first_gpio: 0,
                count: 48
            }
            .transfer_len(),
            192
        );
    }

    #[test]
    fn gpio_entries_decode_from_the_header_layout() {
        // Three entries; byte 3 of each is reserved and must be ignored, so it
        // is deliberately non-zero here.
        let buf = [
            0, 1, 0, 0xAA, // free, high, input
            2, 0, 1, 0xBB, // serving-driven, low, output
            9, 1, 1, 0xCC, // an unknown use from a newer device
        ];

        let entries = GpioEntry::decode_all(&buf).expect("decodes");
        assert_eq!(entries.len(), 3);

        assert_eq!(
            entries[0],
            GpioEntry {
                gpio_use_raw: 0,
                level: 1,
                is_output: 0
            }
        );
        assert_eq!(entries[0].gpio_use(), Some(GpioUse::Free));

        assert_eq!(
            entries[1],
            GpioEntry {
                gpio_use_raw: 2,
                level: 0,
                is_output: 1
            }
        );
        assert_eq!(entries[1].gpio_use(), Some(GpioUse::ServingDriven));

        assert_eq!(entries[2].gpio_use_raw, 9);
        assert_eq!(entries[2].gpio_use(), None);
    }

    #[test]
    fn gpio_entries_reject_a_partial_entry() {
        assert!(matches!(
            GpioEntry::decode_all(&[0, 0, 0, 0, 1]),
            Err(DecodeError::GpioEntriesMisaligned(5))
        ));
        assert_eq!(GpioEntry::decode_all(&[]).expect("empty is legal").len(), 0);
    }

    /// A full 32-byte response, laid out by hand against the C struct.
    fn caps_bytes() -> [u8; 32] {
        [
            0x20, 0x00, // struct_len = 32
            0x01, 0x02, // ext_major = 1, ext_minor = 2
            0x07, 0x00, 0x00, 0x00, // features = SET | QUERY | HOLD
            48,   // num_gpios (RP2350B)
            0xAA, 0xAA, 0xAA, // reserved0, deliberately non-zero
            0xE8, 0x03, 0x00, 0x00, // max_hold_ms = 1000
            // reserved1, deliberately non-zero
            0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB, 0xBB,
            0xBB, 0xBB,
        ]
    }

    #[test]
    fn caps_decode_from_the_header_layout() {
        let caps = Caps::decode(&caps_bytes()).expect("decodes");

        assert_eq!(caps.struct_len, 32);
        assert_eq!(caps.ext_major, 1);
        assert_eq!(caps.ext_minor, 2);
        assert_eq!(caps.features, 0x0000_0007);
        assert_eq!(caps.num_gpios, 48);
        assert_eq!(caps.max_hold_ms, 1000);

        assert!(caps.has_feature(ONEROM_FEAT_GPIO_SET));
        assert!(caps.has_feature(ONEROM_FEAT_GPIO_QUERY));
        assert!(caps.has_feature(ONEROM_FEAT_GPIO_HOLD));
        assert!(!caps.has_feature(1 << 3));
    }

    #[test]
    fn caps_decode_honours_num_gpios_of_thirty() {
        // Nothing on either side may assume 48.
        let mut buf = caps_bytes();
        buf[8] = 30;
        assert_eq!(Caps::decode(&buf).expect("decodes").num_gpios, 30);
    }

    #[test]
    fn caps_decode_zeroes_fields_beyond_struct_len() {
        // A device reporting only as far as num_gpios: 9 meaningful bytes.
        let mut buf = caps_bytes();
        buf[0] = 9;

        let caps = Caps::decode(&buf).expect("decodes");
        assert_eq!(caps.struct_len, 9);
        assert_eq!(caps.ext_major, 1);
        assert_eq!(caps.features, 0x0000_0007);
        assert_eq!(caps.num_gpios, 48);
        // Past struct_len, so not meaningful even though bytes were sent.
        assert_eq!(caps.max_hold_ms, 0);
    }

    #[test]
    fn caps_decode_accepts_a_struct_len_over_thirty_two() {
        // A future device with a longer struct. The host still asks for 32
        // bytes, so only 32 arrive; everything within them must still decode.
        let mut buf = caps_bytes();
        buf[0] = 48;

        let caps = Caps::decode(&buf).expect("decodes");
        assert_eq!(caps.struct_len, 48);
        assert_eq!(caps.num_gpios, 48);
        assert_eq!(caps.max_hold_ms, 1000);
    }

    #[test]
    fn caps_decode_accepts_a_longer_response() {
        // And if such a device is ever asked for more than 32 bytes, the extra
        // is ignored rather than rejected.
        let mut buf = caps_bytes().to_vec();
        buf[0] = 48;
        buf.extend_from_slice(&[0xCC; 16]);

        let caps = Caps::decode(&buf).expect("decodes");
        assert_eq!(caps.struct_len, 48);
        assert_eq!(caps.max_hold_ms, 1000);
    }

    #[test]
    fn caps_decode_accepts_a_short_response() {
        // A device that sent fewer bytes than it claims are meaningful. The
        // absent fields read as zero rather than erroring.
        let caps = Caps::decode(&caps_bytes()[..8]).expect("decodes");
        assert_eq!(caps.struct_len, 32);
        assert_eq!(caps.ext_major, 1);
        assert_eq!(caps.features, 0x0000_0007);
        assert_eq!(caps.num_gpios, 0);
        assert_eq!(caps.max_hold_ms, 0);
    }

    #[test]
    fn caps_decode_rejects_a_response_without_a_struct_len() {
        assert!(matches!(
            Caps::decode(&[0x20]),
            Err(DecodeError::CapsTooShort(1))
        ));
        assert!(matches!(
            Caps::decode(&[]),
            Err(DecodeError::CapsTooShort(0))
        ));
    }
}
