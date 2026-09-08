// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The screen, driven the way the app drives it.
//!
//! Every test here goes through [`Screen::update`] with a real [`Message`] and
//! reads the result off the screen's own accessors — nothing pokes a field.
//! That matters more than usual for a generated screen: the thing under test
//! is whether the description alone is enough, and a test that sets up state
//! by hand has answered a different question.
//!
//! Nothing here names a command.  Each test looks through `COMMANDS` for one
//! with the property it is about, so it keeps testing the real description as
//! the CLI grows.

use onerom_cli::colour::RgbColour;
use onerom_cli::pin::parse_pin;
use onerom_config::chip::CHIP_TYPES;
use onerom_config::hw::{Board, Model};
use studiov2_commands::{COMMANDS, Command, GLOBALS, Kind, Opt, Source, name};
use studiov2_generated::form::{Target, Where};
use studiov2_generated::resolve::{self, Browse, Context, Offer};
use studiov2_generated::{Message, Screen, real, stub, ui};
use studiov2_shared::Shared;

/// A screen and the state it reads.
struct Harness {
    /// The screen under test.
    screen: Screen,
    /// What every screen shares.  A temporary log, and one made-up device.
    shared: Shared,
}

impl Harness {
    /// A fresh screen over a temporary log.
    fn new() -> Self {
        Self {
            screen: Screen::new(),
            shared: Shared::stub().expect("a temporary log store"),
        }
    }

    /// Sends a message the way the application does.
    ///
    /// The returned task is dropped: every message a test sends is one that
    /// finishes in `update`, and the one that does not — `Run` — is driven by
    /// sending its reply itself.
    fn send(&mut self, message: Message) {
        let _ = self.screen.update(message, &mut self.shared);
    }

    /// Selects a command by index, through the tabs.
    fn select(&mut self, index: usize) {
        let path = COMMANDS[index].path;
        for (depth, segment) in path.iter().enumerate() {
            self.send(Message::Tab(depth, segment));
        }
        assert_eq!(
            self.screen.command().map(|command| command.path),
            Some(path),
            "clicking the tabs of {path:?} should land on it"
        );
    }

    /// The command line the screen currently describes.
    fn line(&self) -> String {
        self.screen.command_line()
    }
}

/// The first command with an option the test is about, and that option's
/// index.
fn find(wanted: impl Fn(&Opt) -> bool) -> Option<(usize, usize)> {
    COMMANDS.iter().enumerate().find_map(|(index, command)| {
        let opt = command.opts.iter().position(&wanted)?;
        Some((index, opt))
    })
}

/// How a command is named in a failure message.
fn name(command: &Command) -> String {
    command.path.join(" ")
}

#[test]
fn every_command_draws_a_pane() {
    let mut harness = Harness::new();

    for (index, command) in COMMANDS.iter().enumerate() {
        harness.select(index);

        // Building the element runs every line of the pane's view: the
        // heading, the widget mapping for each option, the command line and
        // the run row.
        let element = harness.screen.view(&harness.shared);
        drop(element);

        // And again with a result on screen, which is a different path.
        let outcome = stub::run(command, &harness.line(), false);
        harness.send(Message::Ran(outcome));
        drop(harness.screen.view(&harness.shared));
    }

    println!("{} panes drawn", COMMANDS.len());
}

#[test]
fn every_command_has_an_output_shape_that_renders() {
    for command in COMMANDS {
        let line = format!("onerom {}", name(command));

        let ok = stub::run(command, &line, false).expect("the happy path succeeds");
        if let stub::Output::Tree(root) = &ok {
            assert!(
                !ui::flatten(root).is_empty(),
                "{}: a tree must flatten to at least its root",
                name(command)
            );
        }

        let failed = stub::run(command, &line, true);
        assert!(
            failed.is_err(),
            "{}: the forced error path must fail",
            name(command)
        );
    }
}

#[test]
fn a_flag_reaches_the_command_line() {
    let Some((index, opt)) = find(|opt| matches!(opt.kind, Kind::Flag)) else {
        panic!("no command in the description has a flag");
    };

    let mut harness = Harness::new();
    harness.select(index);

    let long = COMMANDS[index].opts[opt].long;
    let before = harness.line();
    assert!(
        !before.contains(&format!("--{long}")),
        "an unticked flag must not be on the line: {before}"
    );

    harness.send(Message::Toggled(
        Target::new(Where::Command(index), opt),
        true,
    ));

    let after = harness.line();
    println!("{after}");
    assert!(
        after.contains(&format!("--{long}")),
        "a ticked flag must reach the line: {after}"
    );
    assert!(
        !after.contains(&format!("--{long} ")) || after.ends_with(&format!("--{long}")),
        "a flag takes no value: {after}"
    );
}

#[test]
fn a_choice_reaches_the_command_line() {
    let Some((index, opt)) = find(|opt| matches!(opt.kind, Kind::Choice(_))) else {
        panic!("no command in the description has a choice");
    };

    let spec = &COMMANDS[index].opts[opt];
    let Kind::Choice(values) = &spec.kind else {
        unreachable!("found by matching on Choice")
    };
    let chosen = values.first().expect("a choice offers at least one value");

    let mut harness = Harness::new();
    harness.select(index);
    harness.send(Message::Edited(
        Target::new(Where::Command(index), opt),
        (*chosen).to_owned(),
    ));

    let line = harness.line();
    println!("{line}");
    assert!(
        line.contains(&format!("--{} {chosen}", spec.long)),
        "a picked value must reach the line: {line}"
    );
}

#[test]
fn a_default_is_on_the_line_before_anybody_types() {
    let Some((index, opt)) = find(|opt| opt.default.is_some() && !matches!(opt.kind, Kind::Flag))
    else {
        panic!("no command in the description states a default");
    };

    let spec = &COMMANDS[index].opts[opt];
    let default = spec.default.expect("found by matching on a default");

    let mut harness = Harness::new();
    harness.select(index);

    let line = harness.line();
    println!("{line}");
    assert!(
        line.contains(&format!("--{} {default}", spec.long)),
        "a prefilled default must show on the line untouched: {line}"
    );
}

