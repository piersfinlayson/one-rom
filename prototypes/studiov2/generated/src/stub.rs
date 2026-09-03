// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What running a command produces, and a stand-in that produces it.
//!
//! Nothing here touches a device.  The point of the stub is the *shape* of the
//! answer: a pane that can only show a line of text is a pane that cannot show
//! a pinout, and finding that out is most of what this prototype is for.
//!
//! Five shapes cover what the CLI prints — [`Output`].  Which one a command
//! produces is the one thing the description cannot tell us, so [`shape`]
//! guesses and is marked as the guess it is.

use studiov2_commands::Command;

/// One node of a tree result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// What this node says.
    pub label: String,
    /// What hangs off it.
    pub children: Vec<Node>,
}

impl Node {
    /// A leaf.
    pub fn leaf(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            children: Vec::new(),
        }
    }

    /// A node with children.
    pub fn branch(label: impl Into<String>, children: Vec<Node>) -> Self {
        Self {
            label: label.into(),
            children,
        }
    }
}

/// A run of rows sharing a heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// What its rows have in common, in whatever words the command uses.
    pub heading: String,
    /// The rows under it, one entry per column in each.
    pub rows: Vec<Vec<String>>,
}

/// Everything under a table's header row.
///
/// Two states rather than a heading per row, because a heading is a fact about
/// a run of rows.  A per-row copy of it is a column that says the same word
/// over and over, which is what the grouping decays into when it has nowhere
/// else to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Rows with nothing dividing them.
    Rows(Vec<Vec<String>>),
    /// Runs of rows, each under a heading of its own.
    Sections(Vec<Section>),
}

impl Body {
    /// Every row, in the order they are drawn.
    pub fn rows(&self) -> Vec<&Vec<String>> {
        match self {
            Self::Rows(rows) => rows.iter().collect(),
            Self::Sections(sections) => sections.iter().flat_map(|section| &section.rows).collect(),
        }
    }
}

/// What a command produced.
///
/// The five are not a taxonomy of the CLI's output formats — they are the five
/// things a pane has to be able to draw.  A table and a tree differ because
/// one needs columns and the other needs indentation, not because the CLI
/// calls them different things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// A single line, such as a confirmation or one measured value.
    Line(String),
    /// A grid with a header row: a device list, a slot list, a register dump.
    Table {
        /// The column titles.
        headers: Vec<String>,
        /// The rows, sectioned or not.
        body: Body,
    },
    /// A fixed-pitch drawing, such as a socket or a jumper header.
    Drawing(String),
    /// A nested structure, such as the parsed contents of a firmware image.
    Tree(Node),
    /// The command said nothing, which is what success looks like for most of
    /// the ones that change something.
    Nothing,
}

/// Which of the five a command produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// [`Output::Line`].
    Line,
    /// [`Output::Table`].
    Table,
    /// [`Output::Drawing`].
    Drawing,
    /// [`Output::Tree`].
    Tree,
    /// [`Output::Nothing`].
    Nothing,
}

/// What a command prints, guessed from its path.
///
/// PLACEHOLDER.  The description says nothing about output, so this stands in
/// for a field it does not carry yet.  It is deliberately the only place in
/// the crate that reads a path for anything but navigation, and it is keyed on
/// the group and the verb rather than on a whole command — a rule that gets
/// most of them roughly right and none of them exactly right.
pub fn shape(command: &Command) -> Shape {
    let group = command.path.first().copied().unwrap_or_default();
    let verb = command.path.last().copied().unwrap_or_default();

    match (group, verb) {
        (_, "scan" | "list" | "releases" | "chips") => Shape::Table,
        (_, "socket" | "header" | "pinout" | "gpio") => Shape::Drawing,
        ("inspect" | "info" | "read" | "monitor", _) => Shape::Tree,
        ("control" | "program" | "flash" | "write" | "update", _) => Shape::Nothing,
        _ => Shape::Line,
    }
}

/// Runs a command, without running anything.
///
/// PLACEHOLDER, and the content is invented — the command line is threaded
/// through it so a pane obviously reflects what was asked for rather than a
/// fixture.  `fail` is the error path, which exists because seeing what an
/// error looks like is half of what a prototype pane is for.
pub fn run(command: &Command, line: &str, fail: bool) -> Result<Output, String> {
    let name = command.path.join(" ");

    if fail {
        return Err(format!(
            "onerom {name}: no One ROM answered on the USB bus.\n  \
             while running: {line}\n  \
             is the device connected, and did a previous command leave it stopped?"
        ));
    }

    Ok(match shape(command) {
        Shape::Line => Output::Line(format!("{name}: done in 42 ms.")),
        Shape::Table => Output::Table {
            headers: ["Serial", "Board", "Firmware", "State"]
                .map(str::to_owned)
                .to_vec(),
            body: Body::Rows(vec![
                ["ORFA-0027-3F1C", "fire-28-d", "0.7.2", "Running"]
                    .map(str::to_owned)
                    .to_vec(),
                ["ORFA-0031-A840", "fire-24-a", "0.7.1", "Stopped"]
                    .map(str::to_owned)
                    .to_vec(),
            ]),
        },
        Shape::Drawing => Output::Drawing(DRAWING.to_owned()),
        Shape::Tree => Output::Tree(Node::branch(
            format!("onerom {name}"),
            vec![
                Node::branch(
                    "metadata".to_owned(),
                    vec![
                        Node::leaf("schema 0.7.2"),
                        Node::leaf("board fire-28-d"),
                        Node::leaf("build 2026-08-14T09:12:07Z"),
                    ],
                ),
                Node::branch(
                    "slots".to_owned(),
                    vec![
                        Node::branch(
                            "0  kernal.bin".to_owned(),
                            vec![Node::leaf("type 23128"), Node::leaf("8 KiB")],
                        ),
                        Node::branch(
                            "1  basic.bin".to_owned(),
                            vec![Node::leaf("type 2364"), Node::leaf("8 KiB")],
                        ),
                    ],
                ),
                Node::leaf("runtime  serving, 0 faults"),
            ],
        )),
        Shape::Nothing => Output::Nothing,
    })
}

/// The fixed-pitch drawing a `Shape::Drawing` command hands back.
const DRAWING: &str = "\
        +--------\\_/--------+
   A7 --| 1             28 |-- VCC
   A6 --| 2             27 |-- A14
   A5 --| 3             26 |-- A13
   A4 --| 4             25 |-- A8
   A3 --| 5             24 |-- A9
   A2 --| 6             23 |-- A11
   A1 --| 7    23128    22 |-- /OE
   A0 --| 8             21 |-- A10
   D0 --| 9             20 |-- /CE
   D1 --| 10            19 |-- D7
   D2 --| 11            18 |-- D6
  GND --| 12            17 |-- D5
   D3 --| 13            16 |-- D4
      \\ | 14            15 | /
        +-------------------+";
