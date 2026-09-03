// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The value sets the CLI itself advertises.
//!
//! Almost every One ROM option takes a value out of a set only the running
//! program knows - a board name, a chip type, a pin.  A handful state their
//! values where clap can print them in `--help`, and those are the only ones
//! a pane can safely turn into a picker.
//!
//! Two of them are written outside `rust/cli/src/args/`, so this module reads
//! those two files as well rather than repeating the values here.  A copy is
//! what goes stale.

use std::collections::BTreeMap;
use std::path::Path;

use syn::{Expr, ImplItem, Item, ItemEnum};

use crate::attrs::{as_string, derives, kebab_variant};
use crate::index::{self, Index};

/// Every fixed value set, by the type or the parser that advertises it.
pub struct ValueSets {
    types: BTreeMap<String, Vec<String>>,
    parsers: BTreeMap<String, Vec<String>>,
}

impl ValueSets {
    /// Read the sets the argument definitions declare, plus the two they
    /// borrow from elsewhere in the tree.
    pub fn read(index: &Index, log_level: &Path, file_format: &Path) -> Self {
        let mut types = BTreeMap::new();

        for (name, item) in index.enums() {
            if derives(&item.attrs, "ValueEnum") {
                types.insert(name.clone(), variants(item));
            }
        }

        let log_level = value_enum(log_level, "LogLevel");
        types.insert("LogLevel".to_string(), variants(&log_level));

        let mut parsers = BTreeMap::new();
        parsers.insert(
            "ImageFormatParser".to_string(),
            file_format_names(file_format),
        );

        Self { types, parsers }
    }

    /// The values an option advertises, where it advertises any.
    pub fn of(&self, type_name: &str, parser: Option<&str>) -> Option<&[String]> {
        parser
            .and_then(|parser| self.parsers.get(parser))
            .or_else(|| self.types.get(type_name))
            .map(Vec::as_slice)
    }
}

/// A `ValueEnum`'s variants, spelled the way clap spells them.
fn variants(item: &ItemEnum) -> Vec<String> {
    item.variants
        .iter()
        .map(|variant| kebab_variant(&variant.ident.to_string()))
        .collect()
}

/// A `ValueEnum` declared outside the argument definitions.
fn value_enum(path: &Path, name: &str) -> ItemEnum {
    index::parse(path)
        .items
        .into_iter()
        .find_map(|item| match item {
            Item::Enum(item) if item.ident == name && derives(&item.attrs, "ValueEnum") => {
                Some(item)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no ValueEnum {name} in {}", path.display()))
}

/// The format names `--from` and `--to` accept.
///
/// The parser builds its possible values by mapping `FileFormat::name`, so the
/// arms of that method are where the spellings are written.  Reading the enum's
/// own variant names instead would give `intel-hex` where the CLI says `ihex`.
fn file_format_names(path: &Path) -> Vec<String> {
    let file = index::parse(path);

    for item in file.items {
        let Item::Impl(block) = item else { continue };
        if !matches!(&*block.self_ty, syn::Type::Path(ty) if ty.path.is_ident("FileFormat")) {
            continue;
        }

        for item in block.items {
            let ImplItem::Fn(function) = item else {
                continue;
            };
            if function.sig.ident != "name" {
                continue;
            }
            return match_arm_strings(&function.block);
        }
    }

    panic!("no FileFormat::name in {}", path.display());
}

/// Every string a method's single `match` returns, in the order written.
fn match_arm_strings(block: &syn::Block) -> Vec<String> {
    for statement in &block.stmts {
        let syn::Stmt::Expr(Expr::Match(matched), _) = statement else {
            continue;
        };
        return matched
            .arms
            .iter()
            .filter_map(|arm| as_string(&arm.body))
            .collect();
    }

    panic!("expected a match returning strings");
}