#[test]
fn a_flag_a_choice_and_a_default_build_one_whole_line() {
    // The three properties on one command where the description has one, and
    // separately where it does not - which is itself worth printing, because
    // it says how much of the CLI a single pane can show off.
    let together = COMMANDS.iter().enumerate().find(|(_, command)| {
        command
            .opts
            .iter()
            .any(|opt| matches!(opt.kind, Kind::Flag))
            && command
                .opts
                .iter()
                .any(|opt| matches!(opt.kind, Kind::Choice(_)))
            && command.opts.iter().any(|opt| opt.default.is_some())
    });

    let Some((index, command)) = together else {
        println!("no single command has a flag, a choice and a default");
        return;
    };

    let mut harness = Harness::new();
    harness.select(index);

    let mut expected = vec!["onerom".to_owned()];
    expected.extend(command.path.iter().map(|word| (*word).to_owned()));

    for (opt, spec) in command.opts.iter().enumerate() {
        let target = Target::new(Where::Command(index), opt);
        match &spec.kind {
            Kind::Flag => {
                harness.send(Message::Toggled(target, true));
                expected.push(format!("--{}", spec.long));
            }
            Kind::Choice(values) => {
                let chosen = values.first().expect("a choice offers a value");
                harness.send(Message::Edited(target, (*chosen).to_owned()));
                expected.push(format!("--{}", spec.long));
                expected.push((*chosen).to_owned());
            }
            _ => {
                if let Some(default) = spec.default {
                    expected.push(format!("--{}", spec.long));
                    expected.push(default.to_owned());
                }
            }
        }
    }

    let line = harness.line();
    println!("{line}");
    assert_eq!(line, expected.join(" "));
}

#[test]
fn run_is_disabled_until_every_required_option_is_filled() {
    // An option the description insists on and that no group speaks for, so
    // the assertion is about the option and not about its group.
    let found = COMMANDS.iter().enumerate().find_map(|(index, command)| {
        let opt = command.opts.iter().position(|opt| {
            opt.must_supply()
                && !command
                    .groups
                    .iter()
                    .any(|group| group.opts.contains(&opt.long))
        })?;
        Some((index, opt))
    });

    let Some((index, opt)) = found else {
        panic!("no command in the description insists on an ungrouped option");
    };

    let mut harness = Harness::new();
    harness.select(index);

    let spec = &COMMANDS[index].opts[opt];
    assert!(
        !harness.screen.can_run(),
        "{}: Run must be off while --{} is empty",
        name(&COMMANDS[index]),
        spec.long
    );
    assert!(
        harness
            .screen
            .missing()
            .iter()
            .any(|wanted| wanted.contains(spec.long)),
        "the pane must say which option is missing"
    );

    // Answer everything the description wants, one at a time.  The globals
    // are in the list because a required global blocks Run on every pane,
    // which is what `--vid-pid` being a `Vec<T>` does today.
    let mut required: Vec<Target> = GLOBALS
        .iter()
        .enumerate()
        .filter(|(_, opt)| opt.must_supply())
        .map(|(position, _)| Target::new(Where::Globals, position))
        .collect();

    let command = &COMMANDS[index];
    for (position, opt) in command.opts.iter().enumerate() {
        let grouped = command
            .groups
            .iter()
            .any(|group| group.opts.contains(&opt.long));
        if opt.must_supply() && !grouped {
            required.push(Target::new(Where::Command(index), position));
        }
    }
    // A required group wants one member, whichever it is.
    for group in command.groups.iter().filter(|group| group.required) {
        let first = group.opts.first().expect("a group names its options");
        let position = command
            .opts
            .iter()
            .position(|opt| opt.long == *first)
            .expect("a group names options of its own command");
        required.push(Target::new(Where::Command(index), position));
    }

    for target in &required {
        assert!(
            !harness.screen.can_run(),
            "Run must stay off until the last, missing {:?}",
            harness.screen.missing()
        );
        harness.send(Message::Edited(*target, "7".to_owned()));
    }

    assert!(
        harness.screen.can_run(),
        "{}: Run must come on once everything is answered, missing {:?}",
        name(&COMMANDS[index]),
        harness.screen.missing()
    );

    // And go off again when one is emptied.
    harness.send(Message::Edited(required[0], String::new()));
    assert!(
        !harness.screen.can_run(),
        "emptying one turns Run off again"
    );
}

#[test]
fn a_conflict_takes_the_losing_option_off_the_command_line() {
    // A command where one option says it clashes with another of its own.
    let found = COMMANDS.iter().enumerate().find_map(|(index, command)| {
        let (first, second) = command.opts.iter().enumerate().find_map(|(first, opt)| {
            let clash = opt
                .conflicts
                .iter()
                .find_map(|name| command.opts.iter().position(|other| other.long == *name))?;
            Some((first, clash))
        })?;
        Some((index, first, second))
    });

    let Some((index, first, second)) = found else {
        println!("no option in the description names a conflicting option");
        return;
    };

    let mut harness = Harness::new();
    harness.select(index);

    let command = &COMMANDS[index];
    for position in [first, second] {
        let target = Target::new(Where::Command(index), position);
        match command.opts[position].kind {
            Kind::Flag => harness.send(Message::Toggled(target, true)),
            _ => harness.send(Message::Edited(target, "1".to_owned())),
        }
    }

    let line = harness.line();
    println!("{line}");
    let (a, b) = (command.opts[first].long, command.opts[second].long);
    assert!(
        !(line.contains(&format!("--{a}")) && line.contains(&format!("--{b}"))),
        "two options that clash must not both reach the line: {line}"
    );

    // And the pane says which one lost.
    let form = harness
        .screen
        .form()
        .expect("a selected command has a form");
    assert!(
        form.blocked(first).is_some() || form.blocked(second).is_some(),
        "one of the two must be blocked"
    );
}

