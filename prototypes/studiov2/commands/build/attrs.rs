// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Reading what a clap attribute and a doc comment say.
//!
//! Only the entries that reach the description are read.  The conflicts and
//! the groups are among them: they say a user may pick one of these and not
//! two, which is a thing to draw and not only a thing to check.  What is left
//! out is the machinery of parsing - the actions, the value counts, the short
//! flags - which describes a command line rather than a choice.

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Lit, Meta, Token};

/// Where an option's default is written.
pub enum DefaultValue {
    /// A `default_value = "..."`, already the text a user would type.
    Text(String),

    /// A `default_value_t = ...`, which is Rust - an enum variant or a
    /// constant - and has to be turned into that text.
    Rust(Expr),
}

/// Where an option's help text is written.
pub enum Help {
    /// A doc comment, or a `help = "..."` string.
    Text(String),

    /// A `help = CONST`, which has to be resolved against the `const` itself.
    Named(String),
}

/// The `#[arg(...)]` entries that describe an option to a user.
#[derive(Default)]
pub struct ArgSpec {
    /// `#[arg(skip)]`, which is a field clap never sees.
    pub skip: bool,

    /// `#[arg(global = true)]`, which puts the option on every command.
    pub global: bool,

    /// Whether the field is an option at all.  A field with no `long` is a
    /// positional, and the One ROM CLI has none.
    pub has_long: bool,

    /// An explicit `long = "..."`, overriding the field name.
    pub long: Option<String>,

    /// `visible_alias` and `visible_aliases`, in the order written.  A plain
    /// `alias` is deliberately left out: it does not appear in `--help`, so a
    /// user has no way to know it exists and nothing can search for it.
    pub aliases: Vec<String>,

    /// The `value_name` placeholder, where one is written.
    pub value_name: Option<String>,

    /// What the CLI says the option defaults to, where it says anything.
    pub default: Option<DefaultValue>,

    /// Help from a `help = ...`, where the option has one instead of a doc
    /// comment.
    pub help: Option<Help>,

    /// The `value_parser`, named by its last path segment.
    pub value_parser: Option<String>,

    /// `#[arg(value_enum)]`, which asks clap for the type's own value set.
    pub value_enum: bool,

    /// The group this option joins, from `group = "..."`.
    ///
    /// It is how a group declared with no `args([...])` gets its members.
    pub group: Option<String>,

    /// Everything `conflicts_with` and `conflicts_with_all` name, as clap ids.
    pub conflicts: Vec<String>,

    /// Everything `requires` and `requires_all` name, as clap ids.
    pub requires: Vec<String>,
}

