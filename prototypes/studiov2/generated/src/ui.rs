// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The pane, drawn from the description.
//!
//! Read [`value_widget`] first.  It is the whole point of the exercise: a
//! [`Kind`] and a [`studiov2_commands::Source`] between them decide every
//! control on every pane, in one match, and nothing anywhere else in the crate
//! decides what a control looks like.  Everything above it is layout, and
//! everything below it is the result.

use iced::widget::{
    Space, button, checkbox, column, container, pick_list, row, rule, scrollable, stack, text,
    text_input,
};
use iced::{Alignment, Center, Color, Element, Fill, Shrink};
use studiov2_commands::{COMMANDS, Group, Kind, Opt, name};
use studiov2_shared::Shared;

use crate::form::{Field, Form, Target, Where};
use crate::real;
use crate::resolve::{self, Browse, Context, Offer};
use crate::screen::{Message, Screen};
use crate::stub::{Body, Node, Output};
use crate::style;
use crate::tree;

/// How wide the label column beside a control is.
const LABEL_WIDTH: f32 = 190.0;

/// How wide the key at the head of a row of tabs is.
const KEY_WIDTH: f32 = 96.0;

/// The size an option's name is set at.
///
/// A point over body text and in the body colour, because it is the first
/// thing read on every row — the help under it is the dim one.
const LABEL_SIZE: f32 = style::BODY + 1.0;

/// How wide a single-value control is.
const CONTROL_WIDTH: f32 = 300.0;

/// How wide the pick list beside a box is, where a set has both.
///
/// Narrower than a control of its own: the box is where the value is read, and
/// the list is a way of filling it in.
const PICKER_WIDTH: f32 = 150.0;

/// The side of the colour swatch beside a control.
const SWATCH: f32 = 24.0;

/// The tallest a table's rows get before they scroll among themselves.
///
/// About twenty rows at the pitch a row is drawn at — long enough that a
/// listing reads as a listing rather than a peephole, and short enough that
/// the command line and the Run button that produced it are still on the page
/// with the result scrolled fully into view.
const TABLE_HEIGHT: f32 = 420.0;

/// The page's scroll region, so a screenshot run can be told where to look.
pub fn scroll_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("page")
}

/// A table result's own scroll region.
///
/// Named for the same reason the page's is: a region a screenshot run cannot
/// move is a region nobody can review past its first screenful.
pub fn table_scroll_id() -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::new("table")
}

/// The whole page.
///
/// Two borrows, as every screen here takes: this screen's own state, and the
/// state it shares with the rest of the app.
pub fn page<'a>(app: &'a Screen, shared: &'a Shared) -> Element<'a, Message> {
    // Resolved once for the page.  Which board is in front of the user is not
    // a fact about an option, and asking it per control would ask it 30 times
    // for one answer.
    let context = Context::new(shared);

    let body = column![
        toolbar(app, shared),
        globals(app, &context),
        divider(),
        tabs(app),
        divider(),
        pane(app, &context),
    ]
    .spacing(14)
    .padding([20, 28]);

    container(scrollable(body).id(scroll_id()).height(Fill))
        .style(style::panel)
        .width(Fill)
        .height(Fill)
        .into()
}

// ---------------------------------------------------------------- the parts --

/// A section heading.
fn heading(title: &str) -> Element<'_, Message> {
    text(title).size(style::HEADING).font(style::MEDIUM).into()
}

/// The line between sections.
fn divider<'a>() -> Element<'a, Message> {
    rule::horizontal(1).style(style::divider).into()
}

/// A dim note.
fn note<'a>(content: impl text::IntoFragment<'a>) -> iced::widget::Text<'a> {
    text(content).size(style::NOTE).style(style::dim)
}

/// A disclosure heading that opens and closes a section.
fn summary<'a>(
    open: bool,
    title: impl text::IntoFragment<'a>,
    on_press: Message,
) -> Element<'a, Message> {
    button(
        row![
            text(if open { "\u{25BE}" } else { "\u{25B8}" }).size(13),
            text(title).size(style::BODY).font(style::MEDIUM),
        ]
        .spacing(8)
        .align_y(Center),
    )
    .style(style::disclosure)
    .padding(0)
    .on_press(on_press)
    .into()
}

