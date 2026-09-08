// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! The CLI's argument definitions, parsed once and looked up by name.
//!
//! clap's derive spreads one command tree over a dozen files, and a variant
//! names the struct that holds its options with no idea where that struct
//! lives.  Indexing every item by name up front is what lets the walk follow
//! those names without caring which file each landed in.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use syn::{Expr, Item, ItemEnum, ItemStruct};

/// Every struct, enum and `const` the argument definitions declare.
pub struct Index {
    structs: BTreeMap<String, ItemStruct>,
    enums: BTreeMap<String, ItemEnum>,
    consts: BTreeMap<String, Expr>,
}

impl Index {
    /// Read every `.rs` file in a directory, without descending into it.
    ///
    /// A name declared twice is a hard error rather than a silent overwrite -
    /// the walk resolves by name alone, so two structs called the same thing
    /// would make it pick one at random.
    pub fn read_dir(dir: &Path) -> Self {
        let mut index = Self {
            structs: BTreeMap::new(),
            enums: BTreeMap::new(),
            consts: BTreeMap::new(),
        };

        let mut paths: Vec<_> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("could not read {}: {e}", dir.display()))
            .map(|entry| entry.expect("could not read a directory entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
            .collect();
        paths.sort();

        for path in paths {
            for item in parse(&path).items {
                index.add(item, &path);
            }
        }

        index
    }

    fn add(&mut self, item: Item, path: &Path) {
        let where_from = path.display();
        match item {
            Item::Struct(item) => {
                let name = item.ident.to_string();
                if self.structs.insert(name.clone(), item).is_some() {
                    panic!("struct {name} is declared twice, the second time in {where_from}");
                }
            }
            Item::Enum(item) => {
                let name = item.ident.to_string();
                if self.enums.insert(name.clone(), item).is_some() {
                    panic!("enum {name} is declared twice, the second time in {where_from}");
                }
            }
            Item::Const(item) => {
                self.consts.insert(item.ident.to_string(), *item.expr);
            }
            _ => {}
        }
    }

    /// The struct of that name, which every clap variant is expected to name.
    pub fn struct_def(&self, name: &str) -> &ItemStruct {
        self.structs
            .get(name)
            .unwrap_or_else(|| panic!("no struct {name} in the argument definitions"))
    }

    /// The enum of that name, which a `#[command(subcommand)]` field names.
    pub fn enum_def(&self, name: &str) -> &ItemEnum {
        self.enums
            .get(name)
            .unwrap_or_else(|| panic!("no enum {name} in the argument definitions"))
    }

    /// Every enum declared, for finding the ones clap takes a value set from.
    pub fn enums(&self) -> impl Iterator<Item = (&String, &ItemEnum)> {
        self.enums.iter()
    }

    /// What a `const` was assigned, for an option whose help is one.
    pub fn const_value(&self, name: &str) -> &Expr {
        self.consts
            .get(name)
            .unwrap_or_else(|| panic!("no const {name} in the argument definitions"))
    }
}

/// Parse one Rust file, saying which file failed rather than where in it.
pub fn parse(path: &Path) -> syn::File {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    syn::parse_file(&text).unwrap_or_else(|e| panic!("could not parse {}: {e}", path.display()))
}
