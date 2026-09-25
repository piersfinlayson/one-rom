// src/layout.rs
//
// Lays out both the working schema and the copy of the last release, compares
// them field by field, and refuses a layout that moved without its hand-raised
// generation number moving with it.
//
// The copy also answers which release a plugin-facing constant or metadata key
// arrived in.  `[schema] firmware_release` names the release under
// development, so everything already in a plugin author's hands names an older
// one, and only the copy separates the two.

use std::collections::{BTreeMap, BTreeSet};

use crate::released::Released;
use crate::schema::{
    ConstantValue, FieldShape, NamedSizes, Schema, release_parts, shape_size, shape_type,
};

/// The release in which `[schema] firmware_release` arrived, so a copy without
/// one was taken before it.
const FIRST_RELEASE_NAMING_ITSELF: (u32, u32, u32) = (0, 8, 0);

// ---------------------------------------------------------------------------
// The layout model
// ---------------------------------------------------------------------------

/// One field's place in a structure.
#[derive(Debug, PartialEq, Eq)]
pub struct FieldLayout {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    /// The field's type as the comparison reads it, e.g. "scalar u32".
    pub described: String,
    /// Reserved bytes nothing reads. Where a field is added to a structure of
    /// fixed size, these are what it is taken out of, so the comparison holds
    /// them to nothing - see `check_fields`.
    pub padding: bool,
}

/// One structure's fields, in declaration order.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct TypeLayout {
    pub fields: Vec<FieldLayout>,
}

/// Every structure a schema describes, and every constant it declares.
///
/// A tagged FAM contributes one entry for its fixed portion under its own
/// name, and one per variant under `fam::VARIANT`, because a variant's
/// parameter struct has a layout an older reader can misparse the same way.
#[derive(Debug, Default)]
pub struct Layout {
    pub types: BTreeMap<String, TypeLayout>,
    pub constants: BTreeMap<String, String>,
}

impl Layout {
    /// Lay one type out, in declaration order.
    fn add_type(&mut self, name: String, shapes: &[FieldShape], named: &dyn NamedSizes) {
        let mut offset = 0;
        let mut fields = Vec::with_capacity(shapes.len());
        for shape in shapes {
            let size = shape_size(shape, named);
            fields.push(FieldLayout {
                name: shape.name.to_string(),
                offset,
                size,
                described: shape_type(shape),
                padding: shape.kind == "padding",
            });
            offset += size;
        }
        self.types.insert(name, TypeLayout { fields });
    }

    /// The name a tagged FAM variant's parameter struct is laid out under.
    fn variant_name(fam: &str, discriminant: &str) -> String {
        format!("{fam}::{discriminant}")
    }
}

/// The discriminant and length fields a tagged FAM's fixed portion opens with,
/// ahead of its common fields.  They are real C fields, and c_gen.rs emits
/// them in this order.
fn fam_prefix<'a>(
    discriminant_field: &'a str,
    discriminant_type: &'a str,
    param_len_field: &'a str,
    param_len_type: &'a str,
) -> [FieldShape<'a>; 2] {
    [
        FieldShape {
            name: discriminant_field,
            kind: "enum",
            type_: Some(discriminant_type),
            element: None,
            count: None,
            rows: None,
            cols: None,
            size: None,
        },
        FieldShape {
            name: param_len_field,
            kind: "scalar",
            type_: Some(param_len_type),
            element: None,
            count: None,
            rows: None,
            cols: None,
            size: None,
        },
    ]
}

/// A constant's value as text, so two schemas' values compare as written.
fn constant_text(value: &ConstantValue) -> String {
    match value {
        ConstantValue::Integer(n) => n.to_string(),
        ConstantValue::Text(s) => s.clone(),
    }
}

// The two constructors sit together because they have to agree: a difference
// between them reads as a layout that moved when nothing had.

