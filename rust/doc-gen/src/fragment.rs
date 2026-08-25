// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Filling in the fragment regions of a markdown file.
//!
//! A `docs/` file names another markdown file, and the assembled text is
//! written into the committed file between the markers:
//!
//! ```text
//! ## Recovery
//!
//! <!--[fragment:docs/fragments/recovery-steps.md]-->
//! ### Putting the device in the bootloader
//!
//! Hold BOOTSEL while applying power...
//! <!--[/]-->
//! ```
//!
//! So a reader on GitHub meets a whole document, with nothing to chase, and the
//! words are written once.
//!
//! The level is worked out rather than declared. The fragment's shallowest
//! heading sits one below the nearest heading above the marker, so
//! `recovery-steps.md`'s own `#` title lands at H3 under a `## Recovery`. The
//! same file embedded under an H1 lands at H2, and neither file needs a word
//! about the other. [`crate::assembly`] does the moving, because a fragment
//! region and a member of a PDF are the same problem.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::assembly::{self, Heading};
use crate::marker;

/// Where a fragment's shallowest heading sits when there is no heading above
/// the marker and the host has none of its own.
const TOP_LEVEL: usize = 1;

/// Where a fragment's text comes from.
///
/// A test supplies a map of documents, and the assembler supplies the working
/// tree, so every rule below is testable without a temporary directory.
pub trait Files {
    /// The text of one markdown file, named relative to the repository root.
    fn read(&self, path: &str) -> Result<String, String>;
}

/// The markdown files of a working tree.
pub struct Tree {
    root: PathBuf,
}

impl Tree {
    /// Read fragments from the tree rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Tree { root: root.into() }
    }
}

impl Files for Tree {
    fn read(&self, path: &str) -> Result<String, String> {
        let full = self.root.join(path);
        std::fs::read_to_string(&full)
            .map_err(|e| format!("could not read {}: {e}", full.display()))
    }
}

/// Fill in every fragment region in `text`, which was read from `path`.
///
/// `None` where the file carries no region, so a caller can tell a document
/// that has nothing to assemble from one whose regions are already current.
pub fn fill(path: &str, text: &str, files: &dyn Files) -> Result<Option<String>, String> {
    let mut chain = vec![path.to_string()];
    fill_regions(path, text, files, &mut chain)
}

/// Fill in the regions of one file, with `chain` holding the files being
/// resolved so a fragment that reaches itself is named rather than recursed on.
fn fill_regions(
    path: &str,
    text: &str,
    files: &dyn Files,
    chain: &mut Vec<String>,
) -> Result<Option<String>, String> {
    let scan = marker::scan(text);
    if let Some(problem) = scan.fragment_problems().next() {
        return Err(format!("{path}:{}: {}", problem.line, problem.detail));
    }
    if scan.fragments.is_empty() {
        return Ok(None);
    }

    let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let own = host_headings(&lines, &scan.fragments);

    let mut out: Vec<String> = Vec::new();
    let mut copied = 0;
    for region in &scan.fragments {
        // Up to and including the opening marker, which stays where it is.
        out.extend_from_slice(&lines[copied..=region.open]);

        let above = own.iter().rfind(|h| h.index < region.open);
        if region.peer && above.is_none() {
            return Err(format!(
                "{path}:{}: the marker says peer, but no heading sits above it \
                 to be a peer of",
                region.open + 1,
            ));
        }
        let target = match above {
            // A peer stands alongside the heading above it.  A child, the
            // default, is content of it.
            Some(heading) if region.peer => heading.level,
            Some(heading) => heading.level + 1,
            // Nothing above the marker, so the fragment opens the document and
            // its shallowest heading is one of the host's own sections.
            None => own.iter().map(|h| h.level).min().unwrap_or(TOP_LEVEL),
        };
        let why = match above {
            Some(heading) if region.peer => format!(
                "The marker says peer, so it stands alongside '{}' at \
                 {path}:{}, and its headings shift to open at H{target}.",
                heading.text,
                heading.index + 1,
            ),
            Some(heading) => format!(
                "It sits one level below '{}' at {path}:{}, so its headings \
                 shift to open at H{target}.",
                heading.text,
                heading.index + 1,
            ),
            None => format!(
                "Nothing in {path} heads the region, so its headings shift to \
                 open at H{target}, alongside the host's own sections."
            ),
        };

        out.extend(embed(path, region, target, &why, files, chain)?);
        copied = region.close;
    }
    out.extend_from_slice(&lines[copied..]);

    Ok(Some(out.join("\n")))
}

