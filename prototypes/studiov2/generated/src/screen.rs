// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The generated screen: state, messages, and the update path.
//!
//! The state is one form per command and a selection, and that is the whole
//! model.  Nothing is stored per command *kind* and nothing is stored about
//! what a command means — a message names a [`Target`], the form it names
//! changes, and the pane redraws from the description.

use std::time::Duration;

use iced::Task;
use studiov2_commands::{COMMANDS, Command, GLOBALS};
use studiov2_shared::Shared;

use crate::form::{Form, Target, Where};
use crate::real;
use crate::stub::{self, Body, Output};
use crate::tree;
use crate::ui;

/// How long a stubbed run pretends to take.
///
/// Long enough that the Run button visibly disables, short enough that a
/// screenshot run does not have to wait for it.
const RUN_DELAY: Duration = Duration::from_millis(350);

/// Everything the user can do.
#[derive(Debug, Clone)]
pub enum Message {
    /// The filter box was edited.
    Filter(String),
    /// A tab was clicked: the depth it sits at, and the word on it.
    Tab(usize, &'static str),
    /// The long description was opened or closed.
    ToggleLong,
    /// The globals section was opened or closed.
    ToggleGlobals,
    /// A flag was ticked or cleared.
    Toggled(Target, bool),
    /// A value was typed or chosen.
    Edited(Target, String),
    /// Another entry was added to a repeating option.
    Added(Target),
    /// One entry of a repeating option was dropped.
    Removed(Target),
    /// An optional value was returned to unset.
    Cleared(Target),
    /// Whether Run should take the error path.
    ForceError(bool),
    /// Run the selected command.
    Run,
    /// Put the built command line on the system clipboard.
    CopyCli,
    /// The run finished.
    Ran(Result<Output, String>),
    /// A development screenshot arrived.  See [`crate::dev`].
    Screenshot(iced::window::Screenshot),

    /// Somebody outside this screen appended to the shared log.
    LogGrew,
    /// The selected device changed.
    DeviceChanged,
}

/// The whole screen.
pub struct Screen {
    /// The command on show, as an index into `COMMANDS`.
    ///
    /// Unset only while the filter matches nothing, which is a state the pane
    /// has to draw rather than a state to avoid.
    pub selected: Option<usize>,
    /// One form per command, in `COMMANDS` order.
    ///
    /// All of them, all the time: a user switching tabs to check something and
    /// coming back expects what they typed to still be there, and 46 forms of
    /// empty strings cost nothing worth reasoning about.
    pub forms: Vec<Form>,
    /// The form for the options every command accepts.
    pub globals: Form,
    /// What is typed in the filter box.
    pub filter: String,
    /// The commands the filter leaves, as indices into `COMMANDS`.
    pub matches: Vec<usize>,
    /// Whether the long description is open.
    pub long_open: bool,
    /// Whether the globals section is open.
    pub globals_open: bool,
    /// Whether Run takes the error path.
    pub force_error: bool,
    /// Whether a run is in flight.
    pub running: bool,
    /// The last result, and the command that produced it.
    ///
    /// Held with its command so switching tabs does not show one command's
    /// answer under another's title.
    pub result: Option<(usize, Result<Output, String>)>,
}

impl Screen {
    /// The screen, plus whatever the screenshot hook asks for.
    pub fn boot(shared: &mut Shared) -> (Self, Task<Message>) {
        let mut screen = Self::new();
        if crate::dev::shot_path().is_some() {
            crate::dev::apply_setup(&mut screen, shared);
            return (screen, crate::dev::capture());
        }
        (screen, Task::none())
    }

    /// A fresh screen with the first command selected.
    pub fn new() -> Self {
        let matches: Vec<usize> = (0..COMMANDS.len()).collect();

        Self {
            selected: matches.first().copied(),
            forms: COMMANDS
                .iter()
                .map(|command| Form::new(command.opts, command.groups))
                .collect(),
            globals: Form::new(GLOBALS, &[]),
            filter: String::new(),
            matches,
            long_open: false,
            globals_open: false,
            force_error: false,
            running: false,
            result: None,
        }
    }

