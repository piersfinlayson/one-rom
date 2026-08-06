// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! onerom - One ROM command-line interface

use clap::{CommandFactory, FromArgMatches};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};

mod args;
mod board;
mod board_view;
mod control;
mod firmware;
mod image;
mod inspect;
mod plugin;
mod program;
mod scan;
mod update;
mod utils;

use args::BoardCommands;
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
            InspectCommands::Header(args) => inspect::cmd_header(&options, args).await,
            InspectCommands::Socket(args) => inspect::cmd_socket(&options, args).await,
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
            ControlCommands::Pin(args) => control::cmd_pin(&options, args).await,
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
            ImageCommands::Deinterleave(args) => image::cmd_deinterleave(&options, args).await,
            ImageCommands::Convert(args) => image::cmd_convert(&options, args).await,
        },
        Commands::Peek(args) => inspect::cmd_peek_live(&options, args).await,
        Commands::Poke(args) => control::cmd_poke_live(&options, args).await,
        Commands::Reboot(args) => control::cmd_reboot(&options, args).await,
        Commands::Chips(args) => firmware::cmd_chips(&options, args).await,
        Commands::Board(args) => match &args.command {
            BoardCommands::List(args) => board::cmd_list(&options, args).await,
            BoardCommands::Header(args) => board::cmd_header(&options, args).await,
            BoardCommands::Socket(args) => board::cmd_socket(&options, args).await,
        },
    }
}

#[cfg(test)]
mod cli_assert {
    use super::*;

    /// Validate the whole clap command tree at test time.
    ///
    /// `debug_assert()` clones the command, propagates global arguments into
    /// every subcommand, and runs clap's internal uniqueness checks - catching
    /// misconfigurations such as a subcommand short option colliding with a
    /// global one (e.g. `--input`'s `-i` clashing with the global `--vid-pid`
    /// `-i`) that otherwise only surface as a panic when the offending
    /// subcommand is invoked. Keep this test so such collisions fail CI rather
    /// than reaching users.
    #[test]
    fn verify_cli() {
        Cli::command().debug_assert();
    }

    /// Walk every subcommand, deepest first, yielding `(path, command)`.
    fn walk(cmd: &clap::Command, path: &str, out: &mut Vec<(String, clap::Command)>) {
        for sub in cmd.get_subcommands() {
            let sub_path = if path.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{path} {}", sub.get_name())
            };
            walk(sub, &sub_path, out);
            out.push((sub_path, sub.clone()));
        }
    }

    fn all_subcommands() -> Vec<(String, clap::Command)> {
        let mut out = Vec::new();
        walk(&Cli::command(), "", &mut out);
        assert!(
            out.len() > 20,
            "command tree looks truncated: {}",
            out.len()
        );
        out
    }

    /// A short flag means one thing across the whole CLI.
    ///
    /// Not the same claim as [`verify_cli`], which only catches two options
    /// colliding on the *same* command. This catches `-b` meaning `--board` on
    /// one command and `--byte` on another: legal clap, but the user has to
    /// remember which is which. `-a` is the sole documented exception, covered
    /// by its own test below.
    #[test]
    fn a_short_flag_means_the_same_thing_everywhere() {
        const EXEMPT: &[char] = &['a'];
        let mut seen: std::collections::HashMap<char, (String, String)> =
            std::collections::HashMap::new();
        for (path, cmd) in all_subcommands() {
            for arg in cmd.get_arguments() {
                let (Some(short), Some(long)) = (arg.get_short(), arg.get_long()) else {
                    continue;
                };
                if EXEMPT.contains(&short) {
                    continue;
                }
                let entry = seen
                    .entry(short)
                    .or_insert_with(|| (long.to_string(), path.clone()));
                assert_eq!(
                    entry.0, long,
                    "-{short} is --{} on '{}' but --{long} on '{path}'",
                    entry.0, entry.1
                );
            }
        }
        // Guard against the loop silently matching nothing.
        assert_eq!(seen.get(&'b').map(|e| e.0.as_str()), Some("board"));
        assert_eq!(seen.get(&'o').map(|e| e.0.as_str()), Some("output"));
    }

    /// `-a` is the one letter carrying two meanings, and no command has both.
    ///
    /// `--address` and `--all` are each long-established on their own commands
    /// and never appear together, so the ambiguity is never presented to a
    /// user. This test fixes that as a deliberate exception rather than an
    /// oversight: adding `-a` to a third meaning, or to a command that already
    /// has the other, fails here.
    #[test]
    fn the_short_a_exception_stays_unambiguous() {
        let mut meanings = std::collections::BTreeSet::new();
        for (path, cmd) in all_subcommands() {
            let on_this_command: Vec<_> = cmd
                .get_arguments()
                .filter(|a| a.get_short() == Some('a'))
                .filter_map(|a| a.get_long())
                .collect();
            assert!(
                on_this_command.len() <= 1,
                "'{path}' gives -a to more than one option: {on_this_command:?}"
            );
            meanings.extend(on_this_command.into_iter().map(str::to_string));
        }
        assert_eq!(
            meanings,
            ["address", "all", "all-versions"]
                .iter()
                .map(|s| s.to_string())
                .collect::<std::collections::BTreeSet<_>>(),
        );
    }

    /// Every argument is `--name value`; the CLI has no positionals.
    ///
    /// `board header <board>` used to be the one exception, and its error
    /// message told the user to pass `--board` - advice the command could not
    /// take. Keep the rule mechanical so the next one cannot creep in.
    #[test]
    fn no_command_takes_a_positional_argument() {
        for (path, cmd) in all_subcommands() {
            let positionals: Vec<_> = cmd
                .get_positionals()
                .filter_map(|a| a.get_id().as_str().into())
                .collect();
            assert!(
                positionals.is_empty(),
                "'{path}' takes positional argument(s): {positionals:?}"
            );
        }
    }

    /// An option never lists its own long name as an alias.
    ///
    /// A self-alias renders in `--help` as `--turbo-boot [aliases:
    /// --turbo-boot]`, which reads as though the two spellings differ.
    #[test]
    fn no_option_aliases_itself() {
        for (path, cmd) in all_subcommands() {
            for arg in cmd.get_arguments() {
                let Some(long) = arg.get_long() else { continue };
                let aliases: Vec<_> = arg.get_visible_aliases().unwrap_or_default();
                assert!(
                    !aliases.contains(&long),
                    "'{path}' --{long} lists itself as an alias"
                );
            }
        }
    }

    /// Every option documents itself, and names its value in one word.
    ///
    /// `--serial-override` shipped with a `//` comment where clap needs `///`,
    /// so it appeared in `--help` with no description at all, and with a
    /// `<NEW SERIAL>` value name that a space made read as two arguments.
    #[test]
    fn every_option_is_documented_with_a_single_word_value_name() {
        for (path, cmd) in all_subcommands() {
            for arg in cmd.get_arguments() {
                let Some(long) = arg.get_long() else { continue };
                assert!(
                    arg.get_help().is_some() || arg.get_long_help().is_some(),
                    "'{path}' --{long} has no help text"
                );
                for value_name in arg.get_value_names().unwrap_or_default() {
                    assert!(
                        !value_name.as_str().contains(' '),
                        "'{path}' --{long} value name '{value_name}' contains a space"
                    );
                }
            }
        }
    }
}
