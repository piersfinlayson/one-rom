# Log viewer prototype

Log and console panes in Iced 0.14.

**Question.** Can a device log be kept whole for a whole session, and still be
scrolled, searched, selected and copied?

**Answer. Yes, with stock Iced widgets.** The log lives in a file. The widget
holds a 120-line window over it. The widget's cost does not grow with the log,
so session length is bounded by disk, not by the toolkit. No custom text widget
is needed.

## Run

```bash
cargo run --release
```

`--help` lists the switches. `--fill 1000000 --probe` loads a million synthetic
lines and prints timings. `--console-demo` runs a scripted device session.
`--mode capped` switches to holding the whole log in the widget instead, for
comparison.

```bash
cargo test --release
```

24 tests. They drive the real widget with real mouse and keyboard events against
a recording clipboard, so selection and copy are proved without touching yours.

## Measured

A million lines, 81 bytes each, on a 32GB M5 MacBook Air.

| | window over a file | whole log in the widget |
| --- | --- | --- |
| memory after use | 208 MB | 699 MB |
| slowest single update | 47 ms | 121 ms |
| jump to any line | 15-18 ms | not possible |
| search whole log | 51 ms, off-thread | not possible |

Jump, window rebuild and live tail measure the same at 10,000 lines as at a
million. Only search and select-all grow with the log, both linearly.

## Limits found

- **A window move costs about 7 ms**, and nearly all of it is Iced rebuilding a
  text buffer it is about to rebuild again on the next draw. Reading the lines
  out of the file takes 20 microseconds. Comfortable on an M5 with no headroom.
  Appending rather than rebuilding removes most of it.
- **A drag stops at the edge of the pane.** Iced publishes no drag event once
  the pointer leaves the widget, so holding a selection past the bottom needs a
  timer to scroll it. 30 lines, here.
- **A selection cannot leave the widget, but a copy can.** Drag, double-click,
  Cmd+A and Cmd+C work across ranges far larger than the window, because what
  the widget highlights and what reaches the clipboard come from different
  places.
- **The scrollbar thumb stops being honest about size past ~30,000 lines.** Its
  position stays exact. Iced floors the thumb at 2px.
- **`text_editor` swallows the scroll wheel**, so a `scrollable` wrapped around
  it never sees one. Its own scroll action has to be forwarded by hand.
- **`iced_term` cannot serve a serial console.** It spawns a PTY and keeps its
  backend private, so a device's bytes cannot be fed to it.
- **Async tasks returned from separate updates have no ordering**, so a device
  replies out of order under several commands. One command in flight fixes it.
- **`iced::window::screenshot` does not capture editor text** — box and border
  only. Screenshots are no good as evidence for a text pane.

The console pane is a command transcript rather than a stream, so it keeps the
whole thing in the widget.