impl ArgSpec {
    /// Read every `#[arg(...)]` on a field.
    pub fn read(attrs: &[Attribute]) -> Self {
        let mut spec = Self::default();

        for meta in metas(attrs, "arg") {
            match &meta {
                Meta::Path(path) if is(path, "skip") => spec.skip = true,
                Meta::Path(path) if is(path, "value_enum") => spec.value_enum = true,
                Meta::Path(path) if is(path, "long") => spec.has_long = true,
                Meta::NameValue(nv) if is(&nv.path, "global") => {
                    spec.global = render(&nv.value) == "true";
                }
                Meta::NameValue(nv) if is(&nv.path, "long") => {
                    spec.has_long = true;
                    spec.long = Some(text(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "visible_alias") => {
                    spec.aliases.push(text(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "visible_aliases") => {
                    spec.aliases.extend(texts(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "value_name") => {
                    spec.value_name = Some(text(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "default_value") => {
                    spec.default = Some(DefaultValue::Text(text(&nv.value)));
                }
                Meta::NameValue(nv) if is(&nv.path, "default_value_t") => {
                    spec.default = Some(DefaultValue::Rust(nv.value.clone()));
                }
                Meta::NameValue(nv) if is(&nv.path, "value_parser") => {
                    spec.value_parser = Some(last_segment(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "group") => {
                    spec.group = Some(text(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "conflicts_with") => {
                    spec.conflicts.push(text(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "conflicts_with_all") => {
                    spec.conflicts.extend(texts(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "requires") => {
                    spec.requires.push(text(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "requires_all") => {
                    spec.requires.extend(texts(&nv.value));
                }
                Meta::NameValue(nv) if is(&nv.path, "help") => {
                    spec.help = Some(match as_string(&nv.value) {
                        Some(literal) => Help::Text(literal),
                        None => Help::Named(last_segment(&nv.value)),
                    });
                }
                _ => {}
            }
        }

        spec
    }
}

/// The `#[command(...)]` entries that place a command in the tree.
#[derive(Default)]
pub struct CommandSpec {
    /// An explicit `name = "..."`, overriding the variant name.
    pub name: Option<String>,

    /// `hide = true`, which keeps the command out of `--help`.  A description
    /// nobody can find in the CLI has no business appearing in a menu.
    pub hide: bool,

    /// `#[command(subcommand)]`, which makes the field a branch of the tree.
    pub subcommand: bool,

    /// Every `group = ArgGroup::new(...)` the struct declares.
    pub groups: Vec<GroupSpec>,
}

/// One `ArgGroup`, as the builder chain states it.
pub struct GroupSpec {
    /// The name `ArgGroup::new` was given.
    pub name: String,

    /// What `args([...])` listed, which is empty where the chain omits it.
    pub members: Vec<String>,

    /// What `required(...)` said, defaulting to clap's own `false`.
    pub required: bool,

    /// What `multiple(...)` said, defaulting to clap's own `false`.
    pub multiple: bool,
}

impl CommandSpec {
    /// Read every `#[command(...)]` on a variant, field or struct.
    pub fn read(attrs: &[Attribute]) -> Self {
        let mut spec = Self::default();

        for meta in metas(attrs, "command") {
            match &meta {
                Meta::Path(path) if is(path, "subcommand") => spec.subcommand = true,
                Meta::NameValue(nv) if is(&nv.path, "name") => spec.name = Some(text(&nv.value)),
                Meta::NameValue(nv) if is(&nv.path, "hide") => {
                    spec.hide = render(&nv.value) == "true";
                }
                Meta::NameValue(nv) if is(&nv.path, "group") => {
                    spec.groups.push(arg_group(&nv.value));
                }
                _ => {}
            }
        }

        spec
    }
}

/// Read an `ArgGroup::new("...").required(...).args([...])` chain.
///
/// The chain is read from the outside in, so the calls arrive in reverse.  That
/// does not matter - each one sets a different thing, and `ArgGroup::new` is
/// what the walk stops at.
fn arg_group(expr: &Expr) -> GroupSpec {
    let mut spec = GroupSpec {
        name: String::new(),
        members: Vec::new(),
        required: false,
        multiple: false,
    };
    let mut current = expr;

    loop {
        match current {
            Expr::MethodCall(call) => {
                let argument = call
                    .args
                    .first()
                    .unwrap_or_else(|| panic!("ArgGroup::{}() was given nothing", call.method));
                match call.method.to_string().as_str() {
                    "required" => spec.required = render(argument) == "true",
                    "multiple" => spec.multiple = render(argument) == "true",
                    "args" => spec.members = texts(argument),
                    other => panic!("ArgGroup::{other}() is not read by this description"),
                }
                current = &call.receiver;
            }
            Expr::Call(call) => {
                spec.name = text(
                    call.args
                        .first()
                        .expect("ArgGroup::new takes the group's name"),
                );
                return spec;
            }
            other => panic!("expected an ArgGroup, got {}", render(other)),
        }
    }
}

/// Whether a `#[derive(...)]` names a particular trait.
pub fn derives(attrs: &[Attribute], trait_name: &str) -> bool {
    metas(attrs, "derive")
        .iter()
        .any(|meta| matches!(meta, Meta::Path(path) if is(path, trait_name)))
}

/// The doc comment, split into paragraphs the way clap splits it.
///
/// Wrapped prose is joined back into one line, since where the source wrapped
/// says nothing about where a pane should.  An indented line keeps its own
/// break: those are the worked examples, and running them together destroys
/// them.
pub fn paragraphs(attrs: &[Attribute]) -> Vec<String> {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in doc_lines(attrs) {
        let line = line.trim_end();
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        } else if current.is_empty() {
            current.push_str(line.trim_start());
        } else if line.starts_with(' ') {
            current.push('\n');
            current.push_str(line);
        } else {
            current.push(' ');
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        paragraphs.push(current);
    }

    paragraphs
}

/// One `/// ...` line per entry, with the space clap strips already stripped.
fn doc_lines(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(nv) => as_string(&nv.value),
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_string())
        .collect()
}

/// The comma-separated entries of every attribute with a given name.
fn metas(attrs: &[Attribute], name: &str) -> Vec<Meta> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident(name))
        .flat_map(|attr| {
            attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .unwrap_or_else(|e| {
                    panic!("could not read #[{name}(...)]: {e}");
                })
        })
        .collect()
}

fn is(path: &syn::Path, name: &str) -> bool {
    path.is_ident(name)
}

/// A string literal's contents, insisting the entry really is one.
fn text(expr: &Expr) -> String {
    as_string(expr).unwrap_or_else(|| panic!("expected a string, got {}", render(expr)))
}

/// The contents of an array of string literals.
fn texts(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Array(array) => array.elems.iter().map(text).collect(),
        _ => panic!("expected an array of strings, got {}", render(expr)),
    }
}

/// A string literal's contents, where the expression is one.
pub fn as_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(lit) => match &lit.lit {
            Lit::Str(text) => Some(text.value()),
            _ => None,
        },
        _ => None,
    }
}

/// The last segment of a path expression, which names a parser or a `const`.
fn last_segment(expr: &Expr) -> String {
    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .expect("a path has at least one segment")
            .ident
            .to_string(),
        _ => render(expr),
    }
}

/// An expression as written, for a default clap states as Rust rather than as
/// a string.
pub fn render(expr: &Expr) -> String {
    tokens(expr.to_token_stream())
}

/// Token text with the spacing `to_string` leaves between punctuation removed.
pub fn tokens(stream: TokenStream) -> String {
    stream
        .to_string()
        .replace(" :: ", "::")
        .replace(" < ", "<")
        .replace(" > ", ">")
        .replace(" , ", ", ")
}

/// A variant name as clap spells it on the command line.
pub fn kebab_variant(name: &str) -> String {
    let mut out = String::new();
    for (index, character) in name.char_indices() {
        if character.is_uppercase() && index != 0 {
            out.push('-');
        }
        out.extend(character.to_lowercase());
    }
    out
}

/// A field name as clap spells it, once the raw-identifier prefix is gone.
pub fn kebab_field(name: &str) -> String {
    name.trim_start_matches("r#").replace('_', "-")
}
