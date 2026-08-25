// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Finding the markers in a markdown document.
//!
//! There are two, and they share one delimiter family, so a reader of the
//! source meets one syntax and `docs/pdf/render.py` strips both with one
//! pattern.
//!
//! A marked span is a value the document states and something else owns:
//!
//! ```text
//! The device's own limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->60 seconds<!--[/]-->.
//! ```
//!
//! A fragment region is a block of text assembled from another markdown file:
//!
//! ```text
//! <!--[fragment:docs/fragments/recovery-steps.md]-->
//! ### Putting the device in the bootloader
//! <!--[/]-->
//! ```
//!
//! The markers are HTML comments, so they are invisible to a reader of the
//! rendered markdown and of the PDF, and visible to whoever edits the source -
//! where they say that the number, or the block, is owned elsewhere.
//!
//! This module knows nothing about where a value comes from or what a fragment
//! file holds. It finds the markers, and it rejects the ways they can be
//! written that would make the check a lie.

/// Opens a marked span. What follows, up to [`SPEC_END`], is the span's spec.
const OPEN: &str = "<!--[";

/// Ends the spec of an opening marker.
const SPEC_END: &str = "]-->";

/// Closes a marked span or a fragment region.
const CLOSE: &str = "<!--[/]-->";

/// The spec prefix that makes an opening marker a fragment region rather than
/// a value span. What follows it is the file's path.
const FRAGMENT: &str = "fragment:";

/// One marked span: what the document claims, and what it says owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// 1-based line the span opens on.
    pub line: usize,

    /// The text between the markers - what the document tells the reader.
    pub text: String,

    /// The spec from the opening marker, e.g. `const:GPIO_MAX_HOLD_MS:seconds`.
    pub spec: String,
}

/// One fragment region: where it sits, and the file whose text fills it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// 0-based line of the opening marker.
    pub open: usize,

    /// 0-based line of the closing marker.
    pub close: usize,

    /// The file named by the marker, relative to the repository root.
    pub path: String,

    /// True where the marker declares the fragment a peer of the heading above
    /// it rather than content of it.  Two placements look identical in a file -
    /// a section of the host, and the body of the section above - so this is
    /// the one thing a marker says that cannot be worked out.
    pub peer: bool,
}

/// Who has to act on a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum About {
    /// A value span, or a marker that is neither one thing nor the other.
    Value,

    /// A fragment region. The assembler cannot fill in a document written this
    /// way, so it stops on these and leaves the rest to the checker.
    Fragment,
}

/// A document that cannot be checked as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// 1-based line the problem is on.
    pub line: usize,

    /// What is wrong, as a sentence.
    pub detail: String,

    /// Which marker the problem is about.
    pub about: About,
}

impl Problem {
    /// Something wrong with a value span, or with a marker that is neither
    /// kind.
    fn value(line: usize, detail: impl Into<String>) -> Self {
        Problem {
            line,
            detail: detail.into(),
            about: About::Value,
        }
    }

    /// Something wrong with a fragment region.
    fn fragment(line: usize, detail: impl Into<String>) -> Self {
        Problem {
            line,
            detail: detail.into(),
            about: About::Fragment,
        }
    }
}

/// Everything a document says, and everything wrong with how it says it.
#[derive(Debug, Default)]
pub struct Scan {
    pub spans: Vec<Span>,
    pub fragments: Vec<Fragment>,
    pub problems: Vec<Problem>,
}

impl Scan {
    /// Whether the document carries any marker at all, well-formed or not.
    ///
    /// A document with none is not checked and is not a failure: marking a
    /// document up is opt-in, one value at a time.
    pub fn is_marked(&self) -> bool {
        !self.spans.is_empty() || !self.fragments.is_empty() || !self.problems.is_empty()
    }

    /// The problems the assembler has to stop on.
    pub fn fragment_problems(&self) -> impl Iterator<Item = &Problem> {
        self.problems
            .iter()
            .filter(|problem| problem.about == About::Fragment)
    }
}

/// Whether a line opens or closes a fenced code block.
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// The token that makes a fragment a peer of the heading above it.
const PEER: &str = ":peer";

