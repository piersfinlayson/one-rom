// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Walking the CLI's command tree into a flat list of leaves.
//!
//! The description carries commands a user can actually run, so a struct that
//! holds nothing but a subcommand is walked through rather than emitted.  What
//! comes out is what `src/lib.rs` describes: one entry per distinct path, in
//! the order the CLI declares them.

use std::collections::HashSet;

use syn::{Attribute, Expr, Field, Fields, ItemStruct, Lit, Type};

use crate::attrs::{self, ArgSpec, CommandSpec, DefaultValue, Help};
use crate::constants;
use crate::index::Index;
use crate::source::Source;
use crate::values::ValueSets;

/// The whole description, ready to be written out.
pub struct Description {
    /// The options every command accepts.
    pub globals: Vec<Opt>,

    /// Every runnable command, in declaration order.
    pub commands: Vec<Command>,
}

/// One runnable command.
///
/// This and [`Opt`] and [`Kind`] below mirror the types in `src/lib.rs`, which
/// is where each field is explained.  Saying it twice would leave two
/// explanations nothing compares.
pub struct Command {
    pub path: Vec<String>,
    pub about: String,
    pub long_about: Option<String>,
    pub opts: Vec<Opt>,
    pub groups: Vec<Group>,
}

/// Options the CLI treats as one choice.
pub struct Group {
    pub name: String,
    pub opts: Vec<String>,
    pub required: bool,
    pub multiple: bool,
}

/// One option of one command.
pub struct Opt {
    pub long: String,
    pub aliases: Vec<String>,
    pub help: String,
    pub value_name: Option<String>,
    pub kind: Kind,
    pub optional: bool,
    pub multiple: bool,
    pub default: Option<String>,
    pub conflicts: Vec<String>,
    pub requires: Vec<String>,
    pub source: Option<Source>,
}

/// What sort of value an option takes.
pub enum Kind {
    Flag,
    Text,
    Number,
    Choice(Vec<String>),
    Domain(String),
}

/// The struct clap's derive starts from, and the enum holding its subcommands.
const ROOT: &str = "Cli";

/// Read the whole tree, starting from the top-level `Parser`.
pub fn walk(index: &Index, values: &ValueSets) -> Description {
    let root = index.struct_def(ROOT);
    let mut commands_enum = None;

    for field in fields(root) {
        if CommandSpec::read(&field.attrs).subcommand {
            commands_enum = Some(type_name(&field.ty));
            continue;
        }

        assert!(
            ArgSpec::read(&field.attrs).global,
            "{ROOT} carries a non-global option, which every command would silently accept"
        );
    }

    let (globals, groups) = options(root, index, values);
    assert!(
        groups.is_empty(),
        "the global options declare a group, which belongs to no command"
    );

    let commands_enum = commands_enum.unwrap_or_else(|| panic!("{ROOT} has no subcommand"));
    let mut walker = Walker {
        index,
        values,
        seen: HashSet::new(),
        commands: Vec::new(),
    };
    walker.enum_variants(&commands_enum, &[]);

    Description {
        globals,
        commands: walker.commands,
    }
}

struct Walker<'a> {
    index: &'a Index,
    values: &'a ValueSets,
    seen: HashSet<String>,
    commands: Vec<Command>,
}

impl Walker<'_> {
    /// Walk one subcommand enum, adding a leaf for each variant that is one.
    fn enum_variants(&mut self, enum_name: &str, prefix: &[String]) {
        for variant in &self.index.enum_def(enum_name).variants {
            let spec = CommandSpec::read(&variant.attrs);
            if spec.hide {
                continue;
            }

            let name = spec
                .name
                .unwrap_or_else(|| attrs::kebab_variant(&variant.ident.to_string()));
            let mut path = prefix.to_vec();
            path.push(name);

            let args = match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    type_name(&fields.unnamed[0].ty)
                }
                _ => panic!(
                    "{enum_name}::{} holds something other than one Args struct",
                    variant.ident
                ),
            };

            match branch(self.index.struct_def(&args)) {
                Some(next) => self.enum_variants(&next, &path),
                None => self.leaf(&args, path, &variant.attrs),
            }
        }
    }

    /// Add a command, unless its options have already been reached by another
    /// path.
    ///
    /// The CLI puts `inspect peek live` at the top level as `peek`, and
    /// `program` inside `firmware` as well as at the top.  Both spellings run
    /// the same command, so the first one the walk reaches - the one the CLI
    /// declares first - is the one described, and the other is an alias with
    /// nothing of its own to say.
    fn leaf(&mut self, args: &str, path: Vec<String>, attrs: &[Attribute]) {
        if self.seen.contains(args) {
            return;
        }
        self.seen.insert(args.to_string());

        let mut paragraphs = attrs::paragraphs(attrs).into_iter();
        let about = paragraphs
            .next()
            .unwrap_or_else(|| panic!("onerom {} has no doc comment", path.join(" ")));
        let rest: Vec<String> = paragraphs.collect();

        let (opts, groups) = options(self.index.struct_def(args), self.index, self.values);

        self.commands.push(Command {
            path,
            about,
            long_about: (!rest.is_empty()).then(|| rest.join("\n\n")),
            opts,
            groups,
        });
    }
}