/// The filter box, the device the app is pointed at, and the error switch.
fn toolbar<'a>(app: &'a Screen, shared: &'a Shared) -> Element<'a, Message> {
    let filter = text_input("Filter commands, options and aliases...", &app.filter)
        .on_input(Message::Filter)
        .size(style::BODY)
        .padding([8, 10])
        .width(360)
        .style(style::field);

    let device = match shared.device.as_ref() {
        Some(device) => note(format!("Device: {device}")),
        None => note("No device selected."),
    };

    let fail = checkbox(app.force_error)
        .label("Force the error path (stubs only)")
        .on_toggle(Message::ForceError)
        .size(15)
        .text_size(style::BODY)
        .spacing(8)
        .style(style::tick);

    column![
        row![
            filter,
            note(format!(
                "{} of {} commands",
                app.matches.len(),
                COMMANDS.len()
            )),
            Space::new().width(Fill),
            fail,
        ]
        .spacing(14)
        .align_y(Center),
        device,
        note(resolve::FAKED),
    ]
    .spacing(8)
    .into()
}

/// The options every command accepts, in the one place they appear.
///
/// Repeating them on 46 panes would be 46 copies of the same four controls,
/// and a user setting `--serial` once means it for the session.
fn globals<'a>(app: &'a Screen, context: &Context) -> Element<'a, Message> {
    let set = app.globals.args().len();
    let title = if set == 0 {
        "Global options".to_owned()
    } else {
        format!("Global options ({set} set)")
    };

    let head = summary(app.globals_open, title, Message::ToggleGlobals);
    if !app.globals_open {
        return head;
    }

    container(column![head, options(Where::Globals, &app.globals, context)].spacing(12))
        .style(style::card)
        .padding([14, 16])
        .width(Fill)
        .into()
}

/// The tab rows, one per depth of the selected command's path.
fn tabs(app: &Screen) -> Element<'_, Message> {
    let path = app
        .command()
        .map(|command| command.path)
        .unwrap_or_default();
    let levels = tree::levels(&app.matches, path);

    if levels.is_empty() {
        return note("Nothing matches that filter.").into();
    }

    let rows = levels.into_iter().map(|level| {
        let buttons = level.segments.into_iter().map(|segment| {
            let selected = level.current == Some(segment);
            button(text(name::heading(segment)).size(style::BODY))
                .style(style::tab(selected))
                .padding([6, 12])
                .on_press(Message::Tab(level.depth, segment))
                .into()
        });

        row![
            container(
                text(tab_key(path, level.depth))
                    .size(LABEL_SIZE)
                    .font(style::MEDIUM)
            )
            .width(KEY_WIDTH)
            .align_x(Alignment::End),
            row(buttons).spacing(6).width(Fill).wrap(),
        ]
        .spacing(12)
        .align_y(Center)
        .into()
    });

    column(rows).spacing(6).into()
}

/// The key at the head of a row of tabs.
///
/// The rows read as a sentence: the first asks which command, and every row
/// after it is headed by the word chosen on the row above.  Under
/// `control rgb on` that is `Command:`, `Control:`, `RGB:` — derived from the
/// path, so no command is named here.
pub fn tab_key(path: &[&str], depth: usize) -> String {
    match depth.checked_sub(1).and_then(|above| path.get(above)) {
        Some(parent) => format!("{}:", name::heading(parent)),
        None => "Command:".to_owned(),
    }
}

/// A command's path as a heading.
///
/// `Control -> RGB -> On`, not `onerom control rgb on`.  The command line is
/// shown once, under `CLI:`, and a heading that looked like a second one made
/// the pane read as two command lines and no title.
pub fn title(path: &[&str]) -> String {
    path.iter()
        .map(|word| name::heading(word))
        .collect::<Vec<_>>()
        .join(" \u{2192} ")
}

// --------------------------------------------------------------- the pane ---

