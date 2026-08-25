// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Joining whole markdown documents into one, with the levels and the links
//! corrected.
//!
//! Nothing here reads a file or writes one. It takes the members' text and
//! returns the assembled text, which is what makes each rule below testable on
//! its own.
//!
//! [`headings`], [`shallowest`] and [`shift_headings`] are the part
//! [`crate::fragment`] shares: one document's text landing inside another moves
//! by the same rule whether it is a member of a PDF or a fragment region in a
//! `docs/` file.

use std::collections::HashMap;

/// The deepest heading markdown has.
pub const MAX_HEADING_LEVEL: usize = 6;

/// Every member's title is a top-level heading, so the shallowest heading
/// beneath it belongs one level down.
const LEVEL_BELOW_TITLE: usize = 2;

/// The branch of the repository a link out of the set points at.
const REPO_BRANCH: &str = "main";

/// One member of a document, as read from the tree.
pub struct Member {
    /// Repository-relative, as `docs.toml` states it. Names the member in an
    /// error, and is what a link between members is resolved against.
    pub path: String,
    pub text: String,
}

/// One heading, by line index into the lines it was found in.
pub struct Heading {
    /// 0-based line the heading is on.
    pub index: usize,

    /// How many hashes it carries, so 1 for an H1.
    pub level: usize,

    /// The heading's text, without the hashes.
    pub text: String,
}

/// A member prepared for assembly: its title, and its lines with the hand
/// written contents already gone.
struct Prepared {
    path: String,
    lines: Vec<String>,
    /// Index of the leading H1, whose text titles the member.
    title_index: usize,
    headings: Vec<Heading>,
    /// Per line, true inside a fenced code block, where the text is quoted
    /// rather than written.
    literal: Vec<bool>,
}

