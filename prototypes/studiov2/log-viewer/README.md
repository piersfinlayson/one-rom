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

21 tests. They drive the real widget with real mouse and keyboard events against
a recording clipboard, so selection and copy are proved without touching yours.

## Measured

A million lines, 81 bytes each, on a 32GB M5 MacBook Air.

| | window over a file | whole log in the widget |
| --- | --- | --- |
| memory after use | 208 MB | 699 MB |
| slowest single update | 47 ms | 121 ms |
| jump to any line | 15-18 ms | not possible |
| search whole log | 51 ms, off-thread | not possible |

Jump, window move and live tail measure the same at 10,000 lines as at a
million. Only search and select-all grow with the log, both linearly.

## Limits found

- **Handing the widget a fresh buffer costs 4.5 ms, whatever changed.**
  `Content::with_text` builds a cosmic-text buffer that carries no size, so it
  shapes all 120 lines at a placeholder font size, and the widget's next layout
  finds the metrics wrong and shapes all 120 again. Reading the lines out of
  the file takes 10 microseconds. A jump pays this and should. A live tail must
  not: the window slides instead, pasting the arriving lines on and deleting
  the departing ones, so the lines that stay keep the layout they were shaped
  with.
- **`iced::window::frames()` costs about 40% of a core, on an idle window.**
  It asks for a callback on every display frame for as long as it is
  subscribed, so the whole window redraws at the refresh rate with nothing
  happening. It is the only way to time frames, so it runs here while a
  measurement does and is off otherwise — the **Measure** button, `--probe` and
  `--bench` turn it on. Idle then costs nothing measurable.
- **A live log costs what the lines cost, once the window slides.** Rebuilding
  it instead cost the same whatever the device said, which is what gave the
  pane away:

  | lines/s | rebuilding the window | sliding it |
  | --- | --- | --- |
  | 20 | 34.5% of a core | 2.6% |
  | 200 | 33.3% | 4.5% |
  | 5,000 | 36.5% | 42.2% |

  Frames during a live tail at 200 lines/s go from p95 26.2 ms and a 43.3 ms
  worst case to p95 11.1 ms and an 11.3 ms worst case.
- **macOS App Nap multiplies every one of these numbers by four**, about 30
  seconds after the app stops being the front window, and it does not hand them
  back. A fixed integer loop on the update thread goes from 45 ms to 185 ms, a
  window rebuild from 5 ms to 38 ms, and the read of the window out of the file
  from 3 to 12 microseconds — the process is on an efficiency core and
  everything it does is slower. The shell hosting this pane used half a core at
  200 lines/s, so four times that saturated one: the pane stopped drawing, the
  50 ms stream tick fell behind, and it stayed pinned for as long as it ran.
  The same build started with `-NSAppSleepDisabled YES` held its update thread
  at 27% and its tick rate at 20 a second through 19,000 lines. This is why a
  desktop pane needs a large multiple of headroom rather than a comfortable
  margin, and it is worth reaching for before believing any figure measured on
  a window that was not in front.
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