/// One command's pane: what it is, what it takes, and what it would run.
fn pane<'a>(app: &'a Screen, context: &Context) -> Element<'a, Message> {
    let (Some(index), Some(command), Some(form)) = (app.selected, app.command(), app.form()) else {
        return note("No command selected.").into();
    };

    let mut content = column![
        text(title(command.path))
            .size(style::HEADING)
            .font(style::SEMIBOLD)
            .style(style::gold),
        text(command.about).size(style::BODY),
    ]
    .spacing(6);

    if let Some(long) = command.long_about {
        content = content.push(summary(app.long_open, "Details", Message::ToggleLong));
        if app.long_open {
            content = content.push(
                container(text(long).size(style::NOTE).font(style::MONO))
                    .style(style::card)
                    .padding([12, 14])
                    .width(Fill),
            );
        }
    }

    content = content.push(divider());

    if command.opts.is_empty() {
        content = content.push(note("This command takes no options of its own."));
    } else {
        content = content.push(options(Where::Command(index), form, context));
    }

    content = content
        .push(divider())
        .push(command_line(app))
        .push(run_row(app))
        .push(result(app));

    content.spacing(12).into()
}

/// Every option of a form, with the grouped ones drawn together.
///
/// A [`Group`] is the only grouping the description carries, and it says more
/// than a heading would: these options are one choice.  Drawing them apart
/// would throw that away, so a group's members appear as a block where the
/// first of them would have gone, and are skipped where they fall later.
fn options<'a>(where_: Where, form: &'a Form, context: &Context) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut drawn: Vec<&'static str> = Vec::new();

    for (index, opt) in form.opts.iter().enumerate() {
        if drawn.contains(&opt.long) {
            continue;
        }

        let Some(group) = form
            .groups
            .iter()
            .find(|group| group.opts.contains(&opt.long))
        else {
            rows.push(option_row(where_, index, form, context));
            continue;
        };

        let members = group.opts.iter().filter_map(|long| {
            let position = form.opts.iter().position(|opt| opt.long == *long)?;
            Some(option_row(where_, position, form, context))
        });

        rows.push(
            container(
                column![
                    text(group_rule(group)).size(style::GROUP).style(style::dim),
                    column(members).spacing(14),
                ]
                .spacing(10),
            )
            .style(style::card)
            .padding([12, 14])
            .width(Fill)
            .into(),
        );
        drawn.extend(group.opts.iter().copied());
    }

    column(rows).spacing(14).into()
}

/// What a group says about the options in it, in words.
fn group_rule(group: &Group) -> String {
    match (group.required, group.multiple) {
        (true, false) => "PICK EXACTLY ONE".to_owned(),
        (true, true) => "PICK AT LEAST ONE".to_owned(),
        (false, false) => "PICK AT MOST ONE".to_owned(),
        (false, true) => "RELATED".to_owned(),
    }
}

/// One option: its name, its control, and what the CLI says about it.
fn option_row<'a>(
    where_: Where,
    index: usize,
    form: &'a Form,
    context: &Context,
) -> Element<'a, Message> {
    let target = Target::new(where_, index);
    let opt = &form.opts[index];

    // A blocked option keeps its value and loses its control, so the pane
    // cannot build a command line the CLI would refuse.
    let blocked = form.blocked(index);
    let needed_now = form.needed_now(index);
    let wanted = blocked.is_none() && (form.required_here(index) || needed_now.is_some());

    // The name a user reads, not the flag they would type.  The long name
    // with its dashes is still on the CLI line and still what the filter
    // searches, which is where it belongs.
    let caption = if wanted {
        text(format!("{} *", name::label(opt.long)))
            .size(LABEL_SIZE)
            .style(style::gold)
    } else {
        text(name::label(opt.long)).size(LABEL_SIZE)
    };

    let body: Element<'a, Message> = match blocked {
        Some(reason) => text(reason).size(style::BODY).style(style::danger).into(),
        None => control(target, opt, &form.fields[index], context),
    };

    // The name sits beside the control and the help sits under it, so the
    // name lines up on the control itself rather than on the middle of the
    // pair.  A repeating option is the exception: its control is a list, and
    // a name halfway down a list of three reads as belonging to the second
    // entry - so that one keeps its name at the top.
    let align = if opt.multiple {
        Alignment::Start
    } else {
        Alignment::Center
    };

    column![
        row![
            container(caption)
                .width(LABEL_WIDTH)
                .align_x(Alignment::End),
            body,
        ]
        .spacing(14)
        .align_y(align),
        row![
            Space::new().width(LABEL_WIDTH + 14.0).height(Shrink),
            note(annotation(form, index, needed_now)),
        ],
    ]
    .spacing(4)
    .into()
}