/// The subcommand enum a structure-only struct leads to, where it is one.
fn branch(item: &ItemStruct) -> Option<String> {
    let mut fields = fields(item);
    let only = fields.next()?;
    if fields.next().is_some() || !CommandSpec::read(&only.attrs).subcommand {
        return None;
    }
    Some(type_name(&only.ty))
}

fn fields(item: &ItemStruct) -> impl Iterator<Item = &Field> {
    match &item.fields {
        Fields::Named(named) => named.named.iter(),
        _ => panic!("{} is not a struct with named fields", item.ident),
    }
}

/// Everything one args struct declares: its options and its groups.
///
/// Both come out together because they name each other.  A group can list its
/// members, and an option can name a group it conflicts with, so neither can be
/// resolved without the other in hand.
fn options(item: &ItemStruct, index: &Index, values: &ValueSets) -> (Vec<Opt>, Vec<Group>) {
    let declared: Vec<(&Field, ArgSpec)> = fields(item)
        .filter(|field| !CommandSpec::read(&field.attrs).subcommand)
        .map(|field| (field, ArgSpec::read(&field.attrs)))
        .filter(|(_, spec)| !spec.skip)
        .collect();

    // clap names an option by its field, and a user by its `--long`.  Every
    // name written in a group, a conflict or a requirement is the former, and
    // everything emitted here is the latter.
    let longs: Vec<(String, String)> = declared
        .iter()
        .map(|(field, spec)| (field_name(field), long_name(field, spec)))
        .collect();
    let long_of = |name: &str| {
        longs
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, long)| long.clone())
    };

    let groups: Vec<Group> = CommandSpec::read(&item.attrs)
        .groups
        .into_iter()
        .map(|group| {
            // A group that does not list its members takes them from the
            // options that name it, which is the other half of clap's own
            // arrangement and the only other place membership is written.
            let opts: Vec<String> = if group.members.is_empty() {
                declared
                    .iter()
                    .filter(|(_, spec)| spec.group.as_deref() == Some(&group.name))
                    .map(|(field, spec)| long_name(field, spec))
                    .collect()
            } else {
                group
                    .members
                    .iter()
                    .map(|member| {
                        long_of(member).unwrap_or_else(|| {
                            panic!(
                                "{}'s group {} lists {member}, which is not an option of it",
                                item.ident, group.name
                            )
                        })
                    })
                    .collect()
            };
            assert!(
                !opts.is_empty(),
                "{}'s group {} has no members",
                item.ident,
                group.name
            );
            Group {
                name: group.name,
                opts,
                required: group.required,
                multiple: group.multiple,
            }
        })
        .collect();

    // A conflict or a requirement names either an option or a group.  Anything
    // else would make clap itself panic when it builds the command, so it fails
    // here rather than reaching a pane as a name that points at nothing.
    let resolve = |name: &str, field: &str, what: &str| {
        long_of(name)
            .or_else(|| {
                groups
                    .iter()
                    .find(|group| group.name == name)
                    .map(|group| group.name.clone())
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}'s {field} {what} {name}, which is neither an option nor a group of it",
                    item.ident
                )
            })
    };

    let opts = declared
        .iter()
        .map(|(field, spec)| {
            let name = field_name(field);
            let mut opt = option(field, spec, index, values);
            opt.conflicts = spec
                .conflicts
                .iter()
                .map(|other| resolve(other, &name, "conflicts with"))
                .collect();
            opt.requires = spec
                .requires
                .iter()
                .map(|other| resolve(other, &name, "requires"))
                .collect();
            opt
        })
        .collect();

    (opts, groups)
}

/// A field's name, which is the id clap knows the option by.
fn field_name(field: &Field) -> String {
    field
        .ident
        .as_ref()
        .expect("a named field has an identifier")
        .to_string()
}

/// The `--long` a user types, which is the field name unless clap is told
/// otherwise.
fn long_name(field: &Field, spec: &ArgSpec) -> String {
    let name = field_name(field);
    assert!(
        spec.has_long,
        "--{name} has no long name, and the One ROM CLI has no positionals"
    );
    spec.long
        .clone()
        .unwrap_or_else(|| attrs::kebab_field(&name))
}