impl Layout {
    /// Lay out the working schema.
    pub fn of_schema(schema: &Schema) -> Self {
        let mut layout = Self::default();

        for s in &schema.structs {
            let shapes: Vec<FieldShape> = s.fields.iter().map(|f| f.shape()).collect();
            layout.add_type(s.name.clone(), &shapes, schema);
        }

        for fam in &schema.tagged_fams {
            let mut shapes: Vec<FieldShape> = fam_prefix(
                &fam.discriminant_field,
                &fam.discriminant_type,
                &fam.param_len_field,
                fam.param_len_type(),
            )
            .into_iter()
            .collect();
            shapes.extend(fam.common_fields.iter().map(|f| f.shape()));
            layout.add_type(fam.name.clone(), &shapes, schema);

            for v in &fam.variants {
                let shapes: Vec<FieldShape> = v.fields.iter().map(|f| f.shape()).collect();
                layout.add_type(
                    Self::variant_name(&fam.name, &v.discriminant),
                    &shapes,
                    schema,
                );
            }
        }

        for c in &schema.constants {
            layout
                .constants
                .insert(c.name.clone(), constant_text(&c.value));
        }

        layout
    }

    /// Lay out the last released schema.
    pub fn of_released(released: &Released) -> Self {
        let mut layout = Self::default();

        for s in &released.structs {
            let shapes: Vec<FieldShape> = s.fields.iter().map(|f| f.shape()).collect();
            layout.add_type(s.name.clone(), &shapes, released);
        }

        for fam in &released.tagged_fams {
            let mut shapes: Vec<FieldShape> = fam_prefix(
                &fam.discriminant_field,
                &fam.discriminant_type,
                &fam.param_len_field,
                fam.param_len_type.as_deref().unwrap_or("u8"),
            )
            .into_iter()
            .collect();
            shapes.extend(fam.common_fields.iter().map(|f| f.shape()));
            layout.add_type(fam.name.clone(), &shapes, released);

            for v in &fam.variants {
                let shapes: Vec<FieldShape> = v.fields.iter().map(|f| f.shape()).collect();
                layout.add_type(
                    Self::variant_name(&fam.name, &v.discriminant),
                    &shapes,
                    released,
                );
            }
        }

        for c in &released.constants {
            layout
                .constants
                .insert(c.name.clone(), constant_text(&c.value));
        }

        layout
    }
}

// ---------------------------------------------------------------------------
// The comparison
// ---------------------------------------------------------------------------

/// Refuse a working schema that has moved away from the last released one
/// without saying so.
///
/// Seven things are checked:
///
/// - a plugin-facing constant or metadata key names the wrong release as the
///   one it arrived in
/// - a constant's value changed, or the constant is gone
/// - an enum value changed its number or its name, or is gone
/// - a shipped field moved, changed size, changed type, or is gone
/// - a shipped variant of a standalone family gained a field
/// - a structure's tree changed shape while its generation stayed put
/// - a generation moved by anything other than one step forward, or moved
///   without a `[[versions]]` entry naming the release it ships in
///
/// A field or a structure that is simply new is the ordinary case and passes,
/// subject to the marker rules [`Schema::parse`] enforces.
pub fn compare(current: &Schema, released: &Released) -> Result<(), Box<dyn std::error::Error>> {
    if released.unreleased() {
        return check_nothing_released(released);
    }
    check_release_order(current, released)?;
    check_first_releases(current, released)?;

    let now = Layout::of_schema(current);
    let then = Layout::of_released(released);

    check_constants(current, &now, &then)?;
    check_enums(current, released)?;
    let changed = check_fields(&now, &then)?;
    check_standalone(current, released, &now, &then)?;
    check_generations(current, released, &changed)
}

/// A copy declaring no release describes nothing, so anything in it beside
/// that declaration is a contradiction and the comparison it asks to be
/// skipped is the one that catches a layout moving.
fn check_nothing_released(released: &Released) -> Result<(), Box<dyn std::error::Error>> {
    let described = !released.structs.is_empty()
        || !released.constants.is_empty()
        || !released.enums.is_empty()
        || !released.type_aliases.is_empty()
        || !released.tagged_fams.is_empty();
    if described {
        return Err(
            "the released schema says unreleased and describes a layout - one release \
                    has shipped or none has"
                .into(),
        );
    }
    Ok(())
}

