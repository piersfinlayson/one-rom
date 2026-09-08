// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom control`.

use crate::args::CommandTrait;
use crate::utils::{
    parse_beacon_period, parse_blink_period, parse_breathe_period, parse_brightness,
    parse_cycle_period, parse_flame_period, parse_gpio_hold_ms, parse_hold_ms, parse_u8, parse_u32,
};
use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use enum_dispatch::enum_dispatch;

use onerom_cli::colour::{RgbColour, parse_colour};
use onerom_cli::pin::{Pin, parse_pin};
use onerom_cli::usb::{GpioState as WireGpioState, RebootArgs};
use onerom_metadata::GPIO_RESET_DEFAULT_HOLD_MS;

#[derive(Debug, Args)]
pub struct ControlArgs {
    #[command(subcommand)]
    pub command: ControlCommands,
}

impl CommandTrait for ControlArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum ControlCommands {
    /// Reboot the One ROM.
    ///
    /// Restarts the One ROM firmware. The device will re-initialise and
    /// resume serving ROM images after the reboot.
    ///
    /// By default, this command pauses after a reboot to give the device time
    /// to re-enumerate.
    ///
    /// Example:
    ///
    ///   onerom control reboot
    Reboot(ControlRebootArgs),

    /// Control the status LED on a One ROM.
    ///
    /// Examples:
    ///
    ///   onerom control led on
    ///
    ///   onerom control led off
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Led(ControlLedArgs),

    /// Control the RGB LED on a One ROM.
    ///
    /// Only some One ROM models have an RGB LED. Run 'onerom inspect gpio' to
    /// see what a board has.
    ///
    /// Examples:
    ///
    ///   onerom control rgb on --colour red
    ///
    ///   onerom control rgb on --colour #FF8000 --brightness 40
    ///
    ///   onerom control rgb off
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Rgb(ControlRgbArgs),