/// Assemble `members`, in the order given, into one markdown document.
///
/// Returns the assembled text, or the first thing wrong with the set.
pub fn assemble(members: &[Member], repo_url: &str) -> Result<String, String> {
    let prepared: Vec<Prepared> = members
        .iter()
        .map(|member| prepare(&member.path, &member.text))
        .collect::<Result<_, _>>()?;

    check_duplicate_headings(&prepared)?;

    // Where a link between members lands: the repository-relative path of each
    // member, against the anchor its title will carry.
    let anchors: HashMap<&str, String> = prepared
        .iter()
        .map(|member| {
            (
                member.path.as_str(),
                slug(&member.headings[member.title_index_in_headings()].text),
            )
        })
        .collect();

    let mut out: Vec<String> = Vec::new();
    for (position, member) in prepared.iter().enumerate() {
        if position > 0 {
            // A horizontal rule is a page break in docs.css, so each member
            // opens a page.  The blank lines keep it a rule: a `---` directly
            // under a paragraph is a setext heading instead.
            out.push(String::new());
            out.push("---".to_string());
            out.push(String::new());
        }
        out.extend(member.emit(repo_url, &anchors)?);
    }

    let mut text = out.join("\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

impl Prepared {
    /// The position of the leading H1 in `headings`.
    fn title_index_in_headings(&self) -> usize {
        self.headings
            .iter()
            .position(|heading| heading.index == self.title_index)
            .expect("the title is one of the headings")
    }

    /// Every heading but the title, which stays where it is.
    fn movable(&self) -> Vec<&Heading> {
        let title = self.title_index_in_headings();
        self.headings
            .iter()
            .enumerate()
            .filter(|(position, _)| *position != title)
            .map(|(_, heading)| heading)
            .collect()
    }

    /// The member's lines, shifted and with its links rewritten.
    ///
    /// The shallowest heading left in the member sits one below the title, and
    /// the rest keep their spacing.  Computing it means a member needs no rule
    /// in config and no edit of its own: `docs/OVERVIEW.md`, whose sections are
    /// already H2, does not move, and `docs/CLI-MANUAL.md`, whose sections are
    /// H1, drops one level.
    fn emit(&self, repo_url: &str, anchors: &HashMap<&str, String>) -> Result<Vec<String>, String> {
        let movable = self.movable();
        let shift = shift_to(&movable, LEVEL_BELOW_TITLE);

        let mut lines = self.lines.clone();
        shift_headings(
            &self.path,
            &mut lines,
            &movable,
            shift,
            &format!(
                "Its member's headings shift by {shift} so the shallowest of \
                 them sits below the member's title."
            ),
        )?;

        // A link inside a fenced block is quoted text - an example command line
        // or a sample of markdown - and printing something other than what was
        // quoted would make the example wrong.
        let dir = parent_dir(&self.path);
        Ok(lines
            .iter()
            .zip(&self.literal)
            .map(|(line, literal)| {
                if *literal {
                    line.clone()
                } else {
                    rewrite_links(line, &dir, anchors, repo_url)
                }
            })
            .collect())
    }
}

/// The shallowest of `headings`, or `None` where there are none.
pub fn shallowest(headings: &[&Heading]) -> Option<usize> {
    headings.iter().map(|heading| heading.level).min()
}

/// How far `headings` move for their shallowest to land at `target`.
///
/// Headings with nothing to move by nothing stay put, which is what a text
/// carrying no heading at all wants.
pub fn shift_to(headings: &[&Heading], target: usize) -> isize {
    match shallowest(headings) {
        Some(level) => target as isize - level as isize,
        None => 0,
    }
}

/// Rewrite `lines` so every heading in `headings` moves by `shift` levels.
///
/// `path` names the file and `why` says what set the shift, because a heading
/// that would leave markdown's six levels has to stop the build somewhere its
/// author can act on - and the shift was worked out from the text, so the line
/// alone does not say why it moved.
pub fn shift_headings(
    path: &str,
    lines: &mut [String],
    headings: &[&Heading],
    shift: isize,
    why: &str,
) -> Result<(), String> {
    for heading in headings {
        let level = heading.level as isize + shift;
        if level > MAX_HEADING_LEVEL as isize || level < 1 {
            return Err(format!(
                "{path}:{}: '{}' is H{} and would become H{}, which markdown \
                 has no room for.\n  {why}",
                heading.index + 1,
                heading.text,
                heading.level,
                level,
            ));
        }
        let hashes = "#".repeat(level as usize);
        lines[heading.index] = format!("{hashes} {}", heading.text);
    }
    Ok(())
}

/// Read one member: drop its hand-written contents, find its title, and index
/// its headings.
fn prepare(path: &str, text: &str) -> Result<Prepared, String> {
    let mut lines = drop_hand_written_toc(text.split('\n').map(str::to_string).collect());
    // The blank lines a member ends with are its own file's, and between two
    // members they would space the page break unevenly.  The assembly puts
    // exactly one blank line either side of the rule.
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    let (headings, literal) = headings(&lines);

    let title_index = headings
        .iter()
        .find(|heading| heading.level == 1)
        .map(|heading| heading.index)
        .ok_or_else(|| format!("{path}: no leading H1, so the member has no title to sit under"))?;

    Ok(Prepared {
        path: path.to_string(),
        lines,
        title_index,
        headings,
        literal,
    })
}

/// Two members whose headings collide cannot both be linked to, and the
/// contents lists the same words twice with no way to tell them apart.
fn check_duplicate_headings(members: &[Prepared]) -> Result<(), String> {
    // Only across members.  A document repeating a heading within itself is its
    // own business, and reads that way on GitHub too.
    let mut seen: HashMap<String, (&str, usize)> = HashMap::new();
    for member in members {
        let mut mine: HashMap<String, usize> = HashMap::new();
        for heading in &member.headings {
            let key = slug(&heading.text);
            if let Some((other, line)) = seen.get(&key)
                && *other != member.path
            {
                return Err(format!(
                    "'{}' is a heading in both {other}:{line} and {}:{}.\n  \
                     A link to one of them would land on the other, so rename \
                     one.",
                    heading.text,
                    member.path,
                    heading.index + 1,
                ));
            }
            mine.entry(key).or_insert(heading.index + 1);
        }
        for (key, line) in mine {
            seen.insert(key, (&member.path, line));
        }
    }
    Ok(())
}

/// Remove a hand-written contents section, up to the next heading.
///
/// `docs/pdf/render.py` does the same to a single-source document, because the
/// generated contents supersedes a list written for GitHub.  A member is
/// prepared before its headings move, so the level written in the file is the
/// level matched here - the depth allowed is the full six all the same, since
/// nothing stops a document opening its contents at H4.
fn drop_hand_written_toc(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut skipping = false;
    for line in lines {
        if is_hand_written_toc(&line) {
            skipping = true;
            continue;
        }
        if skipping {
            if line.starts_with('#') {
                skipping = false;
            } else {
                continue;
            }
        }
        out.push(line);
    }
    out
}

/// True for a heading that opens a hand-written contents list.
fn is_hand_written_toc(line: &str) -> bool {
    let Some((hashes, rest)) = split_heading(line) else {
        return false;
    };
    if hashes > MAX_HEADING_LEVEL {
        return false;
    }
    let rest = rest.trim().to_ascii_lowercase();
    rest == "contents" || rest == "table of contents"
}

/// Walk a markdown text once, for its headings and for which of its lines are
/// quoted rather than written.
///
/// Skips the lines that only look like headings.
pub fn headings(lines: &[String]) -> (Vec<Heading>, Vec<bool>) {
    let mut found = Vec::new();
    let mut literal = vec![false; lines.len()];
    let mut fence: Option<String> = None;
    let mut in_comment = false;

    for (index, line) in lines.iter().enumerate() {
        // A fenced block holds shell comments and C preprocessor lines, which
        // open with a hash and are not headings.
        if let Some(marker) = fence.clone() {
            literal[index] = true;
            if line.trim_start().starts_with(&marker) {
                fence = None;
            }
            continue;
        }
        if let Some(marker) = opening_fence(line) {
            literal[index] = true;
            fence = Some(marker);
            continue;
        }

        // An HTML comment is invisible to a reader, so a hash inside one is
        // prose about a heading rather than a heading.  CLI-MANUAL.md opens
        // with thirty-six lines of them.
        if in_comment {
            in_comment = !line.contains("-->");
            continue;
        }
        if let Some(rest) = line.trim_start().strip_prefix("<!--")
            && !rest.contains("-->")
        {
            in_comment = true;
            continue;
        }

        if let Some((level, text)) = split_heading(line)
            && level <= MAX_HEADING_LEVEL
        {
            found.push(Heading {
                index,
                level,
                text: text.trim().to_string(),
            });
        }
    }
    (found, literal)
}

/// `("### ", "Title")` split into its level and its text, for a real heading.
fn split_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if hashes == 0 {
        return None;
    }
    let rest = &line[hashes..];
    // ATX needs a space after the hashes, so `#include` is not a heading.
    if !rest.starts_with(' ') {
        return None;
    }
    Some((hashes, rest))
}

/// The fence marker a line opens, if it opens one.
fn opening_fence(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    for marker in ["```", "~~~"] {
        if trimmed.starts_with(marker) {
            return Some(marker.to_string());
        }
    }
    None
}

/// The anchor pandoc gives a heading: lower case, punctuation dropped, spaces
/// hyphenated.
///
/// Used for the link to a member's own title and for comparing two members'
/// headings, so it has to agree with pandoc's `gfm` identifiers.
fn slug(text: &str) -> String {
    let plain = strip_inline_markup(text);
    let mut out = String::new();
    for c in plain.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        } else if c == ' ' {
            out.push('-');
        }
    }
    out
}