/// The constants holding the versioned structures' generations.
///
/// They are the one set meant to change, and [`check_generations`] holds them
/// instead - to a rise of exactly one, alongside a `[[versions]]` entry naming
/// the release that generation ships in.
fn generation_constants(current: &Schema) -> BTreeSet<&str> {
    current
        .versioned_structs()
        .into_iter()
        .filter_map(|name| current.generation_constant(name))
        .map(|(constant, _)| constant)
        .collect()
}

/// The release the copy is of, as a message says it.
///
/// A copy taken before `[schema] firmware_release` existed names none, so all
/// that can be said is that it predates the key.
fn copy_release(released: &Released) -> String {
    match released.firmware_release() {
        Some(release) => release.to_string(),
        None => {
            let (major, minor, patch) = FIRST_RELEASE_NAMING_ITSELF;
            format!("a release before {major}.{minor}.{patch}")
        }
    }
}

/// Check that the copy really is of an earlier release than this schema.
///
/// The copy is refreshed by hand, once per development cycle, alongside the
/// bump to `[schema] firmware_release`.  Refreshing it and not bumping leaves
/// the build comparing the working schema with itself, which passes whatever
/// has changed.
fn check_release_order(
    current: &Schema,
    released: &Released,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = release_parts(&current.schema.firmware_release).ok_or_else(|| {
        format!(
            "[schema] firmware_release is '{}', which is not a firmware release like 0.8.0",
            current.schema.firmware_release
        )
    })?;

    match released.firmware_release() {
        None => {
            if now < FIRST_RELEASE_NAMING_ITSELF {
                let (major, minor, patch) = FIRST_RELEASE_NAMING_ITSELF;
                return Err(format!(
                    "the released schema names no firmware_release, so it was taken before \
                     {major}.{minor}.{patch}, and this schema says {}",
                    current.schema.firmware_release
                )
                .into());
            }
        }
        Some(was) => {
            let was_parts = release_parts(was).ok_or_else(|| {
                format!(
                    "the released schema's firmware_release is '{was}', which is not a firmware \
                     release like 0.8.0"
                )
            })?;
            if was_parts >= now {
                return Err(format!(
                    "the released schema names {was} and this schema names {}, so there is no \
                     released layout to compare against - raise [schema] firmware_release, or \
                     refresh the copy with ci/update-released-schema.sh",
                    current.schema.firmware_release
                )
                .into());
            }
        }
    }
    Ok(())
}

/// What the copy of the last release said about when one plugin-facing item
/// arrived.
enum Shipped<'a> {
    /// Not in the copy's plugin API at all, so it arrives now.
    Absent,
    /// In it, naming the release it arrived in.
    Named(&'a str),
    /// In it, from before the copy said when anything arrived.  All it
    /// establishes is that the item was already there.
    Unstated,
}

/// Read a lookup in the copy as one of the three cases.
fn shipped<'a>(found: Option<&Option<&'a str>>) -> Shipped<'a> {
    match found {
        None => Shipped::Absent,
        Some(None) => Shipped::Unstated,
        Some(Some(release)) => Shipped::Named(release),
    }
}

/// Refuse a `first_release` that does not say what the last release did.
///
/// An `@since` line in the plugin headers is what a plugin author sets
/// min_fw_version from, and the firmware refuses a plugin asking for newer
/// than itself.  Get it wrong and either a plugin that would have run is
/// refused, or one is let in against firmware without what it uses.
fn check_first_releases(
    current: &Schema,
    released: &Released,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = current.schema.firmware_release.as_str();
    let copy = copy_release(released);

    let constants: BTreeMap<&str, Option<&str>> = released
        .ora_constants()
        .map(|c| (c.name.as_str(), c.first_release.as_deref()))
        .collect();
    for c in current.ora_constants() {
        // Schema::parse refuses an ora_api constant carrying none, so this
        // walk has nothing to say about one.
        let Some(names) = c.first_release.as_deref() else {
            continue;
        };
        check_arrival(
            &format!("constant {}", c.name),
            names,
            shipped(constants.get(c.name.as_str())),
            now,
            &copy,
        )?;
    }

    let keys: BTreeMap<&str, Option<&str>> = released
        .plugin_keys()
        .map(|k| (k.name.as_str(), k.first_release.as_deref()))
        .collect();
    for entry in current.plugin_keys() {
        check_arrival(
            &format!("plugin key {}", entry.key.name),
            &entry.key.first_release,
            shipped(keys.get(entry.key.name.as_str())),
            now,
            &copy,
        )?;
    }
    Ok(())
}

