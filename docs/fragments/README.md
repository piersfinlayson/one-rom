# Documentation fragments

A fragment is a partial markdown file, written only to be embedded in another
one. It is not a document and it is not part of the published set: it has no
PDF, nothing links to it, and read on its own it starts in the middle of a
subject.

A document embeds one by naming it in a fragment marker, and the assembled text
is written into the committed file between the markers:

    ## Recovery

    <!==[fragment:docs/fragments/recovery-steps.md]==>
    ### Putting the device in the bootloader

    Hold BOOTSEL while applying power...
    <!==[/]==>

(written there with `=` in place of `-`, so this note is not itself a region,
the same convention `docs/CLI-MANUAL.md` uses for a value marker.) So a reader
on GitHub sees a whole document, with nothing to chase, and the words exist
once.

## Writing one

Write it as a document in its own right, opening with a `#` title. The levels
are worked out, not declared: the fragment's shallowest heading sits one level
below the nearest heading above the marker in the host, so the example above
puts the fragment's `#` title at H3. Embedded under an H1 the same file opens at
H2 instead. Neither file says a word about the other, and a shift that would
push a heading past H6 stops the build naming the file and the line.

Two placements look the same in a file - a fragment that is a section of the
host, and one that is the body of the section above it. Both have a heading of
the same level before and after. So the marker takes an optional `:peer` token,
which puts the fragment alongside the heading above it rather than inside it:

    <!==[fragment:docs/OVERVIEW.md:peer]==>

Without it the fragment is content of the section above, which is the common
case. That token is the only thing a marker says that cannot be worked out from
the two files.

Write links as they should read in the host, not relative to this directory. A
link is copied across untouched, so it resolves from wherever it lands, and a
fragment is never read on its own.

A fragment can state a value it does not own, in a value marker, exactly as any
other document does. The marker travels into the host, where the reader sees it
and where `cargo run -p doc-gen` checks it.

## Filling the regions in

    cargo run -p doc-gen --bin doc-assemble -- --fragments docs

`ci/rust-tests.sh` runs that, then fails where a region differs from what is
committed - so a fragment edited without its hosts being filled in does not get
past the gate.

A document already in the published set can be named by a fragment marker too:
`docs/OVERVIEW.md` is a document and an opening chapter both. This directory is
for the text that is only ever the second of those.