/// Reduce inline markdown to the plain text behind it.
fn strip_inline_markup(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // A link keeps its text and loses its target.
            '[' => {
                for inner in chars.by_ref() {
                    if inner == ']' {
                        break;
                    }
                    out.push(inner);
                }
                if chars.peek() == Some(&'(') {
                    for inner in chars.by_ref() {
                        if inner == ')' {
                            break;
                        }
                    }
                }
            }
            '`' | '*' | '_' => {}
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// The directory a member's links are relative to.
fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(at) => path[..at].to_string(),
        None => String::new(),
    }
}

/// Resolve a link target against the member holding it, to a repository
/// relative path.
pub fn resolve(target: &str, dir: &str) -> String {
    let joined = if let Some(rooted) = target.strip_prefix('/') {
        rooted.to_string()
    } else if dir.is_empty() {
        target.to_string()
    } else {
        format!("{dir}/{target}")
    };

    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

/// Rewrite every link in a line that points outside this PDF or at another
/// member of it.
///
/// A relative path means nothing to a reader holding a PDF, so it becomes
/// either an anchor within the document or a URL at the repository.
fn rewrite_links(line: &str, dir: &str, anchors: &HashMap<&str, String>, repo_url: &str) -> String {
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
        out.push_str(&format!(
            "]({})",
            rewrite_target(target, dir, anchors, repo_url)
        ));
        rest = &inside[close + 1..];
    }

    out.push_str(rest);
    out
}

