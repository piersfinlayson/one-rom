// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! Physical jumper-header descriptor for One ROM boards.
//!
//! Describes the jumper / programming header at the top edge of a One ROM
//! board, column by column, so host tools (e.g. the web ROM Slot Builder) can
//! draw an accurate wireframe of which pads to jumper for image selection,
//! rather than assuming a single fixed layout that is wrong for older or
//! differently-shaped board revisions.
//!
//! The data is emitted as `const` board data by the build script and returned
//! by [`Board::jumper_header`](crate::hw::Board::jumper_header). It is purely a
//! description of the *physical* header; the electrical assignments (image-
//! select GPIOs, the SWD-multiplexed select pins, the X pins) continue to live
//! in the MCU pin arrays, and are cross-checked against this descriptor at
//! build time so the two cannot silently drift apart.
//!
//! # Model
//!
//! The header is a 2×N pin block: a top row and a bottom row of pads, grouped
//! into [`HeaderColumn`]s numbered from 1 at the board's left edge. Some boards
//! add an extra third-row pad below specific columns (the X pins), captured by
//! [`HeaderColumn::row3`]. Absent columns (e.g. a revision missing its
//! left-most 5V/GND pair) are simply omitted, so the columns that *are* present
//! keep their absolute positions and stay aligned across revisions when drawn.

/// A function carried by a single physical header pad.
///
/// A pad may carry up to two roles at once where a signal is multiplexed onto
/// it — for example an image-select line that doubles as an SWD debug pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderRole {
    /// 5V supply.
    Power5V,
    /// Ground.
    Gnd,
    /// RUN / reset line.
    Run,
    /// BOOTSEL line.
    Bootsel,
    /// Image-select line carrying the given bit weight (0 = jumper A = the
    /// least-significant image-select bit).
    Select(u8),
    /// SWD clock (always multiplexed onto an image-select line).
    Swclk,
    /// SWD data (always multiplexed onto an image-select line).
    Swdio,
    /// X1 socket pin.
    X1,
    /// X2 socket pin.
    X2,
    /// A high address line broken out on the header (the given line number, e.g.
    /// `Addr(17)` = A17). Occupies the extra pad row on boards that expose it
    /// (32-pin boards) where smaller boards put an X pin instead.
    Addr(u8),
}

/// The state of one pad position (row) within a [`HeaderColumn`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderSlot {
    /// No pad is fitted at this position.
    NotPopulated,
    /// A pad is fitted but is not connected to anything.
    NotConnected,
    /// A pad is fitted and carries one or two [`HeaderRole`]s.
    Roles(&'static [HeaderRole]),
}

/// One column of the header: a top/bottom pad pair, plus an optional third-row
/// pad on boards that have one.
///
/// Columns are numbered from 1 at the left edge of the board. [`Self::row1`] is
/// the top pad and [`Self::row2`] the bottom pad of the standard 2×N header;
/// [`Self::row3`] is an extra pad sitting below the column on boards with X
/// pins, and is `None` where no such pad physically exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderColumn {
    /// Absolute column position, 1-based from the board's left edge.
    pub col: u8,
    /// Top-row pad.
    pub row1: HeaderSlot,
    /// Bottom-row pad.
    pub row2: HeaderSlot,
    /// Optional third-row pad, present only where one physically exists.
    pub row3: Option<HeaderSlot>,
}

/// Physical description of a board's jumper header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumperHeader {
    /// The columns present on the header, in ascending [`HeaderColumn::col`]
    /// order. Absent columns are omitted rather than listed, so drawn positions
    /// stay aligned across board revisions.
    pub columns: &'static [HeaderColumn],
}