#[test]
fn a_group_is_drawn_as_one_block() {
    let grouped = COMMANDS
        .iter()
        .find(|command| !command.groups.is_empty())
        .map(|command| (command, command.groups));

    let Some((command, groups)) = grouped else {
        println!("no command in the description declares a group");
        return;
    };

    for group in groups {
        assert!(
            !group.opts.is_empty(),
            "{}: a group with no options cannot be drawn",
            name(command)
        );
        for long in group.opts {
            assert!(
                command.opts.iter().any(|opt| opt.long == *long),
                "{}: group {} names --{long}, which is not an option of it",
                name(command),
                group.name
            );
        }
    }
    println!("{}: {} group(s)", name(command), groups.len());
}

#[test]
fn the_filter_finds_a_command_by_an_option_alias() {
    let found = COMMANDS.iter().enumerate().find_map(|(index, command)| {
        let alias = command
            .opts
            .iter()
            .find_map(|opt| opt.aliases.first().copied())?;
        Some((index, alias))
    });

    let Some((index, alias)) = found else {
        panic!("no option in the description carries an alias");
    };

    let mut harness = Harness::new();
    harness.send(Message::Filter(alias.to_owned()));

    println!(
        "{alias} matches {} of {}",
        harness.screen.matches.len(),
        COMMANDS.len()
    );
    assert!(
        harness.screen.matches.contains(&index),
        "searching for the alias {alias} must find {}",
        name(&COMMANDS[index])
    );
    assert!(
        harness.screen.matches.len() < COMMANDS.len() || COMMANDS.len() == 1,
        "a filter that matches everything has narrowed nothing"
    );

    // And the tabs narrow with it.
    let path = harness
        .screen
        .command()
        .map(|command| command.path)
        .expect("a match leaves something selected");
    let levels = studiov2_generated::tree::levels(&harness.screen.matches, path);
    let offered: usize = levels.iter().map(|level| level.segments.len()).sum();
    let all = studiov2_generated::tree::levels(&(0..COMMANDS.len()).collect::<Vec<_>>(), path);
    let unfiltered: usize = all.iter().map(|level| level.segments.len()).sum();
    assert!(
        offered <= unfiltered,
        "the tabs must not grow when the filter narrows"
    );
}

#[test]
fn a_number_box_keeps_only_digits() {
    let Some((index, opt)) = find(|opt| matches!(opt.kind, Kind::Number)) else {
        panic!("no command in the description takes a number");
    };

    let mut harness = Harness::new();
    harness.select(index);
    harness.send(Message::Edited(
        Target::new(Where::Command(index), opt),
        "12ms; rm -rf /".to_owned(),
    ));

    let line = harness.line();
    println!("{line}");
    let long = COMMANDS[index].opts[opt].long;
    assert!(
        line.contains(&format!("--{long} 12")),
        "only the digits should survive: {line}"
    );
    assert!(!line.contains("rm"), "nothing else should: {line}");
}

#[test]
fn a_repeating_option_emits_one_flag_per_value() {
    let Some((index, opt)) = find(|opt| opt.multiple && !matches!(opt.kind, Kind::Flag)) else {
        println!("no option in the description repeats");
        return;
    };

    let mut harness = Harness::new();
    harness.select(index);

    let target = Target::new(Where::Command(index), opt);
    harness.send(Message::Added(target));
    harness.send(Message::Edited(target.at(0), "one".to_owned()));
    harness.send(Message::Added(target));
    harness.send(Message::Edited(target.at(1), "two".to_owned()));

    let long = COMMANDS[index].opts[opt].long;
    let line = harness.line();
    println!("{line}");
    assert_eq!(
        line.matches(&format!("--{long} ")).count(),
        2,
        "two values means the option twice: {line}"
    );

    harness.send(Message::Removed(target.at(0)));
    let line = harness.line();
    assert_eq!(
        line.matches(&format!("--{long} ")).count(),
        1,
        "removing one entry drops one: {line}"
    );
}

#[test]
fn globals_lead_the_command_line_and_are_not_on_the_pane() {
    let Some((opt, spec)) = GLOBALS
        .iter()
        .enumerate()
        .find(|(_, opt)| !matches!(opt.kind, Kind::Flag))
    else {
        println!("the description has no global taking a value");
        return;
    };

    let mut harness = Harness::new();
    harness.send(Message::Edited(
        Target::new(Where::Globals, opt),
        "value".to_owned(),
    ));

    let line = harness.line();
    println!("{line}");
    let expected = format!("onerom --{} value ", spec.long);
    assert!(
        line.starts_with(&expected),
        "a global belongs before the path: {line}"
    );

    // The same global must not be duplicated by the command's own options.
    assert_eq!(
        line.matches(&format!("--{}", spec.long)).count(),
        1,
        "a global appears once: {line}"
    );
}

#[test]
fn an_optional_value_can_be_returned_to_unset() {
    let Some((index, opt)) = find(|opt| opt.optional && !matches!(opt.kind, Kind::Flag)) else {
        panic!("no command in the description has an optional value");
    };

    let mut harness = Harness::new();
    harness.select(index);

    let target = Target::new(Where::Command(index), opt);
    harness.send(Message::Edited(target, "9".to_owned()));
    let long = COMMANDS[index].opts[opt].long;
    assert!(harness.line().contains(&format!("--{long}")));

    harness.send(Message::Cleared(target));
    let line = harness.line();
    println!("{line}");
    assert!(
        !line.contains(&format!("--{long}")),
        "unsetting must take it off the line: {line}"
    );
}