/// The host's own headings - the ones its author wrote, with every fragment
/// region taken out.
///
/// A heading that came from a fragment is not the host's. Letting one set the
/// level of a later region would make that region depend on what a previous run
/// wrote into the file, rather than on what its author put there.
fn host_headings(lines: &[String], regions: &[marker::Fragment]) -> Vec<Heading> {
    let mut own = lines.to_vec();
    for region in regions {
        for line in own.iter_mut().take(region.close + 1).skip(region.open) {
            line.clear();
        }
    }
    assembly::headings(&own).0
}

/// One region's text: the file it names, resolved and shifted to `target`.
fn embed(
    host: &str,
    region: &marker::Fragment,
    target: usize,
    why: &str,
    files: &dyn Files,
    chain: &mut Vec<String>,
) -> Result<Vec<String>, String> {
    if chain.contains(&region.path) {
        chain.push(region.path.clone());
        return Err(format!(
            "{host}:{}: '{}' is assembled from itself: {}",
            region.open + 1,
            region.path,
            chain.join(" -> ")
        ));
    }

    let text = files
        .read(&region.path)
        .map_err(|e| format!("{host}:{}: {e}", region.open + 1))?;

    // The named file may carry regions of its own - a published document such
    // as docs/OVERVIEW.md is a fragment like any other.  Resolving them here
    // rather than trusting what is committed means the host does not depend on
    // which file the assembler reached first.
    chain.push(region.path.clone());
    let resolved = fill_regions(&region.path, &text, files, chain)?.unwrap_or(text);
    chain.pop();

    let mut lines = embedded_lines(&resolved);
    let (found, _) = assembly::headings(&lines);
    let movable: Vec<&Heading> = found.iter().collect();
    let shift = assembly::shift_to(&movable, target);
    assembly::shift_headings(&region.path, &mut lines, &movable, shift, why)?;
    for line in &mut lines {
        *line = anchor_self_links(line, host, &region.path);
    }
    Ok(lines)
}

/// Turn a link that names the host into a bare anchor.
///
/// A fragment writes its links as they should read wherever it lands, so a link
/// to the manual is written as a path to the manual.  Embedded in the manual
/// that is a link out of the document and back in, which reads as an external
/// reference and, in a PDF, leaves the document.  Every other link is left
/// exactly as written - the host is a committed markdown file, so a repository
/// path resolves for the reader as it stands.
fn anchor_self_links(line: &str, host: &str, fragment_path: &str) -> String {
    let dir = fragment_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut out = String::new();
    let mut rest = line;

    while let Some(at) = rest.find("](") {
        let (before, from_bracket) = rest.split_at(at);
        out.push_str(before);
        let inside = &from_bracket[2..];
        let Some(close) = inside.find(')') else {
            out.push_str(from_bracket);
            return out;
        };
        let target = &inside[..close];
        out.push_str(&format!("]({})", anchor_self(target, dir, host)));
        rest = &inside[close + 1..];
    }

    out.push_str(rest);
    out
}

/// One link target, as an anchor where it names `host`.
fn anchor_self(target: &str, dir: &str, host: &str) -> String {
    if target.starts_with('#') || target.contains("://") || target.starts_with("mailto:") {
        return target.to_string();
    }
    let (path, anchor) = match target.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (target, None),
    };
    if path.is_empty() || assembly::resolve(path, dir) != host {
        return target.to_string();
    }
    match anchor {
        Some(anchor) => format!("#{anchor}"),
        // A link to the host with no heading names the document the reader is
        // already in, so there is nowhere to send them.
        None => target.to_string(),
    }
}

/// The fragment's text as it goes into the host: its own markers gone, and no
/// blank line at either end.
///
/// The markers go because the host's own region already says where the text
/// came from, and a nested pair left in place would be read as a region of the
/// host next time round. The blank lines go because the host puts the text
/// directly between its markers, so the file's own spacing would show up as a
/// gap that grows or shrinks with the fragment.
fn embedded_lines(text: &str) -> Vec<String> {
    let markers: HashSet<usize> = marker::scan(text)
        .fragments
        .iter()
        .flat_map(|region| [region.open, region.close])
        .collect();

    let lines: Vec<String> = text
        .split('\n')
        .enumerate()
        .filter(|(index, _)| !markers.contains(index))
        .map(|(_, line)| line.to_string())
        .collect();

    let first = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    let last = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(first, |index| index + 1);
    lines[first..last].to_vec()
}

