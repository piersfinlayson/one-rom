// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! A screenshot hook, so all of the panes can be reviewed without a human
//! clicking through them.
//!
//! Set `ONEROM_PROTO_SHOT` to a path and the app applies whatever
//! `ONEROM_PROTO_SETUP` describes, writes a PNG of its window, and quits.
//! Unset, none of the variables does anything and the app runs normally.
//!
//! The setup script is comma-separated steps, so a value cannot contain a
//! comma.  Everything after the first colon of a step is the value, so a path
//! or a URL can.
//!
//! | Step | What it does |
//! | --- | --- |
//! | `cmd:control rgb on` | select a command by path, `/` or space separated |
//! | `filter:led` | type into the filter box |
//! | `set:colour:red` | fill one option of the selected command |
//! | `add:slot` | add another entry to a repeating option |
//! | `global:serial:ORFA-0027-3F1C` | fill one global option |
//! | `fill` | fill every option the command insists on |
//! | `long` | open the details disclosure |
//! | `globals` | open the global options section |
//! | `fail` | make Run take the error path |
//! | `run` | press Run, through the real update path |
//! | `bottom` | scroll the page to the end, so a result is in shot |
//! | `table:240` | scroll a table result that far down its own region |
//!
//! A `cmd:` step naming nothing exits rather than photographing the wrong
//! pane, because a batch run of every command has no other way to notice.

use std::path::PathBuf;

use iced::window;
use studiov2_commands::{Kind, Opt};
use studiov2_shared::Shared;

use crate::form::Where;
use crate::screen::{Message, Screen};

/// The window size a screenshot run asks for, as `WIDTHxHEIGHT`.
pub fn window_size() -> iced::Size {
    let default = iced::Size::new(1040.0, 900.0);
    let Ok(text) = std::env::var("ONEROM_PROTO_SIZE") else {
        return default;
    };
    match text.split_once('x') {
        Some((width, height)) => match (width.parse(), height.parse()) {
            (Ok(width), Ok(height)) => iced::Size::new(width, height),
            _ => default,
        },
        None => default,
    }
}

/// Where a screenshot should be written, if anywhere.
pub fn shot_path() -> Option<PathBuf> {
    std::env::var_os("ONEROM_PROTO_SHOT").map(PathBuf::from)
}

/// Applies `ONEROM_PROTO_SETUP` to a fresh screen.
pub fn apply_setup(app: &mut Screen, _shared: &mut Shared) {
    let Ok(script) = std::env::var("ONEROM_PROTO_SETUP") else {
        return;
    };

    for step in script.split(',').filter(|step| !step.trim().is_empty()) {
        apply_step(app, step.trim());
    }

    if let Some(command) = app.command() {
        println!("selected: {}", command.path.join(" "));
    }
    println!("command line: {}", app.command_line());
}

/// One step of the setup script.
fn apply_step(app: &mut Screen, step: &str) {
    // Handled by `capture`, not here: one has to go through `update` and the
    // rest are widget operations.
    if step == "run" || step == "bottom" || step.starts_with("table:") {
        return;
    }

    if step == "long" {
        app.long_open = true;
        return;
    }
    if step == "globals" {
        app.globals_open = true;
        return;
    }
    if step == "fail" {
        app.force_error = true;
        return;
    }
    if step == "fill" {
        fill(app);
        return;
    }

    if let Some(path) = step.strip_prefix("cmd:") {
        let words: Vec<&str> = path
            .split(['/', ' '])
            .filter(|word| !word.is_empty())
            .collect();
        if !app.select_path(&words) {
            eprintln!("no command matches: {}", words.join(" "));
            std::process::exit(2);
        }
        return;
    }

    if let Some(text) = step.strip_prefix("filter:") {
        app.filter = text.to_owned();
        let matches = crate::tree::matching(&app.filter);
        if !app.selected.is_some_and(|index| matches.contains(&index)) {
            app.selected = matches.first().copied();
        }
        app.matches = matches;
        return;
    }

    if let Some(rest) = step.strip_prefix("global:") {
        set_named(app, Where::Globals, rest);
        return;
    }

    if let Some(rest) = step.strip_prefix("set:") {
        let Some(index) = app.selected else {
            eprintln!("set with no command selected: {rest}");
            return;
        };
        set_named(app, Where::Command(index), rest);
        return;
    }

    if let Some(name) = step.strip_prefix("add:") {
        let Some(index) = app.selected else {
            return;
        };
        let form = Where::Command(index);
        if let Some(opt) = app.opt_index(form, name) {
            app.forms[index].add(opt);
        } else {
            eprintln!("no option --{name} on the selected command");
        }
        return;
    }

    eprintln!("unrecognised setup step: {step}");
}

/// Applies `name:value` to a form.
fn set_named(app: &mut Screen, form: Where, rest: &str) {
    let Some((name, value)) = rest.split_once(':') else {
        eprintln!("a set step is name:value, got: {rest}");
        return;
    };

    let Some(opt) = app.opt_index(form, name) else {
        eprintln!("no option --{name} to set");
        return;
    };

    let value = value.to_owned();
    match form {
        Where::Globals => write(&mut app.globals, opt, value),
        Where::Command(index) => write(&mut app.forms[index], opt, value),
    }
}