#[test]
fn a_result_belongs_to_the_command_that_produced_it() {
    if COMMANDS.len() < 2 {
        println!("one command: nothing to switch between");
        return;
    }

    let mut harness = Harness::new();
    harness.select(0);
    let outcome = stub::run(&COMMANDS[0], &harness.line(), false);
    harness.send(Message::Ran(outcome));
    assert!(harness.screen.shown_result().is_some());

    harness.select(1);
    assert!(
        harness.screen.shown_result().is_none(),
        "another command's answer must not appear under this title"
    );
}

#[test]
fn the_forced_error_path_reaches_the_pane_and_the_log() {
    let mut harness = Harness::new();
    harness.select(0);
    harness.send(Message::ForceError(true));

    let outcome = stub::run(&COMMANDS[0], &harness.line(), true);
    let before = harness.shared.log.len();
    harness.send(Message::Ran(outcome));

    match harness.screen.shown_result() {
        Some(Err(error)) => println!("{error}"),
        other => panic!("the pane should show an error, got {}", other.is_some()),
    }
    assert!(
        harness.shared.log.len() > before,
        "an error must reach the shared log as well as the pane"
    );
}

#[test]
fn a_requirement_reaches_the_run_button() {
    // A command where giving one option makes another necessary.
    let found = COMMANDS.iter().enumerate().find_map(|(index, command)| {
        let opt = command
            .opts
            .iter()
            .position(|opt| !opt.requires.is_empty() && !opt.must_supply())?;
        Some((index, opt))
    });

    let Some((index, opt)) = found else {
        println!("no optional option in the description requires another");
        return;
    };

    let mut harness = Harness::new();
    harness.select(index);

    let spec = &COMMANDS[index].opts[opt];
    let before = harness.screen.missing();

    let target = Target::new(Where::Command(index), opt);
    match spec.kind {
        Kind::Flag => harness.send(Message::Toggled(target, true)),
        _ => harness.send(Message::Edited(target, "1".to_owned())),
    }

    let after = harness.screen.missing();
    println!("--{}: {before:?} -> {after:?}", spec.long);
    assert!(
        after.len() > before.len(),
        "giving --{} must ask for what it needs: {after:?}",
        spec.long
    );
    assert!(
        after.iter().any(|wanted| wanted.contains(spec.long)),
        "and must say which option asked: {after:?}"
    );
}

#[test]
fn a_pane_heading_is_words_and_not_a_command_line() {
    assert_eq!(
        ui::title(&["control", "rgb", "on"]),
        "Control \u{2192} RGB \u{2192} On"
    );
    assert_eq!(
        ui::title(&["image", "swap-bytes"]),
        "Image \u{2192} Swap Bytes"
    );

    for command in COMMANDS {
        let heading = ui::title(command.path);
        assert!(
            !heading.contains("onerom"),
            "{heading}: a heading is not a command line"
        );
        assert!(
            !heading.contains('-') || heading.contains("\u{2192}"),
            "{heading}: a separator survived into a heading"
        );
        for word in heading.split(" \u{2192} ") {
            let first = word.chars().next().expect("a heading word is not empty");
            assert!(first.is_uppercase(), "{heading}: {word} should start upper");
        }
    }
}

#[test]
fn each_row_of_tabs_is_keyed_by_the_word_above_it() {
    let path = ["control", "rgb", "on"];

    assert_eq!(ui::tab_key(&path, 0), "Command:");
    assert_eq!(ui::tab_key(&path, 1), "Control:");
    assert_eq!(ui::tab_key(&path, 2), "RGB:");

    // A row offering to drill past the selected leaf has no word above it in
    // the path, and falls back rather than panicking.
    assert_eq!(ui::tab_key(&["scan"], 5), "Command:");

    // Every command's own rows are keyed all the way down.
    for command in COMMANDS {
        for depth in 0..command.path.len() {
            let key = ui::tab_key(command.path, depth);
            assert!(key.ends_with(':'), "{key}: a key ends with a colon");
            assert!(!key.contains('-'), "{key}: a separator survived");
        }
    }
}

#[test]
fn an_option_label_is_a_name_and_not_a_flag() {
    assert_eq!(name::label("colour"), "Colour");
    assert_eq!(name::label("base-firmware"), "Base firmware");
    assert_eq!(name::label("vid-pid"), "VID PID");

    let all = COMMANDS
        .iter()
        .flat_map(|command| command.opts.iter())
        .chain(GLOBALS.iter());

    for opt in all {
        let caption = name::label(opt.long);
        assert!(
            !caption.starts_with('-'),
            "--{}: a label carries no dashes, got {caption}",
            opt.long
        );
        assert!(
            !caption.contains('-') && !caption.contains('_'),
            "--{}: a separator survived into {caption}",
            opt.long
        );
        let first = caption.chars().next().expect("a label is not empty");
        assert!(
            first.is_uppercase(),
            "--{}: a label starts upper, got {caption}",
            opt.long
        );
    }
}

#[test]
fn one_roms_own_words_stay_upper_wherever_they_land() {
    // The acronym list is the description's, and the pane must not carry a
    // second one.  These go through the same two functions the pane calls.
    for word in name::ACRONYMS {
        let lower = word.to_lowercase();
        assert_eq!(&name::heading(&lower), word, "heading({lower})");
        assert_eq!(&name::label(&lower), word, "label({lower})");

        // And in the middle of a phrase, where the pane usually meets them.
        let phrase = format!("show-{lower}-state");
        assert!(
            name::heading(&phrase).contains(*word),
            "heading({phrase}) lost {word}"
        );
    }

    assert_eq!(name::heading("gpios"), "GPIOs");
    assert_eq!(name::label("list-gpios"), "List GPIOs");
}