/// The path a line names and its placement, where the whole line is a fragment
/// marker.
///
/// A fragment marker sits alone on its line, because the region it opens is a
/// block of lines - text either side of it would end up neither in the region
/// nor plainly outside it.
fn fragment_spec(line: &str) -> Option<(&str, bool)> {
    let spec = line
        .trim()
        .strip_prefix(OPEN)?
        .strip_suffix(SPEC_END)?
        .strip_prefix(FRAGMENT)?;
    let (path, peer) = match spec.strip_suffix(PEER) {
        Some(path) => (path, true),
        None => (spec, false),
    };
    if path.is_empty() || path.contains(SPEC_END) || path.contains(':') {
        None
    } else {
        Some((path, peer))
    }
}

/// Find every marked span and every fragment region in `markdown`.
///
/// A span must open and close on one line. Markdown puts a table row, a list
/// item and a sentence on a line, which is every place a value is stated, and
/// requiring it means an unclosed marker is caught where it is written rather
/// than swallowing the paragraphs after it.
///
/// A fragment region is the other way round: it opens and closes on lines of
/// its own and holds whole lines between them. The spans inside one are still
/// found, because a fragment states values like any other prose and the
/// assembled copy is what a reader sees.
///
/// A marker inside a fenced code block is refused rather than checked. Fenced
/// blocks in this documentation hold pasted command output, and a number in a
/// transcript is a record of what a command printed - not a claim about what
/// the software does today, and not something to hold against the current
/// source of the value.
pub fn scan(markdown: &str) -> Scan {
    let mut out = Scan::default();
    let mut in_fence = false;
    let mut open_fragment: Option<(usize, String, bool)> = None;

    for (index, line) in markdown.lines().enumerate() {
        let number = index + 1;

        if is_fence(line) {
            in_fence = !in_fence;
            continue;
        }

        if !line.contains(OPEN) {
            continue;
        }

        if in_fence {
            out.problems.push(Problem::value(
                number,
                "a marker inside a fenced code block: quoted output is a record \
                 of what was printed, so it must not be checked against today's \
                 value",
            ));
            continue;
        }

        // The closing marker alone on its line ends the region.  Inside a
        // region every other line, marked up or not, is the region's text.
        if let Some((open, path, peer)) = &open_fragment
            && line.trim() == CLOSE
        {
            out.fragments.push(Fragment {
                open: *open,
                close: index,
                path: path.clone(),
                peer: *peer,
            });
            open_fragment = None;
            continue;
        }

        if let Some((path, peer)) = fragment_spec(line) {
            if open_fragment.is_some() {
                out.problems.push(Problem::fragment(
                    number,
                    "a fragment region inside another fragment region: a region \
                     holds one file's text, and the file it names carries its \
                     own regions",
                ));
                continue;
            }
            open_fragment = Some((index, path.to_string(), peer));
            continue;
        }

        // A fragment marker sharing its line with anything else, including a
        // second one.
        if line.contains(&format!("{OPEN}{FRAGMENT}")) {
            out.problems.push(Problem::fragment(
                number,
                "a fragment marker sharing its line: it opens a block of lines, \
                 so it belongs alone on one",
            ));
            continue;
        }

        scan_line(line, number, &mut out);
    }

    if let Some((open, path, _)) = open_fragment {
        out.problems.push(Problem::fragment(
            open + 1,
            format!("fragment region '{path}' is never closed by '{CLOSE}'"),
        ));
    }

    if in_fence {
        out.problems.push(Problem::value(
            markdown.lines().count(),
            "an unclosed fenced code block",
        ));
    }

    out
}