    /// Write data to One ROM's SRAM or the live ROM image.
    ///
    /// Poke provides transient (non-persistent) writes to device memory. Changes
    /// are lost on reboot. Use `update` subcommands for persistent flash writes.
    ///
    /// Data can be written as a single byte value or from a binary file.
    ///
    /// Example:
    ///
    ///   onerom control poke memory --address 0x20000010 --byte 0xFF
    ///
    ///   onerom control poke memory --address 0x20000010 --input patch.bin
    ///
    ///   onerom control poke live --address 0x100 --byte 0xEA
    ///
    ///   onerom control poke live --address 0x100 --input patch.bin
    #[command(
        subcommand_value_name = "COMMAND",
        subcommand_help_heading = "Commands"
    )]
    Poke(ControlPokeArgs),

    /// Pulse a One ROM GPIO low to reset the host system.
    ///
    /// Pulses the named GPIO low, then releases it to high impedance, to
    /// reset the host system the One ROM is installed in. Useful in scripted
    /// workflows to reset the host after programming a new ROM image.
    ///
    /// --pin is the pin your reset wire is soldered to - typically an X pad, or
    /// an image-select pad whose jumper you have removed. Name it by pad
    /// ('x1', 'sel_a') or by MCU GPIO ('gpio9'). Run 'onerom inspect header' to
    /// see which GPIO is behind each pad.
    ///
    /// The reset line is only ever driven low and then released: a reset net
    /// has its own pull-up and may have other drivers on it, so there is
    /// deliberately no way to drive it high. Use 'onerom control pin' if you
    /// need arbitrary states.
    ///
    /// The device times the pulse, not this command, so an interrupted CLI
    /// cannot leave the host held in reset.
    ///
    /// Examples:
    ///
    ///   onerom control reset --pin x1
    ///
    ///   onerom control reset --pin gpio9 --hold 500
    Reset(ControlResetArgs),

    /// Select the active ROM slot (not yet supported).
    ///
    /// Switches the device to serving the specified image slot. This takes
    /// effect immediately but does not persist across power cycles unless.
    ///
    /// Example:
    ///
    ///   onerom control select --slot 2
    Select(ControlSelectArgs),

    /// Drive a One ROM pin high, low or high-impedance.
    ///
    /// --pin names an MCU GPIO, written 'gpio<N>', or a header pad: 'sel_a'
    /// to 'sel_e', 'x1' or 'x2'. Run 'onerom inspect header' to see which GPIO
    /// is behind each header pad, and 'onerom inspect gpio' to see what One ROM
    /// is currently using each GPIO for.
    ///
    /// Without --hold the state is latched until something changes it. With
    /// --hold the device holds the state for that many milliseconds and then
    /// applies --then (high impedance unless you say otherwise). The device
    /// times the hold, not this command, so an interrupted CLI cannot leave a
    /// pin latched.
    ///
    /// A GPIO One ROM is itself using is refused unless --force is given.
    /// Forcing a pin One ROM drives (a data pin) breaks ROM serving until the
    /// device is rebooted; forcing one it only reads (address, chip-select,
    /// /BYTE) is undone by setting it back to z.
    ///
    /// Examples:
    ///
    ///   onerom control pin --pin gpio9 --state high
    ///
    ///   onerom control pin --pin x1 --state 0 --hold 250
    ///
    ///   onerom control pin --pin sel_a --state z
    Pin(ControlPinArgs),

    /// Erase this One ROM's flash memory.
    ///
    /// Permanently erase flash contents on the device, including firmware,
    /// metadata and ROM images.
    ///
    /// If a One ROM's firmware has been erased it will subsequently boot into
    /// the RP2350 bootloader from where it can be reprogrammed.  However, you
    /// will need to use the --unrecognized to detect and program it.
    ///
    /// It is highly recommended that this command is used when One ROM is
    /// stopped (and the default is this command will reboot the device if
    /// required before erasing to make it so).
    ///
    /// Use with extreme caution while One ROM is running.  Erasing the core
    /// firmware or the system plugin's flash will cause the USB stack to be
    /// non-functional, requiring manually forcing into BOOTSEL mode using One
    /// ROM's header pins.  In addition, erasing flash causes One ROM to
    /// temporarily suspend interrupts and cause flash to become inaccessible.
    /// Anything else running from flash (like a user plugin) may well crash
    /// as a result.
    ///
    /// For a similar reason, large erase operations while running may cause
    /// One ROM's USB support to become unavailable and then re-enumerate
    /// after the flash erase.  In this case, the flash likely succeeded and
    /// can be checked with `inspect peek memory`.
    ///
    /// You can use this command to erase multiple ranges in a single operation
    /// with multiple --offset/--address and --length arguments.
    ///
    /// Example:
    ///
    ///   onerom control erase -a
    ///
    ///   onerom control erase --offset 0x20000 --length 0x1000
    Erase(ControlEraseArgs),
}

#[derive(Debug, Args)]
pub struct ControlLedArgs {
    #[command(subcommand)]
    pub command: ControlLedCommands,
}

impl CommandTrait for ControlLedArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum ControlLedCommands {
    /// Turn the status LED on.
    On(ControlLedOnArgs),
    /// Turn the status LED off.
    Off(ControlLedOffArgs),
    /// Beacon the status LED to identify a physical One ROM.
    Beacon(ControlLedBeaconArgs),
    /// Flame the status LED.
    Flame(ControlLedFlameArgs),
    /// Blink the status LED until something changes it.
    Blink(ControlLedBlinkArgs),
}

// What `--period` and `--hold` say they default to, taken from the metadata
// schema so the number a user reads is the number the firmware was built with.
//
// These are `help =` rather than doc comments because clap reads a doc comment
// as a literal and would see an unexpanded macro here, leaving the option with
// no help at all.  See the `const_str!` macro in this module's parent.
const HELP_RESET_HOLD: &str = concat!(
    "Milliseconds to hold the reset signal asserted. Defaults to ",
    const_str!("GPIO_RESET_DEFAULT_HOLD_MS"),
    "."
);