/// Hold one item's `first_release` to what the copy says of it.  The three
/// cases are three different mistakes, so each says which it is.
fn check_arrival(
    what: &str,
    names: &str,
    was: Shipped<'_>,
    now: &str,
    copy: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match was {
        Shipped::Absent => {
            if names != now {
                return Err(format!(
                    "{what} is not in the copy of the last release, so it reaches the plugin API \
                     in {now} - its first_release says {names}"
                )
                .into());
            }
        }
        Shipped::Named(was) => {
            if names != was {
                return Err(format!(
                    "{what} named {was} in the last release and names {names} now - the release \
                     something reached the plugin API in is a fact about firmware already \
                     shipped, and a plugin built against it set its min_fw_version from that"
                )
                .into());
            }
        }
        Shipped::Unstated => {
            if names == now {
                return Err(format!(
                    "{what} was already in the plugin API of {copy} and names {now}, the release \
                     under development - it reached the plugin API before that, and the copy \
                     says no more of it than that it was there"
                )
                .into());
            }
        }
    }
    Ok(())
}

/// Refuse a constant whose value has moved, or that is gone.
///
/// A constant is a value the firmware, a plugin and a host agree on, and none
/// of them rebuilds because this file changed.  A constant used as an array
/// dimension trips the layout rule as well, so the generation bump that covers
/// the layout does not swallow the constant with it.
///
/// Removing one is the same event: everything already built against it names
/// it still.  A constant is retired by `deprecated_release` instead, which
/// leaves it here with its value and tells a reader of the generated header
/// not to reach for it.
///
/// The generation constants are the only exception, and
/// [`check_generations`] holds them to a tighter rule.
fn check_constants(
    current: &Schema,
    now: &Layout,
    then: &Layout,
) -> Result<(), Box<dyn std::error::Error>> {
    let generations = generation_constants(current);
    for (name, was) in &then.constants {
        let Some(is) = now.constants.get(name) else {
            return Err(format!(
                "constant {name} was in the last release and is gone - the firmware, its plugins \
                 and every host built against it name it still, so a constant is deprecated with \
                 deprecated_release rather than removed"
            )
            .into());
        };
        if generations.contains(name.as_str()) {
            continue;
        }
        if is != was {
            return Err(format!(
                "constant {name} was {was} in the last release and is {is} now - a constant is a \
                 value the firmware, its plugins and every host agree on, and none of them \
                 rebuilds because this file changed"
            )
            .into());
        }
    }
    Ok(())
}

/// Refuse an enum value whose number or name has changed since the last
/// release, or which is gone.
///
/// Devices and hosts already hold the numbers.  An enum whose values come from
/// elsewhere, such as `onerom_rom_type_t` from `chip-types.json`, lists none in
/// the copy, so nothing here checks it.
fn check_enums(current: &Schema, released: &Released) -> Result<(), Box<dyn std::error::Error>> {
    for was in released.enums.iter().filter(|e| !e.variants.is_empty()) {
        let Some(is) = current.enums.iter().find(|e| e.name == was.name) else {
            return Err(format!(
                "enum {} was in the last release and is gone - devices and hosts hold its \
                 values still",
                was.name
            )
            .into());
        };
        for old in &was.variants {
            match is.variants.iter().find(|v| v.name == old.name) {
                Some(new) if new.value == old.value => {}
                Some(new) => {
                    return Err(format!(
                        "{}::{} was {} in the last release and is {} now - a released value \
                         keeps its number, because devices and hosts hold it",
                        was.name, old.name, old.value, new.value
                    )
                    .into());
                }
                None => {
                    let renamed = is.variants.iter().find(|v| v.value == old.value);
                    return Err(match renamed {
                        Some(new) => format!(
                            "{}::{} is called {} now - a released value keeps its name, and an \
                             alias gives it another",
                            was.name, old.name, new.name
                        ),
                        None => format!(
                            "{}::{} was in the last release and is gone - a released value is \
                             deprecated with deprecated_release rather than removed",
                            was.name, old.name
                        ),
                    }
                    .into());
                }
            }
        }
    }
    Ok(())
}