/// The help line under a control.
///
/// Everything the description carries about the option that the control
/// cannot show by itself, in the order a user would want it.
///
/// Every option this names is named the way its own control is labelled, so a
/// pane speaks one vocabulary.  The exception is an alias, which keeps its
/// dashes because it is telling a user what they may type.
pub fn annotation(form: &Form, index: usize, needed_now: Option<&str>) -> String {
    let opt = &form.opts[index];
    let mut parts = vec![opt.help.to_owned()];

    if !opt.aliases.is_empty() {
        let spellings: Vec<String> = opt
            .aliases
            .iter()
            .map(|alias| format!("--{alias}"))
            .collect();
        parts.push(format!("Also {}.", spellings.join(", ")));
    }
    if opt.multiple {
        parts.push("Can be given more than once.".to_owned());
    }
    if !opt.requires.is_empty() {
        let needed: Vec<String> = opt.requires.iter().map(|name| form.name_of(name)).collect();
        parts.push(format!("Needs {}.", needed.join(", ")));
    }
    if !opt.conflicts.is_empty() {
        let clashes: Vec<String> = opt
            .conflicts
            .iter()
            .map(|name| form.name_of(name))
            .collect();
        parts.push(format!("Not with {}.", clashes.join(", ")));
    }
    if form.required_here(index) {
        parts.push("Required.".to_owned());
    }
    if let Some(asker) = needed_now {
        parts.push(format!(
            "Needed now, because {} is set.",
            name::label(asker)
        ));
    }

    parts.join("  ")
}