#[test]
fn the_copy_button_carries_the_line_the_pane_shows() {
    // The button and the CLI line must not drift: one asks for the other's
    // text, and this is what says so.
    let mut shared = Shared::stub().expect("a log store");
    let mut screen = Screen::new();
    assert!(screen.select_path(&["control", "rgb", "on"]));

    let shown = screen.command_line();
    assert!(shown.contains("control rgb on"), "{shown}");

    // Copying is a task, so what is asserted is that asking for it does not
    // disturb the pane and the line is still the one on screen.
    let _ = screen.update(Message::CopyCli, &mut shared);
    assert_eq!(screen.command_line(), shown);
}

#[test]
fn a_help_line_names_an_option_the_way_its_control_is_labelled() {
    // One pane, one vocabulary: a control labelled `All` must not be called
    // `--all` two lines below it.
    //
    // Only the sentences this screen writes are checked.  The CLI's own help
    // prose names options with dashes - `--length` reads "paired with --offset
    // or --address" - and so does the alias sentence, deliberately, because
    // that one is telling a user what they may type.
    let mut screen = Screen::new();
    assert!(screen.select_path(&["control", "erase"]));
    let here = Where::Command(screen.selected.expect("a selection"));

    let form = screen.form().expect("a form");
    let length = screen.opt_index(here, "length").expect("--length");

    let line = ui::annotation(form, length, None);
    assert!(line.contains("Not with All."), "{line}");
    assert!(!line.contains("Not with --"), "{line}");

    let asked = ui::annotation(form, length, Some("offset"));
    assert!(asked.contains("because Offset is set"), "{asked}");
    assert!(!asked.contains("because --"), "{asked}");

    let all = screen.opt_index(here, "all").expect("--all");
    if let Some(reason) = form.blocked(all) {
        assert!(!reason.contains("--"), "{reason}");
    }
}

#[test]
fn a_help_line_carries_no_rust_type_and_no_repeated_default() {
    // `RgbColour` means nothing to a user, and the box above already shows
    // `red`, so saying it again is noise.
    let mut screen = Screen::new();
    assert!(screen.select_path(&["control", "rgb", "on"]));
    let here = Where::Command(screen.selected.expect("a selection"));

    let form = screen.form().expect("a form");
    let colour = screen.opt_index(here, "colour").expect("--colour");
    let line = ui::annotation(form, colour, None);

    assert!(!line.contains("RgbColour"), "{line}");
    assert!(!line.contains("Default"), "{line}");
    assert!(
        line.contains("Also --color"),
        "an alias lost its dashes: {line}"
    );
}

// ------------------------------------------------------- where values come --

/// Every option of every command, with the command it belongs to.
fn all_opts() -> impl Iterator<Item = (&'static Command, &'static Opt)> {
    COMMANDS
        .iter()
        .flat_map(|command| command.opts.iter().map(move |opt| (command, opt)))
}

#[test]
fn every_source_offers_something_a_pane_can_draw() {
    // The tally is the answer to "how much of the CLI can be offered rather
    // than typed", so it is printed rather than only asserted.
    let harness = Harness::new();
    let context = Context::new(&harness.shared);

    let mut picked = 0;
    let mut browsed = 0;
    let mut free = 0;

    for (command, opt) in all_opts().chain(GLOBALS.iter().map(|opt| (&COMMANDS[0], opt))) {
        match resolve::offer(opt.source, &context) {
            Offer::Pick { choices, .. } => {
                assert!(
                    !choices.is_empty(),
                    "{} --{}: a pick list with nothing on it is a dead control",
                    name(command),
                    opt.long
                );
                let mut sorted = choices.clone();
                sorted.sort();
                sorted.dedup();
                assert_eq!(
                    sorted.len(),
                    choices.len(),
                    "{} --{}: a value offered twice",
                    name(command),
                    opt.long
                );
                picked += 1;
            }
            Offer::Browse(_) => browsed += 1,
            Offer::Free => free += 1,
        }
    }

    println!("{picked} pick lists, {browsed} browse rows, {free} other controls");

    // And the number the README quotes: panes with nothing left to type blind.
    // A flag, a number and a fixed set were always real controls, so what is
    // counted is a pane with no bare box on it.
    let whole = COMMANDS
        .iter()
        .filter(|command| {
            command.opts.iter().all(|opt| {
                !matches!(
                    (resolve::offer(opt.source, &context), &opt.kind),
                    (Offer::Free, Kind::Text | Kind::Domain(_))
                )
            })
        })
        .count();
    println!("{whole} of {} panes have no bare text box", COMMANDS.len());

    // Naming the rest, because "four panes still have one" is a number nobody
    // can act on and a list of options is.
    for command in COMMANDS {
        let left: Vec<&str> = command
            .opts
            .iter()
            .filter(|opt| {
                matches!(
                    (resolve::offer(opt.source, &context), &opt.kind),
                    (Offer::Free, Kind::Text | Kind::Domain(_))
                )
            })
            .map(|opt| opt.long)
            .collect();
        if !left.is_empty() {
            println!("  {}: {}", name(command), left.join(", "));
        }
    }

    assert!(picked > 0 && browsed > 0, "the sources reach no control");
}

#[test]
fn a_board_option_offers_fire_boards_and_nothing_else() {
    // Studio v2 is Fire and USB.  An Ice board on the list would be a board it
    // could never talk to.
    let harness = Harness::new();
    let context = Context::new(&harness.shared);

    let fire: Vec<&str> = Model::Fire.boards().iter().map(Board::name).collect();
    let ice: Vec<&str> = Model::Ice.boards().iter().map(Board::name).collect();
    assert!(!ice.is_empty(), "the tree has Ice boards to leave out");

    let mut seen = 0;
    for (command, opt) in all_opts() {
        if opt.source != Some(Source::Board) {
            continue;
        }
        seen += 1;

        let Offer::Pick { choices, open } = resolve::offer(opt.source, &context) else {
            panic!("{} --{}: a board is a list", name(command), opt.long);
        };
        assert!(!open, "every board this build knows is on the list");
        assert_eq!(choices, fire, "{} --{}", name(command), opt.long);
        for board in &ice {
            assert!(
                !choices.iter().any(|choice| choice == board),
                "{} --{}: {board} is an Ice board",
                name(command),
                opt.long
            );
        }
    }

    println!("{seen} board options, {} boards each", fire.len());
    assert!(seen > 0, "the description names no board option");
}