/// Turn one field into the option it declares, less what only its struct knows.
fn option(field: &Field, spec: &ArgSpec, index: &Index, values: &ValueSets) -> Opt {
    let long = long_name(field, spec);
    let (inner, optional, multiple) = shape(&field.ty);
    let kind = match inner.as_str() {
        "bool" => Kind::Flag,
        "String" => Kind::Text,
        "u8" | "u16" | "u32" | "u64" | "usize" => Kind::Number,
        _ => match values.of(&inner, spec.value_parser.as_deref()) {
            Some(choices) => Kind::Choice(choices.to_vec()),
            None => Kind::Domain(inner),
        },
    };

    let default = spec.default.as_ref().map(|default| match default {
        DefaultValue::Text(text) => text.clone(),
        DefaultValue::Rust(expr) => rust_default(&long, expr),
    });

    // A default a user could not type is a default nothing can act on, and the
    // only place that can be checked is against a set the CLI itself states.
    if let (Kind::Choice(choices), Some(default)) = (&kind, &default) {
        assert!(
            choices.contains(default),
            "--{long} defaults to {default}, which is not one of its values"
        );
    }

    Opt {
        long: long.clone(),
        aliases: spec.aliases.clone(),
        help: help(&long, spec, &field.attrs, index),
        value_name: spec.value_name.clone(),
        kind,
        optional,
        multiple,
        default,
        conflicts: Vec::new(),
        requires: Vec::new(),
        source: None,
    }
}

/// What a `default_value_t` amounts to on a command line.
///
/// clap prints the value itself, so a `ValueEnum` variant reaches a user as its
/// kebab-case name and never as the Rust path the source writes.  A bare name
/// is a metadata-schema constant, quoted the same way an option's help quotes
/// one.
fn rust_default(long: &str, expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(text) => text.value(),
            other => attrs::tokens(quote::ToTokens::to_token_stream(other)),
        },
        Expr::Path(path) => {
            let segments = &path.path.segments;
            let last = segments
                .last()
                .expect("a path has at least one segment")
                .ident
                .to_string();
            if segments.len() > 1 {
                attrs::kebab_variant(&last)
            } else {
                constants::value(&last).to_string()
            }
        }
        other => panic!(
            "--{long} defaults to {}, which this description cannot turn into a value",
            attrs::render(other)
        ),
    }
}

/// An option's help, from wherever the CLI writes it.
fn help(long: &str, spec: &ArgSpec, attrs: &[Attribute], index: &Index) -> String {
    match &spec.help {
        Some(Help::Text(text)) => text.clone(),
        Some(Help::Named(name)) => resolve(index.const_value(name)),
        None => attrs::paragraphs(attrs)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("--{long} has no help text")),
    }
}

/// What a `const` used as help says.
///
/// The CLI builds these with `concat!` around a `const_str!`, which pastes in a
/// firmware constant's value at compile time.  The same value is read out of
/// the metadata schema here, so a pane shows the number and not the name of it.
fn resolve(expr: &Expr) -> String {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(text) => text.value(),
            _ => panic!("a help const is not a string"),
        },
        Expr::Macro(mac) if mac.mac.path.is_ident("concat") => mac
            .mac
            .parse_body_with(syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated)
            .expect("could not read a concat!")
            .into_iter()
            .map(|part| resolve(&part))
            .collect(),
        Expr::Macro(mac) if mac.mac.path.is_ident("const_str") => {
            let name: syn::LitStr = mac.mac.parse_body().expect("const_str! takes a name");
            constants::value(&name.value()).to_string()
        }
        other => panic!("a help const holds {}", attrs::render(other)),
    }
}

/// Peel `Option` and `Vec` off a field's type, keeping what they said.
fn shape(ty: &Type) -> (String, bool, bool) {
    let mut optional = false;
    let mut multiple = false;
    let mut current = ty;

    loop {
        match wrapper(current) {
            Some(("Option", inner)) => {
                optional = true;
                current = inner;
            }
            Some(("Vec", inner)) => {
                multiple = true;
                current = inner;
            }
            _ => return (type_name(current), optional, multiple),
        }
    }
}

/// The single type argument of `Option<T>` or `Vec<T>`.
fn wrapper(ty: &Type) -> Option<(&'static str, &Type)> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    let name = match segment.ident.to_string().as_str() {
        "Option" => "Option",
        "Vec" => "Vec",
        _ => return None,
    };
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some((name, inner)),
        _ => None,
    }
}

/// A type as the description names it - the type's own name where it has one.
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .expect("a path has at least one segment")
            .ident
            .to_string(),
        other => attrs::tokens(quote::ToTokens::to_token_stream(other)),
    }
}