const HELP_BEACON_PERIOD: &str = concat!(
    "Milliseconds for one blink. Defaults to ",
    const_str!("LED_BEACON_DEFAULT_PERIOD_MS"),
    "."
);

const HELP_BEACON_HOLD: &str = concat!(
    "Milliseconds to beacon for, before going back to what the LED was doing \
     before. Defaults to ",
    const_str!("LED_BEACON_DEFAULT_DURATION_MS"),
    "."
);

const HELP_FLAME_PERIOD: &str = concat!(
    "Milliseconds for one pass of the flicker. Defaults to ",
    const_str!("LED_FLAME_DEFAULT_PERIOD_MS"),
    "."
);

const HELP_CYCLE_PERIOD: &str = concat!(
    "Milliseconds for one rotation of the hues. Defaults to ",
    const_str!("LED_CYCLE_DEFAULT_PERIOD_MS"),
    "."
);

const HELP_BREATHE_PERIOD: &str = concat!(
    "Milliseconds for one fade up and down. Defaults to ",
    const_str!("LED_BREATHE_DEFAULT_PERIOD_MS"),
    "."
);

const HELP_BLINK_PERIOD: &str = concat!(
    "Milliseconds for one on and off. Defaults to ",
    const_str!("LED_BLINK_DEFAULT_PERIOD_MS"),
    "."
);

#[derive(Debug, Args)]
pub struct ControlLedOnArgs {
    /// Stay on for this many milliseconds, then go back to what the LED was
    /// doing before.
    ///
    /// The device times the hold, so it completes even if this command does
    /// not.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlLedOnArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlLedOffArgs {
    /// Stay off for this many milliseconds, then go back to what the LED was
    /// doing before.
    ///
    /// The device times the hold, so it completes even if this command does
    /// not.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlLedOffArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlLedBeaconArgs {
    #[arg(long, value_name = "MS", value_parser = parse_beacon_period, help = HELP_BEACON_PERIOD)]
    pub period: Option<u16>,

    #[arg(long, value_name = "MS", value_parser = parse_hold_ms, help = HELP_BEACON_HOLD)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlLedBeaconArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlLedFlameArgs {
    #[arg(long, value_name = "MS", value_parser = parse_flame_period, help = HELP_FLAME_PERIOD)]
    pub period: Option<u16>,

    /// Flicker for this many milliseconds, then go back to what the LED was
    /// doing before.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlLedFlameArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbArgs {
    #[command(subcommand)]
    pub command: ControlRgbCommands,
}

impl CommandTrait for ControlRgbArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum ControlRgbCommands {
    /// Light the RGB LED at a colour.
    On(ControlRgbOnArgs),
    /// Turn the RGB LED off.
    Off(ControlRgbOffArgs),
    /// Beacon the RGB LED to identify a physical One ROM.
    Beacon(ControlRgbBeaconArgs),
    /// Flame the RGB LED.
    Flame(ControlRgbFlameArgs),
    /// Rotate the RGB LED through the hues.
    Cycle(ControlRgbCycleArgs),
    /// Fade the RGB LED up and down.
    Breathe(ControlRgbBreatheArgs),
    /// Alternate the RGB LED between a colour and dark.
    Blink(ControlRgbBlinkArgs),
}

#[derive(Debug, Args)]
pub struct ControlRgbOnArgs {
    /// Colour to light: a name, or a hex colour written '#RRGGBB' or
    /// '0xRRGGBB'.
    ///
    /// Named colours are red, green, blue, white, yellow, cyan, magenta,
    /// orange, purple and pink.
    #[arg(long, visible_alias = "color", value_name = "COLOUR", value_parser = parse_colour, default_value = "red")]
    pub colour: RgbColour,

    /// Brightness as a percentage, 1 to 100.
    ///
    /// Omit to use the device's default, which is deliberately modest - an RGB
    /// LED at full output is uncomfortable at desk distance.
    #[arg(long, value_name = "PERCENT", value_parser = parse_brightness)]
    pub brightness: Option<u8>,