/// Find the spans on one line, outside any code fence.
fn scan_line(line: &str, number: usize, out: &mut Scan) {
    let mut rest = line;

    while let Some(open_at) = rest.find(OPEN) {
        let after_open = &rest[open_at + OPEN.len()..];

        // A close with no open reaches here as an opening marker whose spec
        // starts with '/', which is worth its own message.
        if after_open.starts_with('/') {
            out.problems.push(Problem::value(
                number,
                "a closing marker with no opening marker before it",
            ));
            return;
        }

        let Some(spec_end) = after_open.find(SPEC_END) else {
            out.problems.push(Problem::value(
                number,
                format!("an opening marker with no '{SPEC_END}'"),
            ));
            return;
        };
        let spec = &after_open[..spec_end];
        let after_spec = &after_open[spec_end + SPEC_END.len()..];

        let Some(close_at) = after_spec.find(CLOSE) else {
            out.problems.push(Problem::value(
                number,
                format!("marker '{spec}' is not closed on its own line"),
            ));
            return;
        };
        let text = &after_spec[..close_at];

        if text.contains(OPEN) {
            out.problems.push(Problem::value(
                number,
                format!("marker '{spec}' contains another marker"),
            ));
            return;
        }

        if spec.is_empty() {
            out.problems
                .push(Problem::value(number, "an opening marker with no spec"));
            return;
        }

        out.spans.push(Span {
            line: number,
            text: text.to_string(),
            spec: spec.to_string(),
        });

        rest = &after_spec[close_at + CLOSE.len()..];
    }

    // Anything left holding a close is a close without an open.
    if rest.contains(CLOSE) {
        out.problems.push(Problem::value(
            number,
            "a closing marker with no opening marker before it",
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marked_span_yields_its_spec_and_its_text() {
        let scan = scan("limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->60 seconds<!--[/]-->.\n");
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert_eq!(
            scan.spans,
            vec![Span {
                line: 1,
                text: "60 seconds".to_string(),
                spec: "const:GPIO_MAX_HOLD_MS:seconds".to_string(),
            }]
        );
    }

    #[test]
    fn two_spans_on_one_line_are_both_found() {
        // A table row states a default and a minimum in one line.
        let scan = scan("| <!--[a:b]-->5000ms<!--[/]--> | <!--[c:d]-->1000ms<!--[/]--> |");
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        let specs: Vec<&str> = scan.spans.iter().map(|s| s.spec.as_str()).collect();
        assert_eq!(specs, ["a:b", "c:d"]);
        let texts: Vec<&str> = scan.spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["5000ms", "1000ms"]);
    }

    #[test]
    fn an_unmarked_document_is_not_marked_and_has_no_problems() {
        let scan = scan("# Title\n\nJust prose, and a number: 60000.\n");
        assert!(!scan.is_marked());
        assert!(scan.problems.is_empty());
    }

    #[test]
    fn a_marker_in_a_fenced_block_is_refused() {
        // The number in a transcript is what the command printed, not a claim.
        let doc =
            "```\n$ onerom control reset --pin x1\nheld for <!--[const:X]-->100ms<!--[/]-->\n```\n";
        let fenced = scan(doc);
        assert!(fenced.spans.is_empty());
        assert_eq!(fenced.problems.len(), 1, "{:?}", fenced.problems);
        assert!(
            fenced.problems[0].detail.contains("fenced"),
            "{:?}",
            fenced.problems
        );

        // The same line outside the fence is a span, so it is the fence doing
        // the refusing and not something about the line.
        let outside = scan("held for <!--[const:X]-->100ms<!--[/]-->\n");
        assert_eq!(outside.spans.len(), 1);
        assert!(outside.problems.is_empty());
    }

    #[test]
    fn markers_after_a_closed_fence_are_checked_again() {
        let doc = "```\ntranscript\n```\nlimit is <!--[const:X:seconds]-->60 seconds<!--[/]-->.\n";
        let scan = scan(doc);
        assert_eq!(scan.spans.len(), 1, "{:?}", scan);
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
    }

    #[test]
    fn an_unclosed_span_is_a_problem_on_its_own_line() {
        let scan = scan("first\nlimit is <!--[const:X]-->60 seconds\nmore prose\n");
        assert!(scan.spans.is_empty());
        assert_eq!(scan.problems.len(), 1);
        assert_eq!(scan.problems[0].line, 2);
        assert!(
            scan.problems[0].detail.contains("not closed"),
            "{:?}",
            scan.problems
        );
    }

    #[test]
    fn a_marker_with_no_spec_end_is_a_problem() {
        let scan = scan("limit is <!--[const:X 60 seconds\n");
        assert_eq!(scan.problems.len(), 1);
        assert!(
            scan.problems[0].detail.contains("]-->"),
            "{:?}",
            scan.problems
        );
    }

    #[test]
    fn a_close_with_no_open_is_a_problem() {
        let scan = scan("60 seconds<!--[/]-->\n");
        assert!(scan.spans.is_empty());
        assert_eq!(scan.problems.len(), 1);
        assert!(
            scan.problems[0].detail.contains("no opening marker"),
            "{:?}",
            scan.problems
        );
    }

    #[test]
    fn a_fragment_region_is_found_with_the_file_it_names() {
        let doc = "## Recovery\n\n<!--[fragment:docs/fragments/steps.md]-->\n### Bootloader\n\ntext\n<!--[/]-->\n\nafter\n";
        let scan = scan(doc);
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert_eq!(
            scan.fragments,
            vec![Fragment {
                open: 2,
                close: 6,
                path: "docs/fragments/steps.md".to_string(),
                peer: false,
            }]
        );
        // The region is not a value span, so nothing tries to resolve
        // 'fragment' as a source of values.
        assert!(scan.spans.is_empty(), "{:?}", scan.spans);
    }

    #[test]
    fn the_peer_token_is_read_off_the_marker() {
        let doc = "# Section\n\n<!--[fragment:docs/fragments/steps.md:peer]-->\n<!--[/]-->\n";
        let scan = scan(doc);
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert_eq!(scan.fragments.len(), 1);
        assert!(scan.fragments[0].peer);
        assert_eq!(scan.fragments[0].path, "docs/fragments/steps.md");
    }

    #[test]
    fn a_token_that_is_not_peer_is_not_a_fragment_marker() {
        // Discriminates against accepting anything after the path: a typo must
        // not quietly become part of the filename either.
        let doc = "# Section\n\n<!--[fragment:docs/fragments/steps.md:peeer]-->\n<!--[/]-->\n";
        let scan = scan(doc);
        assert!(scan.fragments.is_empty(), "{:?}", scan.fragments);
    }

    #[test]
    fn a_value_marker_inside_a_fragment_region_is_still_a_span() {
        let doc = "<!--[fragment:docs/fragments/steps.md]-->\n\
                   The limit is <!--[const:X:seconds]-->60 seconds<!--[/]-->.\n\
                   <!--[/]-->\n";
        let scan = scan(doc);
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        assert_eq!(scan.fragments.len(), 1, "{:?}", scan.fragments);
        assert_eq!(scan.spans.len(), 1, "{:?}", scan.spans);
        assert_eq!(scan.spans[0].spec, "const:X:seconds");
        assert_eq!(scan.spans[0].text, "60 seconds");
    }

    #[test]
    fn an_unclosed_fragment_region_is_a_problem_on_its_opening_line() {
        let doc = "text\n<!--[fragment:docs/fragments/steps.md]-->\n### Bootloader\n";
        let scan = scan(doc);
        assert!(scan.fragments.is_empty());
        assert_eq!(scan.problems.len(), 1, "{:?}", scan.problems);
        assert_eq!(scan.problems[0].line, 2);
        assert_eq!(scan.problems[0].about, About::Fragment);
        assert!(
            scan.problems[0].detail.contains("never closed"),
            "{:?}",
            scan.problems
        );
    }

    #[test]
    fn a_fragment_region_inside_another_is_refused() {
        let doc = "<!--[fragment:a.md]-->\n<!--[fragment:b.md]-->\n<!--[/]-->\n";
        let scan = scan(doc);
        let problems: Vec<&Problem> = scan.fragment_problems().collect();
        assert!(
            problems.iter().any(|p| p.detail.contains("inside another")),
            "{:?}",
            scan.problems
        );
    }

    #[test]
    fn a_fragment_marker_sharing_its_line_is_refused() {
        let scan = scan("See <!--[fragment:a.md]--> here\n<!--[/]-->\n");
        assert!(scan.fragments.is_empty());
        assert_eq!(scan.problems[0].about, About::Fragment);
        assert!(
            scan.problems[0].detail.contains("alone on one"),
            "{:?}",
            scan.problems
        );
    }

    #[test]
    fn a_fragment_marker_in_a_fenced_block_is_refused_like_any_other() {
        // docs/wip proposals quote the syntax as an example.  A quoted marker
        // is not a region, so nothing tries to read the file it names.
        let doc = "```markdown\n<!--[fragment:docs/fragments/steps.md]-->\n### Bootloader\n<!--[/]-->\n```\n";
        let scan = scan(doc);
        assert!(scan.fragments.is_empty(), "{:?}", scan.fragments);
        assert!(scan.fragment_problems().next().is_none());
    }

    #[test]
    fn an_unclosed_fence_is_a_problem() {
        let scan = scan("```\ntranscript\n");
        assert_eq!(scan.problems.len(), 1);
        assert!(
            scan.problems[0].detail.contains("fenced"),
            "{:?}",
            scan.problems
        );
    }
}