#[test]
fn a_file_option_gets_a_browse_button() {
    let mut screen = Screen::new();
    assert!(screen.select_path(&["image", "convert"]));
    let here = Where::Command(screen.selected.expect("a selection"));
    let input = screen.opt_index(here, "input").expect("--input");

    let opt = &screen.form().expect("a form").opts[input];
    let harness = Harness::new();
    let offer = resolve::offer(opt.source, &Context::new(&harness.shared));
    assert_eq!(
        offer,
        Offer::Browse(Browse::Open),
        "a file that has to exist is browsed for, not typed"
    );

    // What the button sends, sent the way the button sends it.
    let mut shared = Shared::stub().expect("a log store");
    let _ = screen.update(
        Message::Edited(Target::new(here, input), resolve::browse_path(Browse::Open)),
        &mut shared,
    );

    let line = screen.command_line();
    println!("{line}");
    assert!(line.contains("--input /"), "an absolute path: {line}");
}

#[test]
fn a_colour_option_offers_the_names_and_still_takes_hex() {
    let mut screen = Screen::new();
    assert!(screen.select_path(&["control", "rgb", "on"]));
    let here = Where::Command(screen.selected.expect("a selection"));
    let colour = screen.opt_index(here, "colour").expect("--colour");

    let opt = &screen.form().expect("a form").opts[colour];
    let harness = Harness::new();
    let Offer::Pick { choices, open } = resolve::offer(opt.source, &Context::new(&harness.shared))
    else {
        panic!("a colour is a list");
    };

    println!("{}", choices.join(", "));
    let named: Vec<&str> = RgbColour::names().collect();
    assert_eq!(choices, named, "the CLI's own colour words, in its order");
    assert!(
        open,
        "a colour is also a hex value, so the list is not the whole set"
    );

    // The swatch reads the value rather than the choice, so a hex colour typed
    // by hand shows its colour too.
    assert_eq!(resolve::swatch(opt.source, "red"), Some((0xFF, 0x00, 0x00)));
    assert_eq!(
        resolve::swatch(opt.source, "#FF8000"),
        Some((0xFF, 0x80, 0x00))
    );
    assert_eq!(resolve::swatch(opt.source, "banana"), None);
    assert_eq!(resolve::swatch(opt.source, ""), None);

    // And an option that is not a colour never gets one.
    let brightness = screen.opt_index(here, "brightness").expect("--brightness");
    let other = &screen.form().expect("a form").opts[brightness];
    assert_eq!(resolve::swatch(other.source, "red"), None);
}

#[test]
fn a_pin_is_a_list_when_a_board_is_known_and_a_box_when_it_is_not() {
    // Which pads exist is a fact about the board, so with no device the pane
    // has nothing to offer and says so by handing back the text box.
    let Some((_, opt)) = all_opts().find(|(_, opt)| opt.source == Some(Source::Pin)) else {
        panic!("the description names no pin option");
    };

    let mut shared = Shared::stub().expect("a log store");
    let with = Context::new(&shared);
    let Offer::Pick { choices, open } = resolve::offer(opt.source, &with) else {
        panic!("a pin on a known board is a list");
    };
    println!(
        "{}: {}",
        shared.device.as_ref().expect("the stub selects one").board,
        choices.join(", ")
    );
    assert!(!choices.is_empty(), "a board has pads");
    assert!(open, "gpio<N> is legal for any N the device has");
    assert!(
        choices.iter().all(|pad| parse_pin(pad).is_ok()),
        "every pad offered must be one --pin takes: {choices:?}"
    );

    shared.device = None;
    assert_eq!(
        resolve::offer(opt.source, &Context::new(&shared)),
        Offer::Free,
        "with no board there is no list to offer"
    );
}

#[test]
fn a_chip_type_option_offers_chips_and_not_plugin_slots() {
    let Some((_, opt)) = all_opts().find(|(_, opt)| opt.source == Some(Source::ChipType)) else {
        panic!("the description names no chip-type option");
    };

    let harness = Harness::new();
    let Offer::Pick { choices, open } = resolve::offer(opt.source, &Context::new(&harness.shared))
    else {
        panic!("a chip type is a list");
    };

    println!("{} chip types", choices.len());
    assert!(!open, "every chip type this build knows is on the list");
    for chip in CHIP_TYPES.iter().filter(|chip| !chip.is_plugin()) {
        assert!(
            choices.iter().any(|choice| choice == chip.name()),
            "{} is missing from the list",
            chip.name()
        );
    }
    for chip in CHIP_TYPES.iter().filter(|chip| chip.is_plugin()) {
        assert!(
            !choices.iter().any(|choice| choice == chip.name()),
            "{} is a plugin slot, not a chip a user picks",
            chip.name()
        );
    }
}

#[test]
fn every_pane_draws_with_no_device_selected() {
    // The other pane test runs with the stub's device selected.  Half the pin
    // controls change shape without one, and that path has to draw too.
    let mut harness = Harness::new();
    harness.shared.device = None;

    for index in 0..COMMANDS.len() {
        harness.select(index);
        drop(harness.screen.view(&harness.shared));
    }
}

// --------------------------------------------------------- what runs for real --

/// A scratch directory, removed when the test that made it is done.
///
/// A real run writes files, and every one of them has to land somewhere
/// nobody's tree notices.
struct Scratch {
    /// The directory itself.
    path: std::path::PathBuf,
}

impl Scratch {
    /// A directory of this test binary's own, under the system temporary one.
    fn new(what: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "onerom-generated-{what}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self { path }
    }