/// One link target, rewritten.
fn rewrite_target(
    target: &str,
    dir: &str,
    anchors: &HashMap<&str, String>,
    repo_url: &str,
) -> String {
    // An absolute URL, a mail address and an anchor within the document are
    // already right.
    if target.starts_with('#') || target.contains("://") || target.starts_with("mailto:") {
        return target.to_string();
    }

    let (path, fragment) = match target.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (target, None),
    };
    if path.is_empty() {
        return target.to_string();
    }

    let resolved = resolve(path, dir);
    match anchors.get(resolved.as_str()) {
        // Another member of this document: the link stays inside the PDF.  A
        // target naming a heading keeps it, since a heading's anchor does not
        // move when its level does.
        Some(anchor) => match fragment {
            Some(fragment) => format!("#{fragment}"),
            None => format!("#{anchor}"),
        },
        // Outside the set, so the reader is sent to the repository.
        None => {
            let url = format!(
                "{}/blob/{REPO_BRANCH}/{resolved}",
                repo_url.trim_end_matches('/')
            );
            match fragment {
                Some(fragment) => format!("{url}#{fragment}"),
                None => url,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(path: &str, text: &str) -> Member {
        Member {
            path: path.to_string(),
            text: text.to_string(),
        }
    }

    const REPO: &str = "https://github.com/piersfinlayson/one-rom";

    #[test]
    fn a_member_whose_sections_are_h2_does_not_move() {
        let text = "# Overview\n\n## Background\n\n### Detail\n\ntext\n";
        let out = assemble(&[member("docs/OVERVIEW.md", text)], REPO).unwrap();
        assert!(out.contains("# Overview\n"), "{out}");
        assert!(out.contains("## Background"), "{out}");
        assert!(out.contains("### Detail"), "{out}");
    }

    #[test]
    fn a_member_whose_sections_are_h1_drops_one_level() {
        let text = "# Manual\n\n# Introduction\n\n## About\n\n### Deeper\n";
        let out = assemble(&[member("docs/CLI-MANUAL.md", text)], REPO).unwrap();
        assert!(out.contains("# Manual\n"), "{out}");
        assert!(out.contains("## Introduction"), "{out}");
        assert!(out.contains("### About"), "{out}");
        assert!(out.contains("#### Deeper"), "{out}");
    }

    #[test]
    fn a_member_starting_deep_is_raised_so_its_shallowest_sits_below_the_title() {
        let text = "# Note\n\n### Only Section\n\n#### Under It\n";
        let out = assemble(&[member("docs/NOTE.md", text)], REPO).unwrap();
        assert!(out.contains("## Only Section"), "{out}");
        assert!(out.contains("### Under It"), "{out}");
    }

    #[test]
    fn a_heading_that_would_pass_h6_stops_the_build_naming_it() {
        // The shallowest is H2, so everything shifts nowhere - except that this
        // member already runs to H6, and H6 plus nothing is fine.  Open at H1
        // instead and the H6 has to become H7.
        let text = "# Deep\n\n# Section\n\n###### Bottom\n";
        let err = assemble(&[member("docs/DEEP.md", text)], REPO).unwrap_err();
        assert!(err.contains("docs/DEEP.md:5"), "{err}");
        assert!(err.contains("Bottom"), "{err}");
        assert!(err.contains("H7"), "{err}");
    }

    #[test]
    fn two_members_sharing_a_heading_stop_the_build_naming_both() {
        let first = member("docs/ONE.md", "# One\n\n## Shared Name\n");
        let second = member("docs/TWO.md", "# Two\n\n## Shared Name\n");
        let err = assemble(&[first, second], REPO).unwrap_err();
        assert!(err.contains("docs/ONE.md:3"), "{err}");
        assert!(err.contains("docs/TWO.md:3"), "{err}");
        assert!(err.contains("Shared Name"), "{err}");
    }

    #[test]
    fn a_member_repeating_a_heading_within_itself_is_left_alone() {
        let text = "# One\n\n## Options\n\n### Thing\n\n## Options\n";
        assert!(assemble(&[member("docs/ONE.md", text)], REPO).is_ok());
    }

    #[test]
    fn a_link_to_another_member_becomes_an_anchor() {
        let first = member("docs/OVERVIEW.md", "# One ROM Overview\n\ntext\n");
        let second = member(
            "docs/CLI-MANUAL.md",
            "# Manual\n\nSee [the overview](OVERVIEW.md) and [a part](OVERVIEW.md#hardware).\n",
        );
        let out = assemble(&[first, second], REPO).unwrap();
        assert!(out.contains("[the overview](#one-rom-overview)"), "{out}");
        assert!(out.contains("[a part](#hardware)"), "{out}");
    }

    #[test]
    fn a_link_outside_the_set_becomes_a_url_at_the_repository() {
        let text = "# Manual\n\nSee [compat](COMPATIBILITY.md) and \
                    [logging](/docs/LOGGING.md#over-usb) and \
                    [a board](/hardware/pcb/README.md).\n";
        let out = assemble(&[member("docs/CLI-MANUAL.md", text)], REPO).unwrap();
        assert!(
            out.contains("[compat](https://github.com/piersfinlayson/one-rom/blob/main/docs/COMPATIBILITY.md)"),
            "{out}"
        );
        assert!(
            out.contains(
                "[logging](https://github.com/piersfinlayson/one-rom/blob/main/docs/LOGGING.md#over-usb)"
            ),
            "{out}"
        );
        assert!(
            out.contains(
                "[a board](https://github.com/piersfinlayson/one-rom/blob/main/hardware/pcb/README.md)"
            ),
            "{out}"
        );
    }

    #[test]
    fn an_anchor_and_an_absolute_url_are_left_alone() {
        let text = "# Manual\n\n[here](#device-states) and [site](https://onerom.org/web).\n";
        let out = assemble(&[member("docs/CLI-MANUAL.md", text)], REPO).unwrap();
        assert!(out.contains("[here](#device-states)"), "{out}");
        assert!(out.contains("[site](https://onerom.org/web)"), "{out}");
    }

    #[test]
    fn a_hash_inside_a_code_block_or_a_comment_is_not_a_heading() {
        let text = "# Manual\n\n<!--\n# Not A Heading\n-->\n\n```sh\n# also not\n```\n\n## Real\n";
        let out = assemble(&[member("docs/CLI-MANUAL.md", text)], REPO).unwrap();
        // The shallowest real heading is the H2, so nothing moves.  A comment
        // or fence counted as a heading would have made it H1 and shifted the
        // document.
        assert!(out.contains("\n# Not A Heading\n"), "{out}");
        assert!(out.contains("\n# also not\n"), "{out}");
        assert!(out.contains("\n## Real"), "{out}");
    }

    #[test]
    fn a_hand_written_contents_is_dropped_up_to_the_next_heading() {
        let text = "# One\n\n## Contents\n\n- [A](#a)\n- [B](#b)\n\n## A\n\ntext\n";
        let out = assemble(&[member("docs/ONE.md", text)], REPO).unwrap();
        assert!(!out.contains("Contents"), "{out}");
        assert!(!out.contains("- [A](#a)"), "{out}");
        assert!(out.contains("## A"), "{out}");
    }

    #[test]
    fn a_member_with_no_title_is_refused() {
        let err = assemble(&[member("docs/ONE.md", "## Only\n")], REPO).unwrap_err();
        assert!(err.contains("docs/ONE.md"), "{err}");
        assert!(err.contains("H1"), "{err}");
    }

    #[test]
    fn members_are_separated_by_a_page_break() {
        let first = member("docs/ONE.md", "# One\n\ntext\n");
        let second = member("docs/TWO.md", "# Two\n\ntext\n");
        let out = assemble(&[first, second], REPO).unwrap();
        assert!(out.contains("text\n\n---\n\n# Two"), "{out}");
        assert!(!out.starts_with("---"), "{out}");
    }

    #[test]
    fn a_slug_matches_the_anchor_pandoc_gives_a_heading() {
        assert_eq!(slug("One ROM Overview"), "one-rom-overview");
        assert_eq!(slug("Part 1 — Guide"), "part-1--guide");
        assert_eq!(slug("`--slot` specifications"), "--slot-specifications");
    }

    #[test]
    fn a_target_is_resolved_against_the_member_holding_it() {
        assert_eq!(resolve("COMPATIBILITY.md", "docs"), "docs/COMPATIBILITY.md");
        assert_eq!(resolve("/docs/LOGGING.md", "docs"), "docs/LOGGING.md");
        assert_eq!(resolve("../README.md", "docs"), "README.md");
    }
}
