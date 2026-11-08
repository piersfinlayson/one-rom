// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Argument definitions for `onerom save`.

use crate::args::CommandTrait;
use crate::utils::parse_u8;
use clap::{ArgGroup, Args};

// clap's usage line joins --output, which --slot requires, to the choice of
// --slot or --path.
#[derive(Debug, Args)]
#[command(
    group(ArgGroup::new("what").required(true).args(["slot", "path"])),
    override_usage = "onerom save [OPTIONS] --slot <INDEX> --output <FILE>\n       \
                      onerom save [OPTIONS] --path <DIR>"
)]
pub struct SaveArgs {
    /// Slot to save the ROM image from. Slots are numbered as in
    /// 'onerom inspect slots'.
    #[arg(
        long,
        value_name = "INDEX",
        value_parser = parse_u8,
        requires = "output",
        conflicts_with = "path"
    )]
    pub slot: Option<u8>,

    /// Chip to save from a slot with multiple chips. Chips are numbered from 0.
    // clap treats a requirement as met where the required argument conflicts
    // with one present, so --chip also conflicts with --path.
    #[arg(
        long,
        value_name = "INDEX",
        value_parser = parse_u8,
        requires = "slot",
        conflicts_with = "path"
    )]
    pub chip: Option<u8>,

    /// File to save the ROM image to.
    #[arg(
        long,
        short,
        visible_alias = "out",
        value_name = "FILE",
        requires = "slot",
        conflicts_with = "path"
    )]
    pub output: Option<String>,

    /// Directory to save config.json and a file for each ROM image to.
    /// Created if it doesn't exist.
    #[arg(long, value_name = "DIR")]
    pub path: Option<String>,

    /// Firmware file to read instead of a connected One ROM.
    #[arg(long, visible_aliases = ["fw", "in", "input"], value_name = "FILE")]
    pub firmware: Option<String>,

    /// Overwrite existing files.
    #[arg(long, short)]
    pub force: bool,
}

impl CommandTrait for SaveArgs {
    fn requires_device(&self) -> bool {
        self.firmware.is_none()
    }

    fn uses_device(&self) -> bool {
        self.firmware.is_none()
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use clap::error::ErrorKind;

    use super::*;
    use crate::args::{Cli, Commands};

    /// `onerom save` and `options`, parsed.
    fn save(options: &[&str]) -> Result<SaveArgs, clap::Error> {
        let cli = Cli::try_parse_from(["onerom", "save"].iter().chain(options))?;
        let Commands::Save(args) = cli.command else {
            panic!("not save");
        };
        Ok(args)
    }

    #[test]
    fn force_with_output_parses() {
        let args = save(&["--slot", "0", "--output", "a.bin", "--force"]).unwrap();
        assert_eq!(args.output.as_deref(), Some("a.bin"));
        assert!(args.force);
    }

    #[test]
    fn chip_conflicts_with_path() {
        let e = save(&["--chip", "1", "--path", "c64"]).unwrap_err();
        assert_eq!(e.kind(), ErrorKind::ArgumentConflict);
    }

    /// Each case's error prints the usage lines, which start with the command.
    #[test]
    fn no_usage_line_joins_output_to_path() {
        for options in [
            &[][..],
            &["--slot", "0"],
            &["--path", "c64", "--output", "a.bin"],
        ] {
            let usage = save(options).unwrap_err().render().to_string();
            let lines: Vec<&str> = usage
                .lines()
                .filter(|l| l.contains("onerom save") && l.contains("--path"))
                .collect();
            assert!(!lines.is_empty(), "{usage}");
            for line in lines {
                assert!(!line.contains("--output"), "{usage}");
            }
        }
    }

    #[test]
    fn force_with_path_parses() {
        let args = save(&["--path", "c64", "--force"]).unwrap();
        assert_eq!(args.path.as_deref(), Some("c64"));
        assert!(args.force);
    }
}