/// The control for one option, with its unset and repeat handling around it.
pub fn control<'a>(
    target: Target,
    opt: &'static Opt,
    field: &'a Field,
    context: &Context,
) -> Element<'a, Message> {
    if matches!(opt.kind, Kind::Flag) {
        return value_widget(target, opt, field, context);
    }

    if opt.multiple {
        let entries = (0..field.entries()).map(|entry| {
            let at = target.at(entry);
            row![
                value_widget(at, opt, field, context),
                button(text("remove").size(style::NOTE))
                    .style(style::small)
                    .padding([6, 10])
                    .on_press(Message::Removed(at)),
            ]
            .spacing(8)
            .align_y(Center)
            .into()
        });

        let add = button(text("+ add").size(style::NOTE))
            .style(style::small)
            .padding([6, 10])
            .on_press(Message::Added(target));

        let mut list = column(entries).spacing(6);
        if field.entries() == 0 {
            list = list.push(note("Not given."));
        }
        return list.push(add).spacing(6).into();
    }

    let widget = value_widget(target, opt, field, context);
    if !opt.optional || field.is_empty() {
        return widget;
    }

    // An optional value needs a way back to unset, and a pick list has none of
    // its own once something is chosen.
    row![
        widget,
        button(text("unset").size(style::NOTE))
            .style(style::small)
            .padding([6, 10])
            .on_press(Message::Cleared(target)),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

/// **The widget mapping.**  The one place a [`Kind`] or a
/// [`studiov2_commands::Source`] becomes a control.
///
/// The source is asked first, because it says more: `--colour` is a `Domain`
/// type and a text box by kind, and a list of ten named colours with a swatch
/// by source.  [`crate::resolve`] answers with values and this decides the
/// shape.
///
/// | what is known | widget |
/// | --- | --- |
/// | a closed set of values | pick list |
/// | a set the CLI takes other values besides | text box, and a pick list |
/// | a file or a directory | text box, and a Browse button |
/// | nothing, and `Kind::Flag` | checkbox |
/// | nothing, and `Kind::Choice(values)` | pick list |
/// | nothing, and `Kind::Number` | text box that refuses anything but digits |
/// | nothing, and `Kind::Text` | text box |
/// | nothing, and `Kind::Domain(name)` | text box, with the type's name where |
/// | | the placeholder would otherwise be empty |
///
/// A new `Kind` or a new `Source` fails to compile — here for the first, and in
/// [`crate::resolve::offer`] for the second — and nowhere else, which is the
/// property worth having.
pub fn value_widget<'a>(
    target: Target,
    opt: &'static Opt,
    field: &'a Field,
    context: &Context,
) -> Element<'a, Message> {
    let value = field.value(target.entry);

    // A flag has no value for a source to be the source of, so it never asks.
    let offer = match opt.kind {
        Kind::Flag => Offer::Free,
        _ => resolve::offer(opt.source, context),
    };

    match offer {
        Offer::Pick { choices, open } => return picker(target, opt, value, choices, open),
        Offer::Browse(browse) => return browser(target, opt, value, browse),
        Offer::Free => {}
    }

    match &opt.kind {
        Kind::Flag => checkbox(field.is_on())
            .label(opt.value_name.unwrap_or("Enabled"))
            .on_toggle(move |on| Message::Toggled(target, on))
            .size(15)
            .text_size(style::BODY)
            .spacing(8)
            .style(style::tick)
            .into(),

        Kind::Choice(values) => {
            let selected = values.iter().copied().find(|offered| *offered == value);
            pick_list(*values, selected, move |chosen: &'static str| {
                Message::Edited(target, chosen.to_owned())
            })
            .placeholder(opt.value_name.unwrap_or("Select..."))
            .text_size(style::BODY)
            .padding([8, 10])
            .width(CONTROL_WIDTH)
            .style(style::picker)
            .into()
        }

        // The digits rule is enforced by `Form::set`, so a paste cannot get
        // round it and a test can reach it without a widget.
        Kind::Number => box_for(opt, value)
            .on_input(move |raw| Message::Edited(target, raw))
            .into(),

        Kind::Text => box_for(opt, value)
            .on_input(move |raw| Message::Edited(target, raw))
            .into(),

        // The same box as `Text`, and kept as its own arm because that is the
        // finding: a domain type with no source is a value set the pane cannot
        // offer, so it hands the user a blank box and hopes.
        Kind::Domain(_) => box_for(opt, value)
            .on_input(move |raw| Message::Edited(target, raw))
            .into(),
    }
}

/// A set of values, with a box beside it where the set is not the whole story.
///
/// A closed set is a pick list alone: every value the CLI accepts is on it, so
/// a box could only be used to type one of the same values or a wrong one.  An
/// open set keeps the box, because the list is the easy half of what the option
/// takes and hiding the rest would make the pane refuse what the CLI accepts.
fn picker<'a>(
    target: Target,
    opt: &'static Opt,
    value: &'a str,
    choices: Vec<String>,
    open: bool,
) -> Element<'a, Message> {
    let selected = choices.iter().find(|offered| *offered == value).cloned();
    let width = if open { PICKER_WIDTH } else { CONTROL_WIDTH };
    let placeholder = if open {
        "or pick...".to_owned()
    } else {
        opt.value_name.unwrap_or("Select...").to_owned()
    };

    let list = pick_list(choices, selected, move |chosen: String| {
        Message::Edited(target, chosen)
    })
    .placeholder(placeholder)
    .text_size(style::BODY)
    .padding([8, 10])
    .width(width)
    .style(style::picker);

    let mut whole = row![].spacing(8).align_y(Center);
    if open {
        whole = whole.push(box_for(opt, value).on_input(move |raw| Message::Edited(target, raw)));
    }
    whole = whole.push(list);
    if let Some(swatch) = swatch(opt, value) {
        whole = whole.push(swatch);
    }
    whole.into()
}

/// The colour a value stands for, drawn as a block beside its control.
///
/// Nothing where the value names no colour, which is what an empty option and a
/// half-typed hex value both look like.
fn swatch<'a>(opt: &'static Opt, value: &str) -> Option<Element<'a, Message>> {
    let (red, green, blue) = resolve::swatch(opt.source, value)?;
    let fill = Color::from_rgb8(red, green, blue);

    Some(
        container(Space::new().width(SWATCH).height(SWATCH))
            .style(style::swatch(fill))
            .into(),
    )
}

