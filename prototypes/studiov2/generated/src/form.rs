// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What a user has filled in, and the command line it makes.
//!
//! One [`Form`] per command, plus one for the globals.  A form holds a
//! [`Field`] per option and knows nothing about widgets — the mapping from an
//! option to a control is in [`crate::ui::control`], and this module is what
//! that control reads and writes.
//!
//! Everything here is driven by [`Opt`], so a form for a command nobody has
//! seen behaves the same as a form for one that shipped years ago.

use studiov2_commands::{Group, Kind, Opt, name};

/// Which form a message is about.
///
/// The globals are not repeated on every pane, so they are not a command's
/// options and cannot be addressed by a command index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// The options every command accepts, shown once.
    Globals,
    /// One command's own options, by its index in `COMMANDS`.
    Command(usize),
}

/// One editable value on screen.
///
/// A message carries this rather than a widget identity, so the update path
/// never has to work out what was clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// Which form the field belongs to.
    pub form: Where,
    /// The option's index within that form.
    pub opt: usize,
    /// Which entry of a `multiple` option.  Zero for everything else.
    pub entry: usize,
}

impl Target {
    /// A target naming the first — usually only — entry of an option.
    pub fn new(form: Where, opt: usize) -> Self {
        Self {
            form,
            opt,
            entry: 0,
        }
    }

    /// The same target pointing at a different entry of the same option.
    pub fn at(self, entry: usize) -> Self {
        Self { entry, ..self }
    }
}

/// What has been filled in for one option.
///
/// Three variants rather than five, because the five [`Kind`]s differ in what
/// control draws them and not in what a command line needs from them.  A
/// number and a domain value both leave as one string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// A bare `--flag`, ticked or not.
    Flag(bool),
    /// One value, empty meaning the option is not given.
    Single(String),
    /// Every value given for an option that can repeat.
    Many(Vec<String>),
}

impl Field {
    /// The starting state for an option: its default where it states one, and
    /// nothing where it does not.
    pub fn new(opt: &Opt) -> Self {
        if matches!(opt.kind, Kind::Flag) {
            return Self::Flag(opt.default == Some("true"));
        }

        match (opt.multiple, opt.default) {
            (true, Some(default)) => Self::Many(vec![default.to_owned()]),
            (true, None) => Self::Many(Vec::new()),
            (false, default) => Self::Single(default.unwrap_or_default().to_owned()),
        }
    }

    /// Whether the user has given nothing.
    ///
    /// A flag is never empty: not ticked is an answer, and the CLI reads it as
    /// one.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Flag(_) => false,
            Self::Single(value) => value.trim().is_empty(),
            Self::Many(values) => values.iter().all(|value| value.trim().is_empty()),
        }
    }

    /// The value at an entry, for the control drawing it.
    pub fn value(&self, entry: usize) -> &str {
        match self {
            Self::Flag(_) => "",
            Self::Single(value) => value,
            Self::Many(values) => values.get(entry).map_or("", String::as_str),
        }
    }

    /// How many entries the control has to draw.
    pub fn entries(&self) -> usize {
        match self {
            Self::Flag(_) | Self::Single(_) => 1,
            Self::Many(values) => values.len(),
        }
    }

    /// Whether a flag is ticked.
    pub fn is_on(&self) -> bool {
        matches!(self, Self::Flag(true))
    }
}

/// Everything filled in for one set of options.
///
/// The options travel with the fields because nothing else can tell the two
/// apart: index three of a form means nothing without index three of the
/// options it was built from.
#[derive(Clone)]
pub struct Form {
    /// The options this form fills in.
    pub opts: &'static [Opt],
    /// The sets among them the CLI treats as one choice.
    pub groups: &'static [Group],
    /// One field per option, in the same order.
    pub fields: Vec<Field>,
}

impl Form {
    /// An untouched form for a set of options and the groups over them.
    pub fn new(opts: &'static [Opt], groups: &'static [Group]) -> Self {
        Self {
            opts,
            groups,
            fields: opts.iter().map(Field::new).collect(),
        }
    }

    /// Whether an option has a value on it, by name.
    ///
    /// Names are how the description writes a conflict, so this is what a
    /// conflict has to be resolved against.
    pub fn given(&self, long: &str) -> bool {
        self.opts
            .iter()
            .position(|opt| opt.long == long)
            .and_then(|index| self.fields.get(index))
            .is_some_and(|field| match field {
                Field::Flag(on) => *on,
                value => !value.is_empty(),
            })
    }