    /// Hold this for this many milliseconds, then go back to what the LED was
    /// doing before.
    ///
    /// The device times the hold, so it completes even if this command does
    /// not.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbOnArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbOffArgs {
    /// Stay off for this many milliseconds, then go back to what the LED was
    /// doing before.
    ///
    /// The device times the hold, so it completes even if this command does
    /// not.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbOffArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbBeaconArgs {
    /// Colour to beacon: a name, or a hex colour written '#RRGGBB' or
    /// '0xRRGGBB'.
    #[arg(long, visible_alias = "color", value_name = "COLOUR", value_parser = parse_colour, default_value = "red")]
    pub colour: RgbColour,

    /// Brightness as a percentage, 1 to 100.
    #[arg(long, value_name = "PERCENT", value_parser = parse_brightness)]
    pub brightness: Option<u8>,

    #[arg(long, value_name = "MS", value_parser = parse_beacon_period, help = HELP_BEACON_PERIOD)]
    pub period: Option<u16>,

    #[arg(long, value_name = "MS", value_parser = parse_hold_ms, help = HELP_BEACON_HOLD)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbBeaconArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbFlameArgs {
    /// Colour to flicker: a name, or a hex colour written '#RRGGBB' or
    /// '0xRRGGBB'.
    #[arg(long, visible_alias = "color", value_name = "COLOUR", value_parser = parse_colour, default_value = "red")]
    pub colour: RgbColour,

    /// Brightness as a percentage, 1 to 100.
    #[arg(long, value_name = "PERCENT", value_parser = parse_brightness)]
    pub brightness: Option<u8>,

    #[arg(long, value_name = "MS", value_parser = parse_flame_period, help = HELP_FLAME_PERIOD)]
    pub period: Option<u16>,

    /// Hold this for this many milliseconds, then go back to what the LED was
    /// doing before.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbFlameArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbCycleArgs {
    #[arg(long, value_name = "MS", value_parser = parse_cycle_period, help = HELP_CYCLE_PERIOD)]
    pub period: Option<u16>,

    /// Brightness as a percentage, 1 to 100.
    #[arg(long, value_name = "PERCENT", value_parser = parse_brightness)]
    pub brightness: Option<u8>,

    /// Hold this for this many milliseconds, then go back to what the LED was
    /// doing before.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbCycleArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbBreatheArgs {
    /// Colour to fade: a name, or a hex colour written '#RRGGBB' or
    /// '0xRRGGBB'.
    #[arg(long, visible_alias = "color", value_name = "COLOUR", value_parser = parse_colour, default_value = "red")]
    pub colour: RgbColour,

    #[arg(long, value_name = "MS", value_parser = parse_breathe_period, help = HELP_BREATHE_PERIOD)]
    pub period: Option<u16>,

    /// Brightness as a percentage, 1 to 100. The fade peaks at this.
    #[arg(long, value_name = "PERCENT", value_parser = parse_brightness)]
    pub brightness: Option<u8>,

    /// Hold this for this many milliseconds, then go back to what the LED was
    /// doing before.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbBreatheArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlRgbBlinkArgs {
    /// Colour to blink: a name, or a hex colour written '#RRGGBB' or
    /// '0xRRGGBB'.
    #[arg(long, visible_alias = "color", value_name = "COLOUR", value_parser = parse_colour, default_value = "red")]
    pub colour: RgbColour,

    #[arg(long, value_name = "MS", value_parser = parse_blink_period, help = HELP_BLINK_PERIOD)]
    pub period: Option<u16>,

    /// Brightness as a percentage, 1 to 100.
    #[arg(long, value_name = "PERCENT", value_parser = parse_brightness)]
    pub brightness: Option<u8>,

    /// Hold this for this many milliseconds, then go back to what the LED was
    /// doing before.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlRgbBlinkArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlLedBlinkArgs {
    #[arg(long, value_name = "MS", value_parser = parse_blink_period, help = HELP_BLINK_PERIOD)]
    pub period: Option<u16>,