/// A path, and the button that fills it in.
///
/// The button is beside the box rather than replacing it because a path is
/// still typeable, and because what a Browse button chose has to be readable
/// afterwards.
fn browser<'a>(
    target: Target,
    opt: &'static Opt,
    value: &'a str,
    browse: Browse,
) -> Element<'a, Message> {
    let label = match browse {
        Browse::Directory => "Browse folders",
        Browse::Open | Browse::Save => "Browse",
    };

    row![
        box_for(opt, value).on_input(move |raw| Message::Edited(target, raw)),
        // The same small button as unset, add and remove: gold on this page is
        // Run, and a row of gold Browse buttons argues with it.
        button(text(label).size(style::NOTE))
            .style(style::small)
            .padding([8, 12])
            .on_press(Message::Edited(target, resolve::browse_path(browse))),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

/// The text box every control a user types into is built on.
fn box_for<'a>(opt: &'static Opt, value: &'a str) -> text_input::TextInput<'a, Message> {
    let placeholder = match (opt.value_name, &opt.kind) {
        (Some(name), _) => name.to_owned(),
        (None, Kind::Domain(name)) => (*name).to_owned(),
        (None, _) => opt.long.to_uppercase(),
    };

    text_input(&placeholder, value)
        .size(style::BODY)
        .padding([8, 10])
        .width(CONTROL_WIDTH)
        .style(style::field)
}

// ------------------------------------------------------- run and result -----

/// The command line the form describes, live.
fn command_line(app: &Screen) -> Element<'_, Message> {
    row![
        container(text("CLI:").size(LABEL_SIZE).font(style::MEDIUM))
            .width(KEY_WIDTH)
            .align_x(Alignment::End),
        container(
            text(app.command_line())
                .size(style::NOTE)
                .font(style::MONO)
                .style(style::gold),
        )
        .style(style::terminal)
        .padding([10, 12])
        .width(Fill),
        button(copy_mark())
            .style(studiov2_shared::style::icon_button)
            .padding([8, 10])
            .on_press(Message::CopyCli),
    ]
    .spacing(12)
    .align_y(Center)
    .into()
}

/// Two overlapping squares, drawn.
///
/// The front square is filled so the back one's edges stop behind it, which is
/// what makes two outlines read as one copy mark rather than a grid.
fn copy_mark() -> Element<'static, Message> {
    let square = || {
        container(Space::new().width(9).height(9))
            .style(style::icon_square)
            .width(Shrink)
            .height(Shrink)
    };

    stack![
        container(square()).padding(iced::Padding::default().bottom(4).right(4)),
        container(square()).padding(iced::Padding::default().top(4).left(4)),
    ]
    .width(15)
    .height(15)
    .into()
}

/// The Run button, and whatever is stopping it.
fn run_row(app: &Screen) -> Element<'_, Message> {
    let mut run = button(text(if app.running { "Running..." } else { "Run" }).size(style::BODY))
        .style(style::gold_button)
        .padding([10, 24]);
    if app.can_run() {
        run = run.on_press(Message::Run);
    }

    let missing = app.missing();
    let status: Element<'_, Message> = if missing.is_empty() {
        note(match app.command().and_then(real::runner) {
            Some(runner) => format!("Runs for real, through {}.", runner.does),
            None => "Stubbed: nothing runs, and the answer below is invented.".to_owned(),
        })
        .into()
    } else {
        text(format!("Still needed: {}", missing.join(", ")))
            .size(style::NOTE)
            .style(style::danger)
            .into()
    };

    row![run, status].spacing(14).align_y(Center).into()
}

/// The last result for the command on show.
fn result(app: &Screen) -> Element<'_, Message> {
    let Some(result) = app.shown_result() else {
        return Space::new().width(Shrink).height(Shrink).into();
    };

    let body: Element<'_, Message> = match result {
        Err(error) => text(error)
            .size(style::NOTE)
            .font(style::MONO)
            .style(style::danger)
            .into(),
        Ok(output) => rendered(output),
    };

    column![
        heading("Result"),
        container(body)
            .style(style::card)
            .padding([12, 14])
            .width(Fill),
    ]
    .spacing(8)
    .into()
}

