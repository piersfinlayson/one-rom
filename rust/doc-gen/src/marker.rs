// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Finding the marked spans in a markdown document.
//!
//! A marked span is a value the document states and something else owns:
//!
//! ```text
//! The device's own limit is <!--[const:GPIO_MAX_HOLD_MS:seconds]-->60 seconds<!--[/]-->.
//! ```
//!
//! The markers are HTML comments, so they are invisible to a reader of the
//! rendered markdown and of the PDF, and visible to whoever edits the source -
//! where they say that the number is owned elsewhere and a hand edit will be
//! caught.
//!
//! This module knows nothing about where a value comes from. It finds spans,
//! and it rejects the ways a span can be written that would make the check a
//! lie.

/// Opens a marked span. What follows, up to [`SPEC_END`], is the span's spec.
const OPEN: &str = "<!--[";

/// Ends the spec of an opening marker.
const SPEC_END: &str = "]-->";

/// Closes a marked span.
const CLOSE: &str = "<!--[/]-->";

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

/// A document that cannot be checked as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// 1-based line the problem is on.
    pub line: usize,

    /// What is wrong, as a sentence.
    pub detail: String,
}

/// Everything a document says, and everything wrong with how it says it.
#[derive(Debug, Default)]
pub struct Scan {
    pub spans: Vec<Span>,
    pub problems: Vec<Problem>,
}

impl Scan {
    /// Whether the document carries any marker at all, well-formed or not.
    ///
    /// A document with none is not checked and is not a failure: marking a
    /// document up is opt-in, one value at a time.
    pub fn is_marked(&self) -> bool {
        !self.spans.is_empty() || !self.problems.is_empty()
    }
}

/// Whether a line opens or closes a fenced code block.
fn is_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

/// Find every marked span in `markdown`.
///
/// A span must open and close on one line. Markdown puts a table row, a list
/// item and a sentence on a line, which is every place a value is stated, and
/// requiring it means an unclosed marker is caught where it is written rather
/// than swallowing the paragraphs after it.
///
/// A marker inside a fenced code block is refused rather than checked. Fenced
/// blocks in this documentation hold pasted command output, and a number in a
/// transcript is a record of what a command printed - not a claim about what
/// the software does today, and not something to hold against the current
/// source of the value.
pub fn scan(markdown: &str) -> Scan {
    let mut out = Scan::default();
    let mut in_fence = false;

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
            out.problems.push(Problem {
                line: number,
                detail: "a marker inside a fenced code block: quoted output is a \
                         record of what was printed, so it must not be checked \
                         against today's value"
                    .to_string(),
            });
            continue;
        }

        scan_line(line, number, &mut out);
    }

    if in_fence {
        out.problems.push(Problem {
            line: markdown.lines().count(),
            detail: "an unclosed fenced code block".to_string(),
        });
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
            out.problems.push(Problem {
                line: number,
                detail: "a closing marker with no opening marker before it".to_string(),
            });
            return;
        }

        let Some(spec_end) = after_open.find(SPEC_END) else {
            out.problems.push(Problem {
                line: number,
                detail: format!("an opening marker with no '{SPEC_END}'"),
            });
            return;
        };
        let spec = &after_open[..spec_end];
        let after_spec = &after_open[spec_end + SPEC_END.len()..];

        let Some(close_at) = after_spec.find(CLOSE) else {
            out.problems.push(Problem {
                line: number,
                detail: format!("marker '{spec}' is not closed on its own line"),
            });
            return;
        };
        let text = &after_spec[..close_at];

        if text.contains(OPEN) {
            out.problems.push(Problem {
                line: number,
                detail: format!("marker '{spec}' contains another marker"),
            });
            return;
        }

        if spec.is_empty() {
            out.problems.push(Problem {
                line: number,
                detail: "an opening marker with no spec".to_string(),
            });
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
        out.problems.push(Problem {
            line: number,
            detail: "a closing marker with no opening marker before it".to_string(),
        });
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