    /// Blink for this many milliseconds, then go back to what the LED was
    /// doing before.
    ///
    /// Omit to blink until something changes it.
    #[arg(long, value_name = "MS", value_parser = parse_hold_ms)]
    pub hold: Option<u32>,
}

impl CommandTrait for ControlLedBlinkArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args, Clone)]
#[command(group = ArgGroup::new("reboot_mode").required(false).multiple(false))]
pub struct ControlRebootArgs {
    /// Reboot One ROM into stopped (bootloader) state
    #[arg(long, short = 'p', group = "reboot_mode")]
    pub stopped: bool,

    /// Reboot One ROM into running (byte serving) state (default).
    #[arg(long, short, group = "reboot_mode")]
    pub running: bool,

    /// Don't pause after reboot for One ROM to re-enumerate (reappear)
    /// on the USB bus.
    #[arg(long)]
    pub fast: bool,

    /// Mount mass storage device when rebooting into stopped mode.
    #[arg(long, short = 'm', conflicts_with = "running")]
    pub msd: bool,
}

impl CommandTrait for ControlRebootArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

impl From<&ControlRebootArgs> for RebootArgs {
    fn from(args: &ControlRebootArgs) -> Self {
        if args.stopped {
            RebootArgs::stopped(args.msd, args.fast)
        } else {
            // Default if unspecified
            RebootArgs::running(args.fast, true)
        }
    }
}

#[derive(Debug, Args)]
pub struct ControlResetArgs {
    /// Pin the reset wire is connected to: an MCU GPIO written gpio<N>, or a
    /// header pad name (sel_a..sel_e, x1, x2).
    ///
    /// A bare number is rejected - see 'onerom inspect header' for the GPIO
    /// behind each header pad.
    #[arg(long, value_name = "PIN", value_parser = parse_pin)]
    pub pin: Pin,

    /// Board type, overriding what the connected One ROM reports.
    ///
    /// Only needed to resolve a header pad name on a One ROM whose board type
    /// this build does not recognise. A GPIO named as gpio<N> needs no board.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    // `help =` rather than a doc comment: see the LED period help above.
    #[arg(long, value_name = "MS", default_value_t = GPIO_RESET_DEFAULT_HOLD_MS, value_parser = parse_gpio_hold_ms, help = HELP_RESET_HOLD)]
    pub hold: u32,
}

impl CommandTrait for ControlResetArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlSelectArgs {
    /// Image slot index to activate (0-15).
    #[arg(long, value_name = "INDEX")]
    pub slot: u8,
}

impl CommandTrait for ControlSelectArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum GpioState {
    /// Drive the pin high. Also accepted as '1'.
    #[value(alias = "1")]
    High,
    /// Drive the pin low. Also accepted as '0'.
    #[value(alias = "0")]
    Low,
    /// Set the pin to high-impedance (tri-state).
    Z,
}

impl From<GpioState> for WireGpioState {
    fn from(state: GpioState) -> Self {
        match state {
            GpioState::High => WireGpioState::High,
            GpioState::Low => WireGpioState::Low,
            // The wire (and firmware) call high impedance "input", because
            // releasing the output driver is what makes it so. The CLI spells
            // it 'z', which is what a schematic calls it.
            GpioState::Z => WireGpioState::Input,
        }
    }
}

impl std::fmt::Display for GpioState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpioState::High => write!(f, "high"),
            GpioState::Low => write!(f, "low"),
            GpioState::Z => write!(f, "high impedance"),
        }
    }
}

#[derive(Debug, Args)]
pub struct ControlPinArgs {
    /// Pin to drive: an MCU GPIO written gpio<N>, or a header pad name
    /// (sel_a..sel_e, x1, x2).
    ///
    /// A bare number is rejected - see 'onerom inspect header' for the GPIO
    /// behind each header pad.
    #[arg(long, value_name = "PIN", value_parser = parse_pin)]
    pub pin: Pin,

    /// Board type, overriding what the connected One ROM reports.
    ///
    /// Only needed to resolve a header pad name on a One ROM whose board type
    /// this build does not recognise. A GPIO named as gpio<N> needs no board.
    #[arg(long, short, value_name = "BOARD")]
    pub board: Option<String>,