    /// A path inside it, which need not exist.
    fn at(&self, name: &str) -> String {
        self.path.join(name).display().to_string()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The command a runner covers, found by the options that runner reads.
///
/// Naming options is not naming a command: this asks the description for
/// whichever command takes exactly these, and fails the test if the CLI has
/// moved them somewhere else.
fn real_taking(names: &[&str]) -> usize {
    let found = COMMANDS.iter().position(|command| {
        real::runner(command).is_some()
            && command.opts.len() == names.len()
            && names
                .iter()
                .all(|name| command.opts.iter().any(|opt| opt.long == *name))
    });

    found.unwrap_or_else(|| panic!("no command runs for real taking {names:?}"))
}

/// Fills one option of the selected command, by name.
impl Harness {
    fn fill(&mut self, name: &str, value: &str) {
        let index = self.screen.selected.expect("a command on show");
        let form = Where::Command(index);
        let opt = self
            .screen
            .opt_index(form, name)
            .unwrap_or_else(|| panic!("no --{name} on the command on show"));
        self.send(Message::Edited(Target::new(form, opt), value.to_owned()));
    }

    /// Runs the selected command for real and puts the answer on the pane.
    ///
    /// The task `Message::Run` hands back cannot be looked into, so this drives
    /// the same two halves the update path does — which is how every other
    /// test here handles Run.
    fn run_real(&mut self) {
        let result = self
            .screen
            .real_run()
            .expect("a command that runs for real");
        self.send(Message::Ran(result));
    }

    /// The answer on the pane, as a line of text.
    fn line_result(&self) -> String {
        match self.screen.shown_result() {
            Some(Ok(stub::Output::Line(line))) => line.clone(),
            other => panic!("expected one line of output, got {other:?}"),
        }
    }

    /// The error on the pane.
    fn error(&self) -> String {
        match self.screen.shown_result() {
            Some(Err(error)) => error.clone(),
            other => panic!("expected a failure, got {other:?}"),
        }
    }
}

#[test]
fn a_real_conversion_round_trips_an_image() {
    let scratch = Scratch::new("convert");
    let source: Vec<u8> = (0..512u16).map(|byte| byte as u8).collect();
    let binary = scratch.at("rom.bin");
    let hex = scratch.at("rom.hex");
    let back = scratch.at("back.bin");
    std::fs::write(&binary, &source).expect("a starting image");

    let index = real_taking(&["from", "to", "input", "output", "load-address"]);
    let mut harness = Harness::new();
    harness.select(index);

    harness.fill("from", "binary");
    harness.fill("to", "ihex");
    harness.fill("input", &binary);
    harness.fill("output", &hex);
    harness.run_real();
    println!("{}", harness.line_result());

    // The answer is the library's, so the file it names has to be one the
    // library can read back.
    let encoded = std::fs::read_to_string(&hex).expect("the converted image");
    assert!(
        encoded.starts_with(':'),
        "an Intel HEX file starts with a record: {}",
        &encoded[..encoded.len().min(40)]
    );

    harness.fill("from", "ihex");
    harness.fill("to", "binary");
    harness.fill("input", &hex);
    harness.fill("output", &back);
    harness.run_real();
    println!("{}", harness.line_result());

    assert_eq!(
        std::fs::read(&back).expect("the image converted back"),
        source,
        "a round trip through ihex must give the bytes back"
    );
}

#[test]
fn a_missing_file_fails_in_the_librarys_own_words() {
    let scratch = Scratch::new("missing");
    let missing = scratch.at("nothing-here.bin");

    let index = real_taking(&["from", "to", "input", "output", "load-address"]);
    let mut harness = Harness::new();
    harness.select(index);
    harness.fill("from", "binary");
    harness.fill("to", "ihex");
    harness.fill("input", &missing);
    harness.fill("output", &scratch.at("out.hex"));
    harness.run_real();

    // Built the same way the CLI builds it, from the same failed read, so this
    // says the pane shows the library's error rather than one shaped like it.
    let wanted = onerom_cli::Error::io(
        &missing,
        std::fs::read(&missing).expect_err("the file must not exist"),
    )
    .to_string();

    assert_eq!(harness.error(), wanted);
    println!("{wanted}");
}

#[test]
fn a_transform_the_library_refuses_reaches_the_pane() {
    let scratch = Scratch::new("refused");
    let odd = scratch.at("odd.bin");
    std::fs::write(&odd, [0u8; 7]).expect("an odd-length image");

    let mut harness = Harness::new();

    // An odd-length image cannot have its byte pairs swapped.
    harness.select(real_taking(&["input", "output"]));
    harness.fill("input", &odd);
    harness.fill("output", &scratch.at("swapped.bin"));
    harness.run_real();
    let swap = harness.error();
    assert!(swap.contains(&odd) && swap.contains('7'), "{swap}");
    println!("{swap}");

    // And a stride that does not divide the image leaves a ragged tail.
    let ragged = scratch.at("ragged.bin");
    std::fs::write(&ragged, [0u8; 9]).expect("an image of nine bytes");
    harness.select(real_taking(&[
        "input", "output", "offset", "stride", "bytes",
    ]));
    harness.fill("input", &ragged);
    harness.fill("output", &scratch.at("lane.bin"));
    harness.fill("offset", "0");
    harness.fill("stride", "2");
    harness.run_real();
    let stride = harness.error();
    assert!(stride.contains("multiple of 2"), "{stride}");
    println!("{stride}");
}

#[test]
fn a_real_table_carries_the_boards_own_chips() {
    let board = Model::Fire.boards().first().copied().expect("a Fire board");
    let expected = onerom_gen::compat::supported_chips(board, onerom_gen::ChipSetType::Single, 1);

    let mut harness = Harness::new();
    harness.select(real_taking(&["board", "all", "chip-type"]));
    harness.fill("board", board.name());
    harness.run_real();

    match harness.screen.shown_result() {
        Some(Ok(stub::Output::Table { headers, body })) => {
            let rows = body.rows();
            assert_eq!(
                rows.len(),
                expected.len(),
                "{}: the pane must show every chip the compatibility pass found",
                board.name()
            );
            for (row, entry) in rows.iter().zip(&expected) {
                assert_eq!(row.len(), headers.len());
                assert_eq!(row[0], entry.alias);
            }
            println!("{} chips on {}", rows.len(), board.name());
        }
        other => panic!("expected a table, got {other:?}"),
    }
}

/// A board whose chips do not all sit in its socket the same way.
///
/// A board with one fit class has one section, which cannot show that the
/// rows survived being split up.
fn board_with_several_fits() -> Board {
    let fits = |board: &Board| {
        let mut offsets: Vec<i16> =
            onerom_gen::compat::supported_chips(*board, onerom_gen::ChipSetType::Single, 1)
                .iter()
                .map(|entry| entry.result.pin_offset)
                .collect();
        offsets.dedup();
        offsets.len()
    };

    Model::Fire
        .boards()
        .iter()
        .copied()
        .max_by_key(fits)
        .filter(|board| fits(board) > 1)
        .expect("a board serving chips of more than one pin count")
}

#[test]
fn a_sectioned_table_keeps_every_row_of_every_section() {
    let board = board_with_several_fits();
    let expected = onerom_gen::compat::supported_chips(board, onerom_gen::ChipSetType::Single, 1);

    let mut harness = Harness::new();
    harness.select(real_taking(&["board", "all", "chip-type"]));
    harness.fill("board", board.name());
    harness.run_real();

    let Some(Ok(stub::Output::Table {
        body: stub::Body::Sections(sections),
        ..
    })) = harness.screen.shown_result()
    else {
        panic!(
            "expected a sectioned table, got {:?}",
            harness.screen.shown_result()
        );
    };

    assert!(
        sections.len() > 1,
        "{}: its chips fit in more than one way, so they cannot be one section",
        board.name()
    );

    // No row goes missing on the way into a section, and none moves: the
    // order is the compatibility pass's own, which is what puts the chips
    // that fit the same way together in the first place.
    let rows: Vec<&Vec<String>> = sections.iter().flat_map(|section| &section.rows).collect();
    assert_eq!(rows.len(), expected.len());
    for (row, entry) in rows.iter().zip(&expected) {
        assert_eq!(row[0], entry.alias);
    }

    // A heading has to tell its section from the next, or it is decoration.
    let headings: Vec<&String> = sections.iter().map(|section| &section.heading).collect();
    for (index, heading) in headings.iter().enumerate() {
        assert!(!heading.is_empty(), "an empty heading says nothing");
        assert!(
            !headings[..index].contains(heading),
            "{}: two sections headed {heading}",
            board.name()
        );
        assert!(
            !sections[index].rows.is_empty(),
            "{heading}: a heading over nothing"
        );
    }

    // And the pane draws it.
    drop(harness.screen.view(&harness.shared));

    let described: Vec<String> = sections
        .iter()
        .map(|section| format!("{} ({})", section.heading, section.rows.len()))
        .collect();
    println!(
        "{} on {}: {}",
        rows.len(),
        board.name(),
        described.join(", ")
    );
}

#[test]
fn a_table_with_no_sections_is_a_flat_run_of_rows() {
    let mut drawn = 0;

    for command in COMMANDS {
        let line = format!("onerom {}", name(command));
        let Ok(stub::Output::Table { headers, body }) = stub::run(command, &line, false) else {
            continue;
        };

        let stub::Body::Rows(rows) = &body else {
            panic!("{}: a stubbed table invents no sections", name(command));
        };

        assert!(!rows.is_empty(), "{}: a table of nothing", name(command));
        for row in rows {
            assert_eq!(row.len(), headers.len());
        }

        // The rows a flat table hands the renderer are the rows it holds, in
        // the order it holds them - which is what makes it draw as it did
        // before sections existed.
        assert_eq!(body.rows(), rows.iter().collect::<Vec<_>>());
        drawn += 1;
    }

    assert!(drawn > 0, "something has to guess at a table");
    println!("{drawn} stubbed tables, none of them sectioned");
}

#[test]
fn a_real_command_is_the_only_one_with_its_options() {
    let same = |left: &Command, right: &Command| {
        left.opts.len() == right.opts.len()
            && left
                .opts
                .iter()
                .all(|opt| right.opts.iter().any(|other| other.long == opt.long))
    };

    let mut real = 0;
    for command in COMMANDS.iter().filter(|c| real::runner(c).is_some()) {
        real += 1;
        let twins = COMMANDS.iter().filter(|other| same(other, command)).count();
        assert_eq!(
            twins,
            1,
            "{}: a contract that fits two commands must run for neither",
            name(command)
        );
    }
    assert!(real > 0, "something has to run for real");
    println!(
        "{real} commands run for real, {} stubbed",
        COMMANDS.len() - real
    );
}

#[test]
fn a_command_that_takes_nothing_can_never_run_for_real() {
    // The finding, held down: a contract is a statement about options, and a
    // command with none says nothing a contract could match.  `board list` is
    // one of these, which is why it is stubbed alongside the rest.
    let bare: Vec<&Command> = COMMANDS
        .iter()
        .filter(|command| command.opts.is_empty())
        .collect();

    assert!(
        bare.len() > 1,
        "the ambiguity has to be real for the rule to be doing anything"
    );
    for command in &bare {
        assert!(
            real::runner(command).is_none(),
            "{}: nothing separates it from the others that take no options",
            name(command)
        );
    }

    let names: Vec<String> = bare.iter().map(|command| name(command)).collect();
    println!("indistinguishable by what they take: {}", names.join(", "));
}