    /// The command on show.
    pub fn command(&self) -> Option<&'static Command> {
        self.selected.map(|index| &COMMANDS[index])
    }

    /// The form for the command on show.
    pub fn form(&self) -> Option<&Form> {
        self.selected.and_then(|index| self.forms.get(index))
    }

    /// The form a target names.
    fn form_mut(&mut self, target: Target) -> Option<&mut Form> {
        match target.form {
            Where::Globals => Some(&mut self.globals),
            Where::Command(index) => self.forms.get_mut(index),
        }
    }

    /// Selects a command by its exact path, for the screenshot harness.
    ///
    /// Returns whether one was found — a caller driving 46 panes needs to know
    /// it got the pane it asked for.
    pub fn select_path(&mut self, path: &[&str]) -> bool {
        let found = COMMANDS
            .iter()
            .position(|command| command.path.len() == path.len() && command.path == path);

        if let Some(index) = found {
            self.selected = Some(index);
            self.result = None;
        }
        found.is_some()
    }

    /// The index of an option by name, for the screenshot harness.
    pub fn opt_index(&self, form: Where, name: &str) -> Option<usize> {
        let opts = match form {
            Where::Globals => self.globals.opts,
            Where::Command(index) => self.forms.get(index)?.opts,
        };
        opts.iter()
            .position(|opt| opt.long == name || opt.aliases.contains(&name))
    }

    /// The command line the form describes, as it would be typed.
    ///
    /// Globals lead, because that is where the CLI takes them and because it
    /// makes the one thing shown in two places on the screen read as one
    /// thing.
    pub fn command_line(&self) -> String {
        let Some(command) = self.command() else {
            return "onerom".to_owned();
        };

        let mut words = vec!["onerom".to_owned()];
        words.extend(self.globals.args());
        words.extend(command.path.iter().map(|word| (*word).to_owned()));
        if let Some(form) = self.form() {
            words.extend(form.args());
        }
        words.join(" ")
    }

    /// The options still to be filled in before Run does anything.
    ///
    /// Globals count: an option the CLI insists on is insisted on wherever it
    /// is declared.
    pub fn missing(&self) -> Vec<String> {
        let mut missing = self.globals.missing();
        if let Some(form) = self.form() {
            missing.extend(form.missing());
        }
        missing
    }

    /// Whether Run has everything it needs.
    pub fn can_run(&self) -> bool {
        self.selected.is_some() && !self.running && self.missing().is_empty()
    }

    /// What a real run of the command on show produces, where one is wired.
    ///
    /// `None` means no [`real::Runner`] fits it and Run falls back to the stub.
    /// Separate from [`Screen::update`] because the task that method hands back
    /// cannot be looked into, so a test driving Run has no other way to read
    /// what Run did.
    pub fn real_run(&self) -> Option<Result<Output, String>> {
        let index = self.selected?;
        let runner = real::runner(&COMMANDS[index])?;
        Some(runner.run(&real::Inputs::new(self.forms.get(index)?)))
    }

    /// The result to draw, if the one held belongs to the command on show.
    pub fn shown_result(&self) -> Option<&Result<Output, String>> {
        let (index, result) = self.result.as_ref()?;
        (Some(*index) == self.selected).then_some(result)
    }

    /// Applies a filter, keeping the selection where the filter allows it.
    fn refilter(&mut self) {
        self.matches = tree::matching(&self.filter);
        let kept = self
            .selected
            .is_some_and(|index| self.matches.contains(&index));
        if !kept {
            self.selected = self.matches.first().copied();
            self.result = None;
        }
    }

    /// Writes a line to the shared log.
    fn note(&self, shared: &mut Shared, line: &str) {
        if let Err(error) = shared.log.append(&[std::sync::Arc::from(line)]) {
            // A log that cannot be written to is not worth losing a result
            // over — the pane is about to show it anyway.
            eprintln!("log: {error}");
        }
    }