    /// Desired pin state: high, low, or z (high-impedance).
    ///
    /// '1' and '0' are accepted for high and low.
    #[arg(long, value_name = "STATE")]
    pub state: GpioState,

    /// Hold --state for this many milliseconds, then apply --then.
    ///
    /// Omit to latch --state until something else changes it. The device times
    /// the hold, so it completes even if this command does not.
    #[arg(long, value_name = "MS", value_parser = parse_gpio_hold_ms)]
    pub hold: Option<u32>,

    /// State to apply when --hold expires. Defaults to z. Requires --hold.
    ///
    /// '1' and '0' are accepted for high and low.
    #[arg(long, value_name = "STATE", requires = "hold")]
    pub then: Option<GpioState>,

    /// Drive the GPIO even though One ROM is using it for serving.
    #[arg(long, short)]
    pub force: bool,
}

impl CommandTrait for ControlPinArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
pub struct ControlPokeArgs {
    #[command(subcommand)]
    pub command: ControlPokeCommands,
}

impl CommandTrait for ControlPokeArgs {
    fn requires_device(&self) -> bool {
        self.command.requires_device()
    }
}

#[derive(Debug, Args)]
#[command(group = ArgGroup::new("erase_target").required(true).args(["all", "offset", "address"]))]
#[command(group = ArgGroup::new("reboot_mode").required(false).args(["stopped", "running"]))]
pub struct ControlEraseArgs {
    /// Erase all flash contents.
    #[arg(long, short)]
    pub all: bool,

    /// Erase at offset(s) relative to flash base (0x10000000).
    ///
    /// Must be 4096-aligned. Pair each with a --length.
    /// Can be repeated for multiple ranges.
    /// Mutually exclusive with --address.
    #[arg(long, value_name = "OFFSET", value_parser = parse_u32, action = clap::ArgAction::Append, conflicts_with = "address", requires = "length")]
    pub offset: Vec<u32>,

    /// Erase at absolute address(es).
    ///
    /// Must be 4096-aligned. Pair each with a --length.
    /// Can be repeated for multiple ranges.
    /// Mutually exclusive with --offset.
    #[arg(long, visible_alias = "addr", value_name = "ADDRESS", value_parser = parse_u32, action = clap::ArgAction::Append, conflicts_with = "offset", requires = "length")]
    pub address: Vec<u32>,

    /// Length of each range to erase (paired with --offset or --address).
    ///
    /// Must be 4096-aligned. Specify once per --offset/--address.
    #[arg(long, visible_aliases = ["len", "size"], value_name = "LENGTH", value_parser = parse_u32, action = clap::ArgAction::Append, conflicts_with = "all")]
    pub length: Vec<u32>,

    /// Do not reboot before or after erasing.  This can be risky, if
    /// One ROM is actively accessing the flash range being erased.
    #[arg(long, short, conflicts_with = "reboot_mode")]
    pub no_reboot: bool,

    /// Reboot One ROM into stopped (bootloader) mode after erasing.
    #[arg(long, short = 'p', conflicts_with = "running")]
    pub stopped: bool,

    /// Reboot One ROM into running mode after erasing.
    #[arg(long, short = 'r', conflicts_with = "stopped")]
    pub running: bool,

    /// Mount mass storage device when rebooting into stopped mode.
    #[arg(long, short = 'm', requires = "stopped")]
    pub msd: bool,

    /// Don't pause after reboot for One ROM to re-enumerate (reappear)
    /// on the USB bus.
    #[arg(long, requires = "reboot_mode")]
    pub fast: bool,
}

impl CommandTrait for ControlEraseArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

impl From<&ControlEraseArgs> for RebootArgs {
    fn from(args: &ControlEraseArgs) -> Self {
        if args.stopped {
            RebootArgs::stopped(args.msd, args.fast)
        } else if args.running {
            RebootArgs::running(args.fast, true)
        } else {
            RebootArgs::none()
        }
    }
}