    /// A group by name.
    pub fn group(&self, name: &str) -> Option<&'static Group> {
        self.groups.iter().find(|group| group.name == name)
    }

    /// The options of a group that have been given.
    pub fn group_given(&self, group: &Group) -> Vec<&'static str> {
        group
            .opts
            .iter()
            .copied()
            .filter(|long| self.given(long))
            .collect()
    }

    /// Whether this option, on its own, is one the user has to answer.
    ///
    /// An option inside a group is not: the group speaks for it, and marking
    /// three members of a pick-one group as required each says the opposite of
    /// what the group means.
    pub fn required_here(&self, index: usize) -> bool {
        self.opts.get(index).is_some_and(|opt| {
            opt.must_supply()
                && !self
                    .groups
                    .iter()
                    .any(|group| group.opts.contains(&opt.long))
        })
    }

    /// Which given option has made this one necessary, if any.
    ///
    /// `requires` turns a normally optional control into one the command
    /// cannot run without, and the pane has to say so at the control rather
    /// than only at the Run button.
    pub fn needed_now(&self, index: usize) -> Option<&'static str> {
        let spec = self.opts.get(index)?;
        if self.given(spec.long) {
            return None;
        }

        self.opts
            .iter()
            .enumerate()
            .filter(|(other, opt)| self.given(opt.long) && self.blocked(*other).is_none())
            .find(|(_, opt)| {
                opt.requires.iter().any(|name| {
                    *name == spec.long
                        || self
                            .group(name)
                            .is_some_and(|group| group.opts.contains(&spec.long))
                })
            })
            .map(|(_, opt)| opt.long)
    }

    /// How a name in `conflicts` or `requires` should be read aloud.
    ///
    /// The CLI writes an option name or a group name in the same place, so a
    /// consumer that assumes the first prints `--reboot_mode`, which is not an
    /// option anybody can type.
    pub fn name_of(&self, name: &str) -> String {
        match self.group(name) {
            Some(group) => {
                let members: Vec<String> =
                    group.opts.iter().map(|long| name::label(long)).collect();
                format!("one of {}", members.join(", "))
            }
            None => name::label(name),
        }
    }

    /// Why an option cannot be given as things stand, if it cannot.
    ///
    /// Two sources, both from the description: what the option says it clashes
    /// with, and a group it belongs to that admits only one member.  A blocked
    /// option keeps whatever was typed into it — clearing the thing that
    /// blocks it brings the value back untouched.
    pub fn blocked(&self, index: usize) -> Option<String> {
        let spec = self.opts.get(index)?;

        for name in spec.conflicts {
            if self.given(name) {
                return Some(format!("Not while {} is set.", name::label(name)));
            }
            if let Some(group) = self.group(name)
                && let Some(first) = self.group_given(group).first()
            {
                return Some(format!("Not while {} is set.", name::label(first)));
            }
        }

        for group in self.groups.iter().filter(|group| !group.multiple) {
            if !group.opts.contains(&spec.long) {
                continue;
            }
            let other = self
                .group_given(group)
                .into_iter()
                .find(|long| *long != spec.long);
            if let Some(other) = other {
                return Some(format!(
                    "Only one of this set: {} is set.",
                    name::label(other)
                ));
            }
        }

        None
    }

    /// Ticks or clears a flag.  Anything else is left alone.
    pub fn toggle(&mut self, opt: usize, on: bool) {
        if let Some(Field::Flag(state)) = self.fields.get_mut(opt) {
            *state = on;
        }
    }

    /// Writes a value into one entry, growing a repeating option to reach it.
    ///
    /// A [`Kind::Number`] keeps only its digits.  The rule lives here rather
    /// than in the text box so it holds however the value arrives — typed,
    /// pasted, or poked in by the screenshot harness.
    pub fn set(&mut self, opt: usize, entry: usize, value: String) {
        let value = match self.opts.get(opt).map(|opt| &opt.kind) {
            Some(Kind::Number) => value.chars().filter(char::is_ascii_digit).collect(),
            _ => value,
        };

        match self.fields.get_mut(opt) {
            Some(Field::Single(current)) => *current = value,
            Some(Field::Many(values)) => {
                if values.len() <= entry {
                    values.resize(entry + 1, String::new());
                }
                values[entry] = value;
            }
            _ => {}
        }
    }

    /// Adds another empty entry to a repeating option.
    pub fn add(&mut self, opt: usize) {
        if let Some(Field::Many(values)) = self.fields.get_mut(opt) {
            values.push(String::new());
        }
    }

    /// Drops one entry of a repeating option.
    pub fn remove(&mut self, opt: usize, entry: usize) {
        if let Some(Field::Many(values)) = self.fields.get_mut(opt)
            && entry < values.len()
        {
            values.remove(entry);
        }
    }

    /// Returns an option to unset, which is what an optional field needs and a
    /// pick list has no other way to reach.
    pub fn clear(&mut self, opt: usize) {
        if let Some(field) = self.fields.get_mut(opt) {
            *field = match field {
                Field::Flag(_) => Field::Flag(false),
                Field::Single(_) => Field::Single(String::new()),
                Field::Many(_) => Field::Many(Vec::new()),
            };
        }
    }

    /// The words this form contributes to a command line.
    ///
    /// A blocked option contributes nothing, so the line on screen is one the
    /// CLI would accept rather than one it would reject.
    pub fn args(&self) -> Vec<String> {
        let mut words = Vec::new();

        for (index, (opt, field)) in self.opts.iter().zip(&self.fields).enumerate() {
            if self.blocked(index).is_some() {
                continue;
            }

            match field {
                Field::Flag(true) => words.push(format!("--{}", opt.long)),
                Field::Flag(false) => {}
                Field::Single(value) => {
                    if !value.trim().is_empty() {
                        words.push(format!("--{}", opt.long));
                        words.push(quote(value.trim()));
                    }
                }
                Field::Many(values) => {
                    for value in values.iter().filter(|value| !value.trim().is_empty()) {
                        words.push(format!("--{}", opt.long));
                        words.push(quote(value.trim()));
                    }
                }
            }
        }

        words
    }

    /// Whether a name in `requires` has been answered.
    ///
    /// A group counts as answered when any member of it is given, which is
    /// what `requires = "reboot_mode"` asks for.
    pub fn satisfied(&self, name: &str) -> bool {
        match self.group(name) {
            Some(group) => !self.group_given(group).is_empty(),
            None => self.given(name),
        }
    }

    /// What a user still has to answer before the command can run.
    ///
    /// Phrased rather than counted, so the pane can say what is wanted.  Three
    /// sources: an option the description insists on, a required group with
    /// none of its members given — which is one answer, not several — and a
    /// companion an option that *is* given says it needs.
    pub fn missing(&self) -> Vec<String> {
        let in_a_group = |long: &str| self.groups.iter().any(|group| group.opts.contains(&long));

        let mut wanted: Vec<String> = self
            .opts
            .iter()
            .enumerate()
            .filter(|(index, opt)| {
                opt.must_supply()
                    && self.fields[*index].is_empty()
                    && self.blocked(*index).is_none()
                    && !in_a_group(opt.long)
            })
            .map(|(_, opt)| format!("--{}", opt.long))
            .collect();

        for group in self.groups.iter().filter(|group| group.required) {
            if self.group_given(group).is_empty() {
                let names: Vec<String> =
                    group.opts.iter().map(|long| format!("--{long}")).collect();
                wanted.push(format!("one of {}", names.join(", ")));
            }
        }

        // An option that is given drags in whatever it says it needs.  A
        // blocked option is not given, so it drags in nothing.
        for (index, opt) in self.opts.iter().enumerate() {
            if !self.given(opt.long) || self.blocked(index).is_some() {
                continue;
            }
            for name in opt.requires.iter().filter(|name| !self.satisfied(name)) {
                let wants = format!("{} for --{}", self.name_of(name), opt.long);
                if !wanted.contains(&wants) {
                    wanted.push(wants);
                }
            }
        }

        wanted
    }
}

/// Written by hand because `Opt` derives nothing, so a form holding one
/// cannot derive `Debug` — and a failing test with no way to print the form is
/// a test nobody can read.
impl std::fmt::Debug for Form {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.opts.iter().map(|opt| opt.long).zip(self.fields.iter()))
            .finish()
    }
}

/// Wraps a value in single quotes where a shell would need them.
///
/// Not a shell escaper — the command line on screen is there to be read and
/// copied, and a value holding a quote is a value the CLI would reject anyway.
fn quote(value: &str) -> String {
    if value.chars().any(char::is_whitespace) {
        format!("'{value}'")
    } else {
        value.to_owned()
    }
}