/// Hold each standalone family to the rule that replaces a generation for it.
///
/// Every entry carries its length, so a reader skips a variant it has no name
/// for, and a new variant is free.  A shipped variant never changes, because a
/// reader built against it reads its fields at fixed offsets.  The part every
/// variant shares is held the same way.  A family also keeps whether it is
/// standalone, since that decides which rule its changes answer to.
///
/// [`check_fields`] has already refused a shipped field that moved, changed or
/// went, so what is left for this to find is a field added.
fn check_standalone(
    current: &Schema,
    released: &Released,
    now: &Layout,
    then: &Layout,
) -> Result<(), Box<dyn std::error::Error>> {
    for fam in &current.tagged_fams {
        if let Some(was) = released.tagged_fams.iter().find(|t| t.name == fam.name)
            && was.standalone != fam.standalone
        {
            let word = |standalone: bool| match standalone {
                true => "standalone",
                false => "not standalone",
            };
            return Err(format!(
                "{} was {} in the last release and is {} now - whether a family is standalone \
                 decides which rule its changes answer to",
                fam.name,
                word(was.standalone),
                word(fam.standalone)
            )
            .into());
        }
        if !fam.standalone {
            continue;
        }

        let shipped = std::iter::once(fam.name.clone()).chain(
            fam.variants
                .iter()
                .map(|v| Layout::variant_name(&fam.name, &v.discriminant)),
        );
        for name in shipped {
            let (Some(was), Some(is)) = (then.types.get(&name), now.types.get(&name)) else {
                continue;
            };
            if is == was {
                continue;
            }
            let change = match is
                .fields
                .iter()
                .find(|f| !was.fields.iter().any(|old| old.name == f.name))
            {
                Some(added) => format!("gained {}", added.name),
                None => "changed".to_string(),
            };
            return Err(format!(
                "{name} has {change} since the last release, and its family is standalone - a \
                 shipped variant never changes, because a reader built against it reads its \
                 fields at fixed offsets"
            )
            .into());
        }
    }
    Ok(())
}

/// Refuse a shipped field that moved, and report which types changed shape.
///
/// A field that has shipped is never moved, renamed or removed: its bytes are
/// where an older host reads them, and that host has no way to find out
/// otherwise.  A field that is merely new is the ordinary case.
///
/// Padding is the exception.  These structures are of fixed size, so a new
/// field's bytes come out of the reserved padding beside it and that padding
/// shrinks, moves or goes entirely.  Nothing reads a padding field, so none of
/// that is visible to a host, and a real field pushed along by it is still
/// caught by its own offset.
///
/// The names returned are the types whose layout differs at all, new ones
/// included - what [`check_generations`] wants a generation bump for.
fn check_fields(
    now: &Layout,
    then: &Layout,
) -> Result<BTreeSet<String>, Box<dyn std::error::Error>> {
    for (name, was) in &then.types {
        let Some(is) = now.types.get(name) else {
            return Err(format!(
                "{name} was in the last release and is no longer described - a shipped structure's \
                 bytes stay where a host that knows them can read them"
            )
            .into());
        };

        for old in &was.fields {
            if old.padding {
                continue;
            }
            let Some(new) = is.fields.iter().find(|f| f.name == old.name) else {
                return Err(format!(
                    "{name}.{} was in the last release and is gone - a shipped field is never \
                     moved, renamed or removed, only deprecated",
                    old.name
                )
                .into());
            };
            if new.offset != old.offset {
                return Err(format!(
                    "{name}.{} was at offset {} in the last release and is at {} now - a shipped \
                     field's bytes stay where they are",
                    old.name, old.offset, new.offset
                )
                .into());
            }
            if new.size != old.size {
                return Err(format!(
                    "{name}.{} was {} byte(s) in the last release and is {} now - a shipped \
                     field's bytes stay where they are",
                    old.name, old.size, new.size
                )
                .into());
            }
            if new.described != old.described {
                return Err(format!(
                    "{name}.{} was {} in the last release and is {} now - a shipped field is read \
                     the way it was written",
                    old.name, old.described, new.described
                )
                .into());
            }
        }
    }

    let mut changed = BTreeSet::new();
    for (name, is) in &now.types {
        if then.types.get(name) != Some(is) {
            changed.insert(name.clone());
        }
    }
    Ok(changed)
}

