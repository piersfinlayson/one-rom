# Shell prototype

Both prototype screens in one window, on Iced 0.14.

**Question.** Can a screen be written and run on its own, and then reused
inside a larger app — and what does the state two screens share cost where
they meet?

**Answer. Yes, and the sharing costs one parameter and one message.** Each
screen's `update` and `view` grew a single `Shared` borrow, not four. All the
cross-screen wiring is 205 lines in `src/main.rs`, of which 24 are message
wrapping. Both screens still run alone, and their 24 tests still pass.

The expensive part was not the shell. It was moving the log out of the screen
that owned it: `LogView` went from holding a `Store` to taking one, which is
16 signatures, 35 call sites and the tests.

## What turned out to be shared

Three things, in `studiov2-shared`:

- **The log.** Both screens write to it and both show it. The store is one
  file, and each screen keeps its own widget-side window over it — a cache,
  refreshed when the store's revision moves, never a second copy.
- **The selected device.** The builder takes its board from it, the console
  takes its banner from it. Which devices are *attached* is the shell's own
  business and is not here.
- **The built image.** The builder produces it, a programmer and an analyser
  would want it, so it cannot live in the builder. It crosses as bytes, a
  name and a line of description — not as `onerom-gen`'s `Built`, which would
  drag `onerom-gen` into the log pane's dependencies.

**The style had to move too**, and that was not on the list. One window has
one theme, so a palette owned by one screen is a palette the other cannot
have. It is in `studiov2-shared::style` and both screens draw from it.

## Run

```bash
cargo run -p studiov2-shell           # both screens, tabs at the top
cargo run -p studiov2-slot-builder    # the builder alone
cargo run -p studiov2-log-viewer      # the log and console alone
cargo test --workspace --release      # 24 tests
```

`ONEROM_SHELL_SHOT` is a path to write a PNG to and `ONEROM_SHELL_SETUP` is a
comma-separated script — `logs`, `device:ORFA-0031-A840`, `stream`.

## What is limited

- **Two screens is not many screens.** The shell's `update` grows one arm per
  screen, and nothing here says what twelve arms feel like.
- **A screen sees all of `Shared`, not the part it uses.** Nothing stops the
  builder reading the log store directly. The compiler enforces that a screen
  cannot reach *another screen*, not that it stays inside its own share.
- **Devices are made up.** `shared::device::attached` returns two invented
  ones. There is no USB here.
- The log screen carries the measurement harness it was built as — options,
  probes, benchmarks — into the shell, where it is unreachable but still
  compiled.