    pub fn update(&mut self, message: Message, shared: &mut Shared) -> Task<Message> {
        match message {
            Message::Filter(text) => {
                self.filter = text;
                self.refilter();
                Task::none()
            }
            Message::Tab(depth, segment) => {
                let mut prefix: Vec<&'static str> = self
                    .command()
                    .map(|command| command.path[..depth.min(command.path.len())].to_vec())
                    .unwrap_or_default();
                prefix.truncate(depth);
                prefix.push(segment);

                if let Some(index) = tree::under(&self.matches, &prefix) {
                    self.selected = Some(index);
                    self.long_open = false;
                }
                Task::none()
            }
            Message::ToggleLong => {
                self.long_open = !self.long_open;
                Task::none()
            }
            Message::ToggleGlobals => {
                self.globals_open = !self.globals_open;
                Task::none()
            }
            Message::Toggled(target, on) => {
                if let Some(form) = self.form_mut(target) {
                    form.toggle(target.opt, on);
                }
                Task::none()
            }
            Message::Edited(target, value) => {
                if let Some(form) = self.form_mut(target) {
                    form.set(target.opt, target.entry, value);
                }
                Task::none()
            }
            Message::Added(target) => {
                if let Some(form) = self.form_mut(target) {
                    form.add(target.opt);
                }
                Task::none()
            }
            Message::Removed(target) => {
                if let Some(form) = self.form_mut(target) {
                    form.remove(target.opt, target.entry);
                }
                Task::none()
            }
            Message::Cleared(target) => {
                if let Some(form) = self.form_mut(target) {
                    form.clear(target.opt);
                }
                Task::none()
            }
            Message::ForceError(force) => {
                self.force_error = force;
                Task::none()
            }
            Message::CopyCli => iced::clipboard::write(self.command_line()),

            Message::Run => {
                let (Some(index), true) = (self.selected, self.can_run()) else {
                    return Task::none();
                };
                let command = &COMMANDS[index];
                let line = self.command_line();
                let fail = self.force_error;

                // A real run is file work on one small image, so it happens
                // here rather than on the runtime.  Sending the form into a
                // future would mean copying every value out of it first, and
                // buying nothing for a job that finishes before the next
                // frame.
                let real = self.real_run();

                self.running = true;
                self.result = None;
                self.note(shared, &format!("$ {line}"));

                match real {
                    Some(result) => Task::done(Message::Ran(result)),
                    None => Task::perform(
                        async move {
                            tokio::time::sleep(RUN_DELAY).await;
                            stub::run(command, &line, fail)
                        },
                        Message::Ran,
                    ),
                }
            }
            Message::Ran(result) => {
                self.running = false;
                for line in transcript(&result) {
                    self.note(shared, &line);
                }
                self.result = self.selected.map(|index| (index, result));
                Task::none()
            }
            Message::Screenshot(shot) => {
                if let Some(path) = crate::dev::shot_path()
                    && let Err(error) = crate::dev::write_png(&path, &shot)
                {
                    eprintln!("screenshot: {error}");
                }
                iced::exit()
            }

            Message::LogGrew | Message::DeviceChanged => Task::none(),
        }
    }

    pub fn view<'a>(&'a self, shared: &'a Shared) -> iced::Element<'a, Message> {
        ui::page(self, shared)
    }
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

/// A result as lines for the log.
///
/// The log is text, so a table and a tree have to become text to reach it.
/// The pane draws the structure — this is the same answer said flatly.
fn transcript(result: &Result<Output, String>) -> Vec<String> {
    match result {
        Err(error) => error.lines().map(str::to_owned).collect(),
        Ok(Output::Nothing) => vec!["ok".to_owned()],
        Ok(Output::Line(line)) => vec![line.clone()],
        Ok(Output::Drawing(drawing)) => drawing.lines().map(str::to_owned).collect(),
        Ok(Output::Table { headers, body }) => {
            // A heading is part of the answer, so the log keeps it rather than
            // handing back a run of rows with nothing saying what they are.
            let mut lines = vec![headers.join("  ")];
            match body {
                Body::Rows(rows) => lines.extend(rows.iter().map(|row| row.join("  "))),
                Body::Sections(sections) => {
                    for section in sections {
                        lines.push(section.heading.clone());
                        lines.extend(section.rows.iter().map(|row| row.join("  ")));
                    }
                }
            }
            lines
        }
        Ok(Output::Tree(root)) => ui::flatten(root),
    }
}