#[enum_dispatch(CommandTrait)]
#[derive(Debug, Subcommand)]
pub enum ControlPokeCommands {
    /// Write a single byte or binary file to One ROM's SRAM.
    ///
    /// Writes data directly to One ROM's SRAM at the specified address.
    /// This is a transient operation — changes are lost on reboot.
    ///
    /// The address must be a valid SRAM address.
    ///
    /// If One ROM is running, virtual addresses are available. For example
    /// 0x90000000 is the start of One ROM's live ROM image.  Prefer 'live'
    /// over 'memory' for poking the live ROM image.
    ///
    /// When writing a file, the entire file contents are written starting
    /// at the given address. When writing a single byte, only that byte is
    /// written.
    ///
    /// Note: writing to arbitrary SRAM addresses can corrupt firmware state.
    /// Use with caution.
    ///
    /// Example:
    ///   onerom control poke memory --address 0x20000010 --value 0xFF
    ///   onerom control poke memory --address 0x20000000 --input patch.bin
    Memory(ControlPokeMemoryArgs),

    /// Write a single byte or binary file to the live ROM image.
    ///
    /// Writes data to the ROM image currently being served by the device,
    /// at the specified logical ROM address (starting from 0). This is a
    /// transient operation — changes are lost on reboot.
    ///
    /// This is useful for patching a running ROM image without reflashing.
    /// The address is a logical ROM offset, not a physical memory address.
    ///
    /// Example:
    ///   onerom control poke live --address 0x100 --value 0xEA
    ///   onerom control poke live --address 0 --input patch.bin
    Live(ControlPokeLiveArgs),
}

#[derive(Debug, Args)]
#[command(group = ArgGroup::new("poke_source").required(true).multiple(false))]
pub struct ControlPokeMemoryArgs {
    /// Write to this memory address on the device.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    #[arg(long, short, visible_alias = "addr", value_name = "ADDRESS", value_parser = parse_u32)]
    pub address: u32,

    /// Write this single byte value.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    /// Mutually exclusive with --input.
    #[arg(long, visible_alias = "value", value_name = "BYTE", value_parser = parse_u8, group = "poke_source")]
    pub byte: Option<u8>,

    /// Write the contents of this binary file.
    ///
    /// Mutually exclusive with --value.
    #[arg(
        long,
        short,
        visible_alias = "in",
        value_name = "FILE",
        group = "poke_source"
    )]
    pub input: Option<String>,
}

impl CommandTrait for ControlPokeMemoryArgs {
    fn requires_device(&self) -> bool {
        true
    }
}

#[derive(Debug, Args)]
#[command(group = ArgGroup::new("poke_source").required(true).multiple(false))]
pub struct ControlPokeLiveArgs {
    /// Write to this logical ROM address, starting from 0.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    #[arg(long, short, visible_alias = "addr", value_name = "ADDRESS", value_parser = parse_u32, default_value = "0")]
    pub address: u32,

    /// Write this single byte value.
    ///
    /// Accepts decimal and hexadecimal (0x prefix) formats.
    /// Mutually exclusive with --input.
    #[arg(long, visible_alias = "value", value_name = "BYTE", value_parser = parse_u8, group = "poke_source")]
    pub byte: Option<u8>,

    /// Write the contents of this binary file.
    ///
    /// Mutually exclusive with --byte.
    #[arg(
        long,
        short,
        visible_alias = "in",
        value_name = "FILE",
        group = "poke_source"
    )]
    pub input: Option<String>,

    /// Only write bytes that differ from the current device's ROM content.
    ///
    /// Requires --input.
    #[arg(long, requires = "input", visible_alias = "deltas")]
    pub delta: bool,

    /// Show what would be written without actually writing.
    ///
    /// Requires --delta.
    #[arg(long, requires = "delta", visible_alias = "dryrun")]
    pub dry_run: bool,
}

impl CommandTrait for ControlPokeLiveArgs {
    fn requires_device(&self) -> bool {
        true
    }
}