/// Refuse a generation that has not kept up with the layout it stands for.
///
/// The generation governs a whole tree, not one structure, so a field added to
/// anything hanging off a root moves that root's generation.  Which root owns
/// which type is [`Schema::ownership`]'s answer.
fn check_generations(
    current: &Schema,
    released: &Released,
    changed: &BTreeSet<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let owners = current.ownership();
    let standalone: BTreeSet<&str> = current
        .tagged_fams
        .iter()
        .filter(|t| t.standalone)
        .map(|t| t.name.as_str())
        .collect();

    // The first changed type under each root, so the message names something
    // to go and look at rather than a count.
    let mut changed_trees: BTreeMap<&str, &str> = BTreeMap::new();
    for name in changed {
        // A variant is laid out under its FAM's name, and belongs to whatever
        // owns the FAM.
        let owned = name.split("::").next().unwrap_or(name);
        // A standalone family answers to check_standalone instead.
        if standalone.contains(owned) {
            continue;
        }
        let Some(root) = owners.get(owned) else {
            return Err(format!(
                "{name}'s layout has changed since the last release, and it sits under none of \
                 the versioned structures, so no generation number covers it"
            )
            .into());
        };
        changed_trees.entry(root).or_insert(name);
    }

    for name in current.versioned_structs() {
        let Some((constant, is)) = current.generation_constant(name) else {
            return Err(format!(
                "{name} carries a generation number and names no constant holding it"
            )
            .into());
        };

        // A structure the last release did not describe has no earlier
        // generation to have moved on from.
        let Some(ConstantValue::Integer(was)) = released.constant(constant) else {
            continue;
        };
        let was = u32::try_from(*was).map_err(|_| {
            format!("{constant} was {was} in the last release, which is not a generation number")
        })?;

        match is.checked_sub(was) {
            Some(0) => {
                if let Some(what) = changed_trees.get(name) {
                    return Err(format!(
                        "{name} is still generation {was}, and {what}'s layout has changed since \
                         the last release - raise {constant} to {}, and add a [[versions]] entry \
                         saying which release that generation ships in",
                        was + 1
                    )
                    .into());
                }
            }
            Some(1) => {
                if !current
                    .versions
                    .iter()
                    .any(|v| v.struct_name == name && v.version == is)
                {
                    return Err(format!(
                        "{constant} was raised to {is} with no [[versions]] entry for {name} \
                         generation {is} - a generation says nothing until something says which \
                         release it ships in"
                    )
                    .into());
                }
            }
            Some(_) => {
                return Err(format!(
                    "{constant} was {was} in the last release and is {is} now - a generation \
                     rises by one per release, so that a host holding a device's bytes can name \
                     every layout in between.  The copy compared against is of {}, and this \
                     schema is {} - a copy left unrefreshed across a development cycle reads as \
                     a rise of more than one, and ci/update-released-schema.sh refreshes it",
                    copy_release(released),
                    current.schema.firmware_release
                )
                .into());
            }
            None => {
                return Err(format!(
                    "{constant} was {was} in the last release and is {is} now - a generation rises \
                     by one per release, so that a host holding a device's bytes can name every \
                     layout in between"
                )
                .into());
            }
        }
    }
    Ok(())
}