/// Writes a value into a form, ticking it where the option is a flag.
fn write(form: &mut crate::form::Form, opt: usize, value: String) {
    if matches!(form.opts[opt].kind, Kind::Flag) {
        form.toggle(opt, value != "false" && value != "0");
    } else {
        form.set(opt, 0, value);
    }
}

/// Fills every option the description insists on, globals included.
///
/// What a review of every pane wants is each one as it looks when a user has
/// answered it, and typing that by hand for each is what this exists to avoid.
///
/// The globals are in scope because one of them is required, which means Run
/// is off on every pane until it is answered — see the crate's report.
fn fill(app: &mut Screen) {
    fill_form(&mut app.globals);
    if let Some(index) = app.selected {
        fill_form(&mut app.forms[index]);
    }
}

/// Fills one form's required options with a plausible value each.
///
/// A required group wants one member and not all of them, so the first is
/// answered and the rest left alone.
fn fill_form(form: &mut crate::form::Form) {
    let grouped: Vec<&str> = form
        .groups
        .iter()
        .flat_map(|group| group.opts.iter().copied())
        .collect();

    for position in 0..form.opts.len() {
        let opt = &form.opts[position];
        if !opt.must_supply() || grouped.contains(&opt.long) {
            continue;
        }
        let value = placeholder(opt);
        write(form, position, value);
    }

    for group in form.groups.iter().filter(|group| group.required) {
        let Some(first) = group.opts.first() else {
            continue;
        };
        let Some(position) = form.opts.iter().position(|opt| opt.long == *first) else {
            continue;
        };
        let value = placeholder(&form.opts[position]);
        write(form, position, value);
    }
}

/// A plausible value for an option, from what the description says about it.
fn placeholder(opt: &Opt) -> String {
    match &opt.kind {
        Kind::Flag => "true".to_owned(),
        Kind::Choice(values) => values.first().copied().unwrap_or_default().to_owned(),
        Kind::Number => "42".to_owned(),
        Kind::Domain(name) => format!("<{}>", name.to_lowercase()),
        Kind::Text => opt
            .value_name
            .unwrap_or(opt.long)
            .to_lowercase()
            .replace('_', "-"),
    }
}

/// Waits for the first frame, grabs the window, and hands the pixels back.
///
/// A `run` step sends [`Message::Run`] first, so the screenshot shows what the
/// real update path produced rather than a result poked into place.
pub fn capture() -> iced::Task<Message> {
    let mut tasks = Vec::new();
    if asked_for("run") {
        tasks.push(iced::Task::done(Message::Run));
    }
    tasks.push(shot());
    iced::Task::batch(tasks)
}

/// Whether the setup script asks for a step that `apply_step` cannot do.
///
/// Two sorts of step have to reach the runtime rather than the state: pressing
/// Run has to go through `update`, and scrolling is a widget operation.
fn asked_for(step: &str) -> bool {
    steps().any(|given| given == step)
}

/// The value of a `name:value` step `apply_step` cannot do, as a number.
fn number_after(prefix: &str) -> Option<f32> {
    steps().find_map(|given| given.strip_prefix(prefix)?.parse().ok())
}

/// The setup script's steps, trimmed.
fn steps() -> impl Iterator<Item = String> {
    std::env::var("ONEROM_PROTO_SETUP")
        .unwrap_or_default()
        .split(',')
        .map(|step| step.trim().to_owned())
        .collect::<Vec<_>>()
        .into_iter()
}

/// The screenshot itself, once the window has had time to draw and any run has
/// had time to finish.
fn shot() -> iced::Task<Message> {
    let mut task = wait(1400);

    // A tall pane puts its result below the window, so a shot of a run wants
    // the page scrolled to the end.  After the first wait, because the page is
    // a different height once a result is on it, and with a wait of its own
    // after, because a scroll changes widget state and the frame that shows it
    // has not been drawn yet.
    if asked_for("bottom") {
        task = task
            .chain(iced::widget::operation::snap_to_end(crate::ui::scroll_id()))
            // A scroll moves widget state and nothing else, so without a
            // message behind it the window is photographed as it was.  Any
            // message will do, and this one changes nothing.
            .chain(iced::Task::done(Message::LogGrew))
            .chain(wait(300));
    }

    // And a table result scrolls among itself, so a shot can show a run of
    // rows that starts below the first screenful of the region.
    if let Some(offset) = number_after("table:") {
        task = task
            .chain(iced::widget::operation::scroll_to(
                crate::ui::table_scroll_id(),
                iced::widget::scrollable::AbsoluteOffset {
                    x: Some(0.0),
                    y: Some(offset),
                },
            ))
            .chain(iced::Task::done(Message::LogGrew))
            .chain(wait(300));
    }

    task.chain(
        window::oldest()
            .and_then(window::screenshot)
            .map(Message::Screenshot),
    )
}

/// A pause that yields, in milliseconds.
///
/// `tokio::time::sleep` rather than `std::thread::sleep`, because a blocking
/// sleep here holds the runtime thread and a stubbed run never gets to finish —
/// which photographs a half-drawn pane.
fn wait(millis: u64) -> iced::Task<Message> {
    iced::Task::future(async move {
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
    })
    .discard()
}

/// Writes a captured window to `path` as a PNG.
pub fn write_png(path: &std::path::Path, shot: &window::Screenshot) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(&shot.rgba)
        .map_err(std::io::Error::other)
}
