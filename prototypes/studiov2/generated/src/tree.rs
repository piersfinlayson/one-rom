// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The command tree, recovered from the one thing that carries it.
//!
//! `COMMANDS` is flat and every command is a leaf, so the hierarchy a menu
//! needs is in [`studiov2_commands::Command::path`] and nowhere else.  One row
//! of tabs per path depth reads it back: `control`, then `rgb`, then `on`.
//!
//! The filter narrows the same list, so the tabs and the search are the same
//! mechanism seen twice.

use studiov2_commands::{COMMANDS, Command};

/// One row of tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    /// How deep into a path this row sits.
    pub depth: usize,
    /// The words offered here, in the order the commands declare them.
    pub segments: Vec<&'static str>,
    /// The word the selected command uses, where it reaches this depth.
    ///
    /// A command shorter than the row leaves this unset — that is what a row
    /// offering to drill further into a selected leaf looks like.
    pub current: Option<&'static str>,
}

/// The rows of tabs for a selected path, across the commands the filter left.
///
/// A row exists for every depth some matching command can still be told apart
/// at, so a three-word command gets three rows and `scan` gets one — plus a
/// second where something sits under it.
pub fn levels(matches: &[usize], path: &[&'static str]) -> Vec<Level> {
    let mut levels = Vec::new();

    for depth in 0.. {
        if depth > path.len() {
            break;
        }

        let mut segments: Vec<&'static str> = Vec::new();
        for command in matches.iter().filter_map(|index| COMMANDS.get(*index)) {
            if command.path.len() <= depth || !command.path.starts_with(&path[..depth]) {
                continue;
            }
            let segment = command.path[depth];
            if !segments.contains(&segment) {
                segments.push(segment);
            }
        }

        if segments.is_empty() {
            break;
        }

        levels.push(Level {
            depth,
            segments,
            current: path.get(depth).copied(),
        });
    }

    levels
}

/// The command a tab click should select.
///
/// The click names a word at a depth, which on its own is a prefix rather than
/// a command.  The first matching command under that prefix is the one that
/// opens, which keeps a click on a group meaningful without the tabs having to
/// know which of its children is interesting.
pub fn under(matches: &[usize], prefix: &[&'static str]) -> Option<usize> {
    matches
        .iter()
        .copied()
        .find(|index| COMMANDS[*index].path.starts_with(prefix))
}

/// The commands a filter leaves, as indices into `COMMANDS`.
///
/// Matched against the path, the description and every option's name and
/// aliases — the aliases because a user searches for what they call the thing,
/// which is the reason the description carries them at all.
pub fn matching(filter: &str) -> Vec<usize> {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return (0..COMMANDS.len()).collect();
    }

    (0..COMMANDS.len())
        .filter(|index| mentions(&COMMANDS[*index], &needle))
        .collect()
}

/// Whether one command answers to a lowercase search term.
fn mentions(command: &Command, needle: &str) -> bool {
    if command.path.join(" ").to_lowercase().contains(needle)
        || command.about.to_lowercase().contains(needle)
    {
        return true;
    }

    command.opts.iter().any(|opt| {
        opt.long.to_lowercase().contains(needle)
            || opt
                .aliases
                .iter()
                .any(|alias| alias.to_lowercase().contains(needle))
    })
}
