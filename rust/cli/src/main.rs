// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! onerom - One ROM command-line interface

use clap::{CommandFactory, FromArgMatches};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

mod args;
mod board;
mod control;
mod firmware;
mod image;
mod inspect;
mod plugin;
mod program;
mod scan;
mod update;
mod utils;

use args::Cli;
use args::Commands;
use args::control::{ControlCommands, ControlLedCommands, ControlPokeCommands};
use args::firmware::FirmwareCommands;
use args::image::ImageCommands;
use args::inspect::{InspectCommands, InspectPeekCommands};
use args::update::UpdateCommands;

use onerom_cli::Error;

#[tokio::main]
async fn main() {
    if let Err(e) = sub_main().await {
        eprintln!("Failed to execute command.\n{e}");
        std::process::exit(1);
    }
}

async fn sub_main() -> Result<(), Error> {
    // We need to convoluted call into clap so we can change the binary name to
    // onerom.
    let mut cli = Cli::from_arg_matches(&Cli::command().bin_name("onerom").get_matches())
        .unwrap_or_else(|e: clap::Error| e.exit());
    let mut options = cli.try_into_options().await?;

    utils::init_logging(&options);

    debug!("One ROM CLI v{}", env!("CARGO_PKG_VERSION"));

    match &cli.command {
        Commands::Scan(args) => scan::cmd_scan(&options, args).await,
        Commands::Firmware(args) => match &args.command {
            FirmwareCommands::Build(args) => firmware::cmd_build(&options, args).await,
            FirmwareCommands::Inspect(args) => firmware::cmd_inspect(&options, args).await,
            FirmwareCommands::Releases(args) => firmware::cmd_releases(&options, args).await,
            FirmwareCommands::Download(args) => firmware::cmd_download(&options, args).await,
            FirmwareCommands::Chips(args) => firmware::cmd_chips(&options, args).await,
            FirmwareCommands::Program(args) => program::cmd_program(&mut options, args).await,
        },
        Commands::Plugin(args) => plugin::cmd_plugin(&options, args).await,
        Commands::Program(args) => program::cmd_program(&mut options, args).await,
        Commands::Inspect(args) => match &args.command {
            InspectCommands::Info(args) => inspect::cmd_info(&options, args).await,
            InspectCommands::Telemetry(args) => inspect::cmd_telemetry(&options, args).await,
            InspectCommands::Slots(args) => inspect::cmd_slots(&options, args).await,
            InspectCommands::Image(args) => inspect::cmd_image(&options, args).await,
            InspectCommands::Gpio(args) => inspect::cmd_gpio(&options, args).await,
            InspectCommands::Peek(args) => match &args.command {
                InspectPeekCommands::Live(args) => inspect::cmd_peek_live(&options, args).await,
                InspectPeekCommands::Memory(args) => inspect::cmd_peek_memory(&options, args).await,
            },
        },
        Commands::Control(args) => match &args.command {
            ControlCommands::Led(args) => match &args.command {
                ControlLedCommands::On(args) => control::cmd_led_on(&options, args).await,
                ControlLedCommands::Off(args) => control::cmd_led_off(&options, args).await,
                ControlLedCommands::Beacon(args) => control::cmd_led_beacon(&options, args).await,
                ControlLedCommands::Flame(args) => control::cmd_led_flame(&options, args).await,
            },
            ControlCommands::Reboot(args) => control::cmd_reboot(&options, args).await,
            ControlCommands::Reset(args) => control::cmd_reset(&options, args).await,
            ControlCommands::Select(args) => control::cmd_select(&options, args).await,
            ControlCommands::Gpio(args) => control::cmd_gpio(&options, args).await,
            ControlCommands::Poke(args) => match &args.command {
                ControlPokeCommands::Memory(args) => control::cmd_poke_memory(&options, args).await,
                ControlPokeCommands::Live(args) => control::cmd_poke_live(&options, args).await,
            },
            ControlCommands::Erase(args) => control::cmd_erase(&mut options, args).await,
        },
        Commands::Update(args) => match &args.command {
            UpdateCommands::Slot(args) => update::cmd_slot(&options, args).await,
            UpdateCommands::Commit(args) => update::cmd_commit(&options, args).await,
            UpdateCommands::Otp(args) => update::cmd_otp(&options, args).await,
        },
        Commands::Image(args) => match &args.command {
            ImageCommands::SwapBytes(args) => image::cmd_swap_bytes(&options, args).await,
        },
        Commands::Peek(args) => inspect::cmd_peek_live(&options, args).await,
        Commands::Poke(args) => control::cmd_poke_live(&options, args).await,
        Commands::Reboot(args) => control::cmd_reboot(&options, args).await,
        Commands::Chips(args) => firmware::cmd_chips(&options, args).await,
        Commands::Boards(args) => board::cmd_boards(&options, args).await,
    }
}
