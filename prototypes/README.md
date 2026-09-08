# Prototypes

Apps built to answer a specific question. Nothing here is expected to ship, and
nothing in the tree depends on it.

Each is excluded from the `rust/` workspace by an empty `[workspace]` table in
its own `Cargo.toml`, so a workspace-wide `cargo` command does not build it.

- [`studiov2/`](studiov2) — Tackle One ROM Studio v2 on Iced questions.
