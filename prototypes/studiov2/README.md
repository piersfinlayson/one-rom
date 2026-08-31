# Studio v2 prototypes

Iced prototypes for a One ROM Studio v2: whether it would look right, whether
it can handle device log output, and how a growing app should be laid out.
They build against this tree's crates.

- [`slot-builder/`](slot-builder) — the website's multi-slot ROM image builder,
  rebuilt in Iced. Answers whether the design survives the toolkit.
- [`log-viewer/`](log-viewer) — log and console panes. Answers whether text can
  be selected and copied, and what a long log session costs.
- [`shell/`](shell) — both of the above in one window. Answers what state two
  screens genuinely share and what it costs where they meet.
- [`shared/`](shared) — the log store, the palette and the shared state types.
  Everything more than one screen reads.

[`REPORT.md`](REPORT.md) is the investigation these came out of — why Iced, what
each prototype proved, and what is still open.