/// One of the five output shapes, drawn.
fn rendered(output: &Output) -> Element<'_, Message> {
    match output {
        Output::Nothing => note("No output.").into(),
        Output::Line(line) => text(line).size(style::BODY).into(),
        Output::Drawing(drawing) => text(drawing).size(style::NOTE).font(style::MONO).into(),
        Output::Tree(root) => text(flatten(root).join("\n"))
            .size(style::NOTE)
            .font(style::MONO)
            .into(),
        Output::Table { headers, body } => table(headers, body),
    }
}

/// A grid, with the columns wide enough for their widest cell.
///
/// The header row stays outside the scroll region, so the column titles are
/// still there once the rows have moved under them.  That is also why the
/// widths are measured across every row of every section rather than per
/// section — one measurement is what makes one header row honest.
fn table<'a>(headers: &'a [String], body: &'a Body) -> Element<'a, Message> {
    let all = body.rows();
    let widths: Vec<f32> = headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let longest = all
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.chars().count())
                .chain(std::iter::once(header.chars().count()))
                .max()
                .unwrap_or(1);
            longest as f32 * 7.6 + 18.0
        })
        .collect();

    let head = row(headers.iter().zip(&widths).map(|(header, width)| {
        container(
            text(header)
                .size(style::NOTE)
                .font(style::MONO)
                .style(style::dim),
        )
        .width(*width)
        .into()
    }));

    let grid: Element<'a, Message> = match body {
        Body::Rows(rows) => rows_block(rows, &widths),
        Body::Sections(sections) => column(sections.iter().map(|section| {
            column![
                text(&section.heading).size(style::NOTE).font(style::MEDIUM),
                rows_block(&section.rows, &widths),
            ]
            .spacing(5)
            .into()
        }))
        .spacing(14)
        .into(),
    };

    column![
        head,
        rule::horizontal(1).style(style::divider),
        // Bounded, so a long answer cannot push the controls that produced it
        // off the bottom of the page.  A table shorter than the bound keeps
        // its own height and shows no scrollbar.
        container(
            scrollable::Scrollable::with_direction(
                grid,
                // Spacing rather than an overlay, so the bar takes its width
                // out of the card only once there is something to scroll.
                scrollable::Direction::Vertical(scrollable::Scrollbar::new().width(8).spacing(8)),
            )
            .id(table_scroll_id()),
        )
        .max_height(TABLE_HEIGHT),
    ]
    .spacing(6)
    .into()
}

/// One run of rows, at the widths the whole table was measured at.
fn rows_block<'a>(rows: &'a [Vec<String>], widths: &[f32]) -> Element<'a, Message> {
    column(rows.iter().map(|cells| {
        row(cells.iter().zip(widths).map(|(cell, width)| {
            container(text(cell).size(style::NOTE).font(style::MONO))
                .width(*width)
                .into()
        }))
        .into()
    }))
    .spacing(3)
    .into()
}

/// A tree as indented lines.
///
/// Public because the log takes the same answer as text, and two renderings of
/// one tree that could disagree would be worse than one that cannot.
pub fn flatten(root: &Node) -> Vec<String> {
    let mut lines = Vec::new();
    walk(root, "", true, true, &mut lines);
    lines
}

/// Appends one node and its children, drawing the connecting lines.
fn walk(node: &Node, prefix: &str, last: bool, root: bool, lines: &mut Vec<String>) {
    if root {
        lines.push(node.label.clone());
    } else {
        lines.push(format!(
            "{prefix}{} {}",
            if last { "\\-" } else { "|-" },
            node.label
        ));
    }

    let child_prefix = if root {
        String::new()
    } else {
        format!("{prefix}{}", if last { "   " } else { "|  " })
    };

    for (index, child) in node.children.iter().enumerate() {
        walk(
            child,
            &child_prefix,
            index + 1 == node.children.len(),
            false,
            lines,
        );
    }
}