/// Fill in one file in the tree, writing it back where its regions moved.
///
/// `None` where the file carries no region. `Some(false)` where it does and the
/// text already there is what the fragments say today.
pub fn fill_file(root: &Path, path: &str) -> Result<Option<bool>, String> {
    let full = root.join(path);
    let text = std::fs::read_to_string(&full)
        .map_err(|e| format!("could not read {}: {e}", full.display()))?;

    let Some(filled) = fill(path, &text, &Tree::new(root))? else {
        return Ok(None);
    };
    if filled == text {
        return Ok(Some(false));
    }

    std::fs::write(&full, &filled)
        .map_err(|e| format!("could not write {}: {e}", full.display()))?;
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handful of documents, by repository-relative path.
    struct Map(Vec<(&'static str, &'static str)>);

    impl Files for Map {
        fn read(&self, path: &str) -> Result<String, String> {
            self.0
                .iter()
                .find(|(name, _)| *name == path)
                .map(|(_, text)| (*text).to_string())
                .ok_or_else(|| format!("no such file '{path}'"))
        }
    }

    const STEPS: &str = "# Putting the device in the bootloader\n\n\
                         Hold BOOTSEL while applying power.\n\n\
                         ## After that\n\n\
                         The device enumerates.\n";

    fn files() -> Map {
        Map(vec![("docs/fragments/steps.md", STEPS)])
    }

    fn host(body: &str) -> String {
        format!("{body}<!--[fragment:docs/fragments/steps.md]-->\nstale\n<!--[/]-->\n")
    }

    #[test]
    fn a_fragment_sits_one_level_below_the_heading_above_the_marker() {
        let out = fill(
            "docs/HOST.md",
            &host("# Manual\n\n## Recovery\n\n"),
            &files(),
        )
        .unwrap()
        .unwrap();
        // The fragment's own H1 title lands at H3, and its H2 keeps its spacing
        // one below that.
        assert!(
            out.contains("### Putting the device in the bootloader"),
            "{out}"
        );
        assert!(out.contains("#### After that"), "{out}");
        assert!(!out.contains("stale"), "{out}");
        // The markers stay, so the region can be filled in again.
        assert!(
            out.contains("<!--[fragment:docs/fragments/steps.md]-->\n### Putting"),
            "{out}"
        );
        assert!(out.contains("The device enumerates.\n<!--[/]-->"), "{out}");
    }

    #[test]
    fn the_peer_token_puts_the_fragment_alongside_the_heading_above_it() {
        // Discriminates against the default: the same host and the same
        // fragment, differing only in the token, land a level apart.
        let peered = "# Manual\n\n## Recovery\n\n<!--[fragment:docs/fragments/steps.md:peer]-->\nstale\n<!--[/]-->\n";
        let out = fill("docs/HOST.md", peered, &files()).unwrap().unwrap();
        assert!(
            out.contains("## Putting the device in the bootloader"),
            "{out}"
        );
        assert!(!out.contains("### Putting"), "{out}");
    }

    #[test]
    fn a_link_naming_the_host_becomes_an_anchor_and_others_are_left_alone() {
        // Discriminates: the two links differ only in which file they name.
        assert_eq!(
            anchor_self_links(
                "see [a](/docs/HOST.md#board-list) and [b](/docs/OTHER.md#x)",
                "docs/HOST.md",
                "docs/fragments/steps.md",
            ),
            "see [a](#board-list) and [b](/docs/OTHER.md#x)"
        );
        // Nowhere to send a reader already in the document, so it stands.
        assert_eq!(
            anchor_self_links(
                "[a](/docs/HOST.md)",
                "docs/HOST.md",
                "docs/fragments/steps.md"
            ),
            "[a](/docs/HOST.md)"
        );
        // A URL and an anchor are already right.
        assert_eq!(
            anchor_self_links(
                "[a](https://onerom.org) [b](#already)",
                "docs/HOST.md",
                "docs/fragments/steps.md",
            ),
            "[a](https://onerom.org) [b](#already)"
        );
    }

    #[test]
    fn peer_with_nothing_above_the_marker_stops_the_build() {
        let orphan = "<!--[fragment:docs/fragments/steps.md:peer]-->\nstale\n<!--[/]-->\n";
        let err = fill("docs/HOST.md", orphan, &files()).unwrap_err();
        assert!(err.contains("no heading sits above it"), "{err}");
    }

    #[test]
    fn the_same_fragment_under_a_shallower_heading_lands_a_level_higher() {
        // Discriminates: only the host's preceding heading differs, and the
        // fragment moves with it.
        let out = fill("docs/HOST.md", &host("# Manual\n\n"), &files())
            .unwrap()
            .unwrap();
        assert!(
            out.contains("## Putting the device in the bootloader"),
            "{out}"
        );
        assert!(out.contains("### After that"), "{out}");
    }

    #[test]
    fn with_no_heading_above_the_marker_the_fragment_opens_at_the_hosts_own_level() {
        // The host's own sections are H2, so the fragment's title is an H2 too
        // rather than being pushed under a heading that is not above it.
        let out = fill(
            "docs/HOST.md",
            &format!(
                "{}\n## Later Section\n\ntext\n",
                host("Some opening prose.\n\n")
            ),
            &files(),
        )
        .unwrap()
        .unwrap();
        assert!(
            out.contains("\n## Putting the device in the bootloader"),
            "{out}"
        );
        assert!(out.contains("\n### After that"), "{out}");
    }

    #[test]
    fn with_no_heading_anywhere_in_the_host_the_fragment_opens_at_h1() {
        let out = fill("docs/HOST.md", &host("Just prose.\n\n"), &files())
            .unwrap()
            .unwrap();
        assert!(
            out.contains("\n# Putting the device in the bootloader"),
            "{out}"
        );
        assert!(out.contains("\n## After that"), "{out}");
    }

    #[test]
    fn a_shift_past_h6_stops_the_build_naming_the_file_and_the_line() {
        // The host's H6 would put the fragment's title at H7, and its own H2 at
        // H8.  The first one that cannot fit is named.
        let err = fill(
            "docs/HOST.md",
            &host("# Manual\n\n###### Very Deep\n\n"),
            &files(),
        )
        .unwrap_err();
        assert!(err.contains("docs/fragments/steps.md:1"), "{err}");
        assert!(
            err.contains("Putting the device in the bootloader"),
            "{err}"
        );
        assert!(err.contains("H7"), "{err}");
        assert!(err.contains("Very Deep"), "{err}");
    }

    #[test]
    fn a_region_is_rewritten_when_the_fragment_changes() {
        let host = host("# Manual\n\n## Recovery\n\n");
        let first = fill("docs/HOST.md", &host, &files()).unwrap().unwrap();
        assert!(
            first.contains("Hold BOOTSEL while applying power."),
            "{first}"
        );

        let moved = Map(vec![(
            "docs/fragments/steps.md",
            "# Putting the device in the bootloader\n\nShort the BOOT pads.\n",
        )]);
        let second = fill("docs/HOST.md", &first, &moved).unwrap().unwrap();
        assert!(second.contains("Short the BOOT pads."), "{second}");
        assert!(!second.contains("Hold BOOTSEL"), "{second}");
        assert!(!second.contains("After that"), "{second}");
    }

    #[test]
    fn filling_a_current_region_leaves_it_exactly_as_it_was() {
        let host = host("# Manual\n\n## Recovery\n\n");
        let once = fill("docs/HOST.md", &host, &files()).unwrap().unwrap();
        let twice = fill("docs/HOST.md", &once, &files()).unwrap().unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn a_value_marker_in_a_fragment_is_carried_into_the_host_as_written() {
        // The checker resolves it in the host, where the reader sees it, so it
        // has to arrive untouched.
        let map = Map(vec![(
            "docs/fragments/steps.md",
            "# Limits\n\nThe hold is <!--[const:X:seconds]-->60 seconds<!--[/]-->.\n",
        )]);
        let out = fill("docs/HOST.md", &host("# Manual\n\n## Recovery\n\n"), &map)
            .unwrap()
            .unwrap();
        assert!(
            out.contains("The hold is <!--[const:X:seconds]-->60 seconds<!--[/]-->."),
            "{out}"
        );
        // And the host still reads as one region, not two.
        let scan = marker::scan(&out);
        assert_eq!(scan.fragments.len(), 1, "{:?}", scan.fragments);
        assert_eq!(scan.spans.len(), 1, "{:?}", scan.spans);
    }

    #[test]
    fn two_regions_in_one_host_are_each_placed_under_their_own_heading() {
        let text = "# Manual\n\n## Recovery\n\n\
             <!--[fragment:docs/fragments/steps.md]-->\nstale\n<!--[/]-->\n\n\
             # Appendix\n\n\
             <!--[fragment:docs/fragments/steps.md]-->\nstale\n<!--[/]-->\n";
        let out = fill("docs/HOST.md", text, &files()).unwrap().unwrap();
        // Under the H2 the title lands at H3, and under the H1 at H2.
        assert!(
            out.contains("\n### Putting the device in the bootloader"),
            "{out}"
        );
        assert!(
            out.contains("\n## Putting the device in the bootloader"),
            "{out}"
        );
        assert!(!out.contains("stale"), "{out}");
    }

    #[test]
    fn a_published_document_named_as_a_fragment_brings_its_own_regions_resolved() {
        // docs/OVERVIEW.md is a document in its own right and may itself be
        // assembled.  What lands in the host is its resolved text, without its
        // markers, so the host holds one region rather than a nested pair.
        let map = Map(vec![
            (
                "docs/OVERVIEW.md",
                "# One ROM Overview\n\n## Hardware\n\n\
                 <!--[fragment:docs/fragments/steps.md]-->\nstale\n<!--[/]-->\n",
            ),
            ("docs/fragments/steps.md", STEPS),
        ]);
        let text = "# Manual\n\n<!--[fragment:docs/OVERVIEW.md]-->\nold\n<!--[/]-->\n";
        let out = fill("docs/CLI-MANUAL.md", text, &map).unwrap().unwrap();

        assert!(out.contains("## One ROM Overview"), "{out}");
        assert!(out.contains("### Hardware"), "{out}");
        // Under an H3 inside the overview, so H4 in the manual.
        assert!(
            out.contains("#### Putting the device in the bootloader"),
            "{out}"
        );
        assert!(!out.contains("stale"), "{out}");
        assert_eq!(marker::scan(&out).fragments.len(), 1);
    }

    #[test]
    fn a_fragment_that_reaches_itself_is_named_rather_than_recursed_on() {
        let map = Map(vec![
            (
                "docs/A.md",
                "# A\n\n<!--[fragment:docs/B.md]-->\n<!--[/]-->\n",
            ),
            (
                "docs/B.md",
                "# B\n\n<!--[fragment:docs/A.md]-->\n<!--[/]-->\n",
            ),
        ]);
        let err = fill("docs/A.md", &map.read("docs/A.md").unwrap(), &map).unwrap_err();
        assert!(err.contains("assembled from itself"), "{err}");
        assert!(err.contains("docs/A.md -> docs/B.md -> docs/A.md"), "{err}");
    }

    #[test]
    fn a_missing_fragment_names_the_marker_that_asked_for_it() {
        let err = fill("docs/HOST.md", &host("# Manual\n\n"), &Map(vec![])).unwrap_err();
        assert!(err.contains("docs/HOST.md:3"), "{err}");
        assert!(err.contains("docs/fragments/steps.md"), "{err}");
    }

    #[test]
    fn a_file_with_no_region_is_left_alone() {
        assert!(
            fill("docs/HOST.md", "# Manual\n\ntext\n", &files())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn an_unclosed_region_stops_the_build_before_anything_is_written() {
        let err = fill(
            "docs/HOST.md",
            "# Manual\n\n<!--[fragment:docs/fragments/steps.md]-->\ntext\n",
            &files(),
        )
        .unwrap_err();
        assert!(err.contains("docs/HOST.md:3"), "{err}");
        assert!(err.contains("never closed"), "{err}");
    }

    #[test]
    fn a_fragment_with_no_heading_at_all_is_copied_as_it_stands() {
        let map = Map(vec![(
            "docs/fragments/steps.md",
            "\n\nJust a paragraph, no heading.\n\n",
        )]);
        let out = fill("docs/HOST.md", &host("# Manual\n\n## Recovery\n\n"), &map)
            .unwrap()
            .unwrap();
        assert!(
            out.contains(
                "<!--[fragment:docs/fragments/steps.md]-->\n\
                 Just a paragraph, no heading.\n\
                 <!--[/]-->"
            ),
            "{out}"
        );
    }
}
