// Copyright (C) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! ASCII renderers for a One ROM board's physical pin layouts.
//!
//! Two views are produced, both driven purely from the static board metadata in
//! [`onerom_config`] (no device required):
//!
//! - [`render_pin_header`] draws the jumper / programming header (the 2xN header
//!   along the board's top edge), pad by pad, annotating each image-select and X
//!   pad with the MCU GPIO behind it and — on RP2350 boards — whether that GPIO
//!   is 5V-tolerant or 3.3V-only (an ADC pin).
//! - [`render_rom_socket`] draws the ROM socket as a DIP pinout. Without a chip
//!   type it labels each socket pin with the GPIO(s) behind it; given a chip
//!   type it labels each pin with that ROM's function (address/data/CS/…), and
//!   `--gpio` overlays both.
//!
//! The functions return the rendered block as a `String` so they are trivial to
//! unit-test and reuse from both `boards` and `inspect` commands.
//!
//! The same board metadata answers the per-GPIO question `inspect gpio` asks —
//! "what is GPIO 23 on this board, and what is it under the ROM being served?"
//! — so the [GPIO naming](#gpio-naming) helpers at the end of this module reuse
//! the renderers' own lookups rather than deriving the answer a second time. The
//! device deliberately reports only a coarse use category and never a role name,
//! so naming is entirely the host's job.

use onerom_config::chip::{CHIP_TYPES, ChipType};
use onerom_config::hw::{Board, HeaderColumn, HeaderRole, HeaderSlot};
use onerom_config::mcu::PinTolerance;
use onerom_gen::ChipSetType;
use onerom_gen::compat::{check_chip_set_on_board, default_cs_config, format_size};
use onerom_gen::socket_pin_offset;

/// Inner text width of a header pad, between its side walls' margin spaces.
const PAD_W: usize = 9;

/// Blank interior of the DIP body drawn by the socket view.
const BODY_W: usize = 17;

// ---------------------------------------------------------------------------
// Small string helpers
// ---------------------------------------------------------------------------

/// Left-align `s` within `w` columns (no truncation; over-long strings are
/// returned as-is).
fn left(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

// ===========================================================================
// Pin header
// ===========================================================================

/// The short label shown on a header pad for a single role.
fn header_role_label(role: &HeaderRole) -> String {
    match role {
        HeaderRole::Power5V => "5V".to_string(),
        HeaderRole::Gnd => "GND".to_string(),
        HeaderRole::Run => "RUN".to_string(),
        HeaderRole::Bootsel => "BOOTSEL".to_string(),
        HeaderRole::Select(b) => format!("SEL_{}", (b'A' + *b) as char),
        HeaderRole::Swclk => "SWCLK".to_string(),
        HeaderRole::Swdio => "SWDIO".to_string(),
        HeaderRole::X1 => "X1".to_string(),
        HeaderRole::X2 => "X2".to_string(),
        HeaderRole::Addr(n) => format!("A{n}"),
    }
}

/// The MCU GPIO behind a role, where one exists (image-select, X and broken-out
/// address pads carry a GPIO; power/ground/SWD/control pads do not).
#[allow(clippy::wildcard_enum_match_arm)]
fn header_role_gpio(board: &Board, role: &HeaderRole) -> Option<u8> {
    match role {
        HeaderRole::Select(b) => board.sel_pins().get(*b as usize).copied(),
        HeaderRole::X1 => (board.pin_x1() != 255).then(|| board.pin_x1()),
        HeaderRole::X2 => (board.pin_x2() != 255).then(|| board.pin_x2()),
        HeaderRole::Addr(n) => board.addr_pins().get(*n as usize).copied(),
        _ => None,
    }
}

/// The one-word tolerance tag shown under a pad's GPIO.
fn tolerance_tag(t: PinTolerance) -> &'static str {
    match t {
        PinTolerance::FiveVolt => "5V",
        PinTolerance::ThreeVolt3 => "!!3V3!!",
    }
}

/// The content tokens for one header pad (role label(s), then the GPIO, then its
/// tolerance), top to bottom. The physical header is not silkscreened with pin
/// numbers, so none are shown.
fn header_pad_tokens(board: &Board, slot: &HeaderSlot) -> Vec<String> {
    match slot {
        // Unpopulated positions are never drawn (the caller skips them), so they
        // contribute no content.
        HeaderSlot::NotPopulated => Vec::new(),
        HeaderSlot::NotConnected => vec!["n/c".to_string()],
        HeaderSlot::Roles(roles) => {
            let mut tokens: Vec<String> = roles.iter().map(header_role_label).collect();
            if let Some(gpio) = roles.iter().find_map(|r| header_role_gpio(board, r)) {
                tokens.push(format!("GPIO{gpio}"));
                if let Some(tol) = board.gpio_tolerance(gpio) {
                    tokens.push(tolerance_tag(tol).to_string());
                }
            }
            tokens
        }
    }
}

/// Render one pad's content lines to exactly `height` rows of `PAD_W` columns:
/// each token left-aligned, top to bottom, padded with blank rows to `height`.
fn header_pad_lines(tokens: &[String], height: usize) -> Vec<String> {
    (0..height)
        .map(|i| left(tokens.get(i).map(String::as_str).unwrap_or(""), PAD_W))
        .collect()
}

/// A single 13-column header cell (walls + margins) around `content`.
fn hcell(content: &str) -> String {
    format!("│ {content} │")
}

/// Cell-width blank used for absent columns / rows.
fn hgap() -> String {
    " ".repeat(PAD_W + 4)
}

const HTOP: &str = "┌───────────┐";
const HDIV: &str = "├───────────┤";
const HBOT: &str = "└───────────┘";

/// Render the pin (jumper / programming) header for `board`.
///
/// Returns `None` if the board has no `jumper_header` descriptor yet, so the
/// caller can print a "not characterised" notice rather than an empty diagram.
#[allow(clippy::wildcard_enum_match_arm)]
pub fn render_pin_header(board: &Board) -> Option<String> {
    let header = board.jumper_header()?;

    let mcu = board
        .rp_variant()
        .map(|v| v.to_string())
        .unwrap_or_else(|| board.mcu_family().to_string());

    let cols: &[HeaderColumn] = header.columns;
    let max_col = cols.iter().map(|c| c.col).max().unwrap_or(0);
    let by_col = |n: u8| cols.iter().find(|c| c.col == n);
    let rp = board.rp_variant().is_some();
    let present = |slot: &HeaderSlot| !matches!(slot, HeaderSlot::NotPopulated);

    // Columns keep their absolute physical position: an unpopulated position
    // (e.g. a USB variant's missing 5V/GND column) is drawn as empty space in
    // place, never shifted away. The header carries no silkscreened pin numbers,
    // so none are drawn.
    let row1_present = |n: u8| by_col(n).is_some_and(|c| present(&c.row1));
    let row2_present = |n: u8| by_col(n).is_some_and(|c| present(&c.row2));
    let row3_present = |n: u8| by_col(n).and_then(|c| c.row3.as_ref()).is_some_and(present);
    let any_row3 = (1..=max_col).any(row3_present);

    // One uniform pad height across the whole header, so every box is the same
    // size regardless of how many lines its label needs.
    let mut pad_h = 1usize;
    for c in cols {
        if present(&c.row1) {
            pad_h = pad_h.max(header_pad_tokens(board, &c.row1).len());
        }
        if present(&c.row2) {
            pad_h = pad_h.max(header_pad_tokens(board, &c.row2).len());
        }
        if let Some(x) = &c.row3
            && present(x)
        {
            pad_h = pad_h.max(header_pad_tokens(board, x).len());
        }
    }

    // Join a per-column cell builder across columns 1..=max_col.
    let row = |f: &dyn Fn(u8) -> String| -> String {
        (1..=max_col)
            .map(f)
            .collect::<Vec<_>>()
            .join(" ")
            .trim_end()
            .to_string()
    };

    let mut out = String::new();
    out.push_str(&format!(
        "Pin header  ·  {}  ·  {mcu}\n",
        board.description()
    ));
    out.push_str(
        "Viewed from above (component side up) — header runs along the board's top edge.\n\n",
    );

    // Orientation marker at the header's pin-1 corner (top-left). The header
    // pads are not numbered; this is purely an orientation aid, and on a board
    // whose pin-1 position is unpopulated it marks where that corner is.
    out.push_str("  ◄ pin 1\n");

    // Top borders.
    out.push_str("  ");
    out.push_str(&row(&|n| {
        if row1_present(n) {
            HTOP.to_string()
        } else {
            hgap()
        }
    }));
    out.push('\n');

    // Top pad content.
    for k in 0..pad_h {
        out.push_str("  ");
        out.push_str(&row(&|n| {
            if row1_present(n) {
                let toks = header_pad_tokens(board, &by_col(n).unwrap().row1);
                hcell(&header_pad_lines(&toks, pad_h)[k])
            } else {
                hgap()
            }
        }));
        out.push('\n');
    }

    // Divider band: closes and/or opens each column's boxes depending on which
    // of its two pads are fitted.
    out.push_str("  ");
    out.push_str(&row(&|n| match (row1_present(n), row2_present(n)) {
        (true, true) => HDIV.to_string(),
        (true, false) => HBOT.to_string(),
        (false, true) => HTOP.to_string(),
        (false, false) => hgap(),
    }));
    out.push('\n');

    // Bottom pad content.
    for k in 0..pad_h {
        out.push_str("  ");
        out.push_str(&row(&|n| {
            if row2_present(n) {
                let toks = header_pad_tokens(board, &by_col(n).unwrap().row2);
                hcell(&header_pad_lines(&toks, pad_h)[k])
            } else {
                hgap()
            }
        }));
        out.push('\n');
    }

    // Bottom borders.
    out.push_str("  ");
    out.push_str(&row(&|n| {
        if row2_present(n) {
            HBOT.to_string()
        } else {
            hgap()
        }
    }));
    out.push('\n');

    // Optional third-row pads (X pins, or high address lines broken out on
    // 32-pin boards), drawn under the columns that carry them.
    if any_row3 {
        out.push_str("  ");
        out.push_str(&row(&|n| {
            if row3_present(n) {
                HTOP.to_string()
            } else {
                hgap()
            }
        }));
        out.push('\n');
        for k in 0..pad_h {
            out.push_str("  ");
            out.push_str(&row(&|n| {
                if row3_present(n) {
                    let x = by_col(n).unwrap().row3.as_ref().unwrap();
                    hcell(&header_pad_lines(&header_pad_tokens(board, x), pad_h)[k])
                } else {
                    hgap()
                }
            }));
            out.push('\n');
        }
        out.push_str("  ");
        out.push_str(&row(&|n| {
            if row3_present(n) {
                HBOT.to_string()
            } else {
                hgap()
            }
        }));
        out.push('\n');
    }

    // Legend — driven by the roles actually present on this board's header.
    let roles_present = |pred: &dyn Fn(&HeaderRole) -> bool| -> bool {
        cols.iter()
            .flat_map(|c| [Some(&c.row1), Some(&c.row2), c.row3.as_ref()])
            .flatten()
            .any(|slot| match slot {
                HeaderSlot::Roles(rs) => rs.iter().any(pred),
                _ => false,
            })
    };
    let has_x = roles_present(&|r| matches!(r, HeaderRole::X1 | HeaderRole::X2));
    let has_addr = roles_present(&|r| matches!(r, HeaderRole::Addr(_)));
    let has_nc = cols
        .iter()
        .flat_map(|c| [Some(&c.row1), Some(&c.row2), c.row3.as_ref()])
        .flatten()
        .any(|slot| matches!(slot, HeaderSlot::NotConnected));

    out.push('\n');
    if rp {
        out.push_str("  !!3V3!! = 3.3V-only (ADC pin, keep ≤3.3V)    5V = 5V-tolerant\n");
    }
    out.push_str("  SEL_A = image-select bit 0 (LSB); each further letter is the next bit\n");
    if has_x {
        out.push_str("  X1/X2 = jumper X pins\n");
    }
    if has_addr {
        out.push_str("  A<n> = high address line broken out on the header\n");
    }
    if has_nc {
        out.push_str("  n/c = pad fitted but not connected\n");
    }

    Some(out)
}

// ===========================================================================
// ROM socket
// ===========================================================================

/// The ROM's function(s) for a socket pin, for a given chip type (e.g. `A12`,
/// `D3`, `CS1`, `CE`, `BYTE`, `VCC`, `GND`, `VPP`). `None` for a pin the chip
/// does not define.
///
/// A pin can carry more than one function on parts with a multiplexed pinout —
/// the 27C400's pin 29 is address A0 in byte mode and data D15 in word mode, for
/// example — in which case the functions are joined with `/` (e.g. `A0/D15`).
fn socket_function(chip: ChipType, pin: u8) -> Option<String> {
    let mut funcs: Vec<String> = Vec::new();
    for (i, &p) in chip.address_pins().iter().enumerate() {
        if p == pin {
            funcs.push(format!("A{i}"));
        }
    }
    for (i, &p) in chip.data_pins().iter().enumerate() {
        if p == pin {
            funcs.push(format!("D{i}"));
        }
    }
    for c in chip.control_lines().iter().filter(|c| c.pin == pin) {
        funcs.push(c.name.to_ascii_uppercase());
    }
    for p in chip.power_pins().iter().filter(|p| p.pin == pin) {
        funcs.push(p.name.to_ascii_uppercase());
    }
    if let Some(pins) = chip.programming_pins() {
        for p in pins.iter().filter(|p| p.pin == pin) {
            funcs.push(p.name.to_ascii_uppercase());
        }
    }
    (!funcs.is_empty()).then(|| funcs.join("/"))
}

/// The GPIO(s) behind a socket pin, formatted (e.g. `GPIO16` or `GPIO16/GPIO40`),
/// or `None` if the pin maps to no GPIO (a power / not-connected pin).
fn socket_gpios(board: &Board, pin: u8) -> Option<String> {
    let gpios = board.gpios_for_socket_pin(pin);
    if gpios.is_empty() {
        None
    } else {
        Some(
            gpios
                .iter()
                .map(|g| format!("GPIO{g}"))
                .collect::<Vec<_>>()
                .join("/"),
        )
    }
}

/// One ROM's own power role (`VCC`/`GND`) at a board socket pin, or `None` if the
/// pin is not one of the board's power pins.
///
/// Derived from the board's native-size ROM chip types (which agree on the power
/// pins for that package). Restricting to the native pin count avoids picking up
/// a differently-sized overhang/fly-lead chip's power pin at the wrong position.
fn board_power_name(board: &Board, pin: u8) -> Option<String> {
    for chip in CHIP_TYPES {
        if chip.chip_pins() == board.chip_pins()
            && board.supports_chip_type(*chip)
            && let Some(p) = chip.power_pins().iter().find(|p| p.pin == pin)
        {
            return Some(p.name.to_ascii_uppercase());
        }
    }
    None
}

/// Render the ROM socket pinout for `board`.
///
/// With no `chip` the board's own socket is drawn, each pin labelled with the
/// GPIO(s) behind it. With a `chip` the socket is drawn at the larger of the
/// board's and the chip's pin counts, the smaller device bottom-justified in the
/// middle (matching [`socket_pin_offset`]):
///
/// - a smaller ROM on a bigger One ROM leaves One ROM pins hanging out of the
///   socket, labelled `overhang`;
/// - a bigger ROM on a smaller One ROM leaves socket pins One ROM does not
///   reach, labelled `(empty)`, with any address line there fly-leaded to
///   `X1`/`X2` (in address-pin order, matching the firmware).
///
/// `show_gpio` appends the GPIO(s) behind each reachable pin (including overhang
/// pins). The caller validates that `chip`, when given, is one the board accepts.
pub fn render_rom_socket(board: &Board, chip: Option<ChipType>, show_gpio: bool) -> String {
    let bp = board.chip_pins() as i16;
    let cp = chip.map(|c| c.chip_pins() as i16).unwrap_or(bp);
    let n = bp.max(cp);
    // Both devices are centred in the N-pin frame; `offset` relates the two pin
    // numberings (board socket pin = chip pin + offset).
    let chip_lo = (n - cp) / 2;
    let board_lo = (n - bp) / 2;
    let offset = chip_lo - board_lo;

    // Fly-lead assignment: each ROM address line overhanging the board's socket
    // is wired to the next X pin (X1 then X2), in address-pin order.
    let mut fly: Vec<(i16, &'static str)> = Vec::new();
    if let Some(c) = chip {
        for &ap in c.address_pins() {
            let ap = ap as i16;
            if (ap + offset < 1 || ap + offset > bp)
                && let Some(x) = ["X1", "X2"].get(fly.len())
            {
                fly.push((ap, x));
            }
        }
    }

    let label = |pos: i16| -> String {
        let chip_pin = pos - chip_lo;
        let board_pin = pos - board_lo;
        let cv = chip.is_some() && (1..=cp).contains(&chip_pin);
        let bv = (1..=bp).contains(&board_pin);
        // What One ROM has behind this pin: a GPIO for a signal pin, else its own
        // power role (VCC/GND) - which matters when it lands on a ROM pin that is
        // not the matching power pin (e.g. One ROM's VCC on the ROM's NC pin).
        let behind = bv
            .then(|| {
                socket_gpios(board, board_pin as u8)
                    .or_else(|| board_power_name(board, board_pin as u8))
            })
            .flatten();
        let with_gpio = |primary: String| match (show_gpio, &behind) {
            // Suppress the annotation when it just repeats the ROM function
            // (e.g. the ROM's VCC pin sitting on One ROM's VCC pin).
            (true, Some(b)) if *b != primary => format!("{primary} ({b})"),
            _ => primary,
        };
        match chip {
            // GPIO map: the GPIO(s), or a power label where the pin has none.
            None => behind.unwrap_or_else(|| "—".to_string()),
            Some(c) => {
                let func = cv.then(|| socket_function(c, chip_pin as u8)).flatten();
                match (cv, bv) {
                    // Reachable pin: ROM function (or NC), plus GPIO if asked.
                    (true, true) => with_gpio(func.unwrap_or_else(|| "NC".to_string())),
                    // One ROM pin outside the (smaller) ROM socket.
                    (false, true) => with_gpio("overhang".to_string()),
                    // Socket pin One ROM does not reach.
                    (true, false) => match fly.iter().find(|(ap, _)| *ap == chip_pin) {
                        Some((_, x)) => format!("{} → {x}", func.unwrap_or_default()),
                        None => match func {
                            Some(f) => format!("{f} (empty)"),
                            None => "(empty)".to_string(),
                        },
                    },
                    (false, false) => "—".to_string(),
                }
            }
        }
    };

    let n = n as usize;
    let half = n / 2;
    let left: Vec<(usize, String)> = (1..=half).map(|p| (p, label(p as i16))).collect();
    let right: Vec<(usize, String)> = ((half + 1)..=n)
        .rev()
        .map(|p| (p, label(p as i16)))
        .collect();
    let lw = left
        .iter()
        .map(|(_, l)| l.chars().count())
        .max()
        .unwrap_or(0);
    let indent = lw + 7; // width of "{label:>lw}  {pin:>2} ──" before the '┤'

    let mut out = String::new();
    let title = match chip {
        None => format!(
            "ROM socket  ·  {}  ·  GPIO map (no ROM type given)",
            board.description()
        ),
        Some(c) => {
            let gpio_suffix = if show_gpio { ", with GPIOs" } else { "" };
            let geom = if bp > cp {
                format!(" (One ROM overhangs the {}-pin socket)", cp)
            } else if bp < cp {
                format!(
                    " ({}-pin socket; One ROM at pins {}–{})",
                    cp,
                    board_lo + 1,
                    board_lo + bp
                )
            } else {
                String::new()
            };
            format!(
                "ROM socket  ·  {}  ·  as {}{gpio_suffix}{geom}",
                board.description(),
                c.name()
            )
        }
    };
    out.push_str(&title);
    out.push_str("\n\n");

    // Top border.
    out.push_str(&format!("{}┌{}┐\n", " ".repeat(indent), "─".repeat(BODY_W)));

    // Pin rows.
    for i in 0..half {
        let (lp, ll) = &left[i];
        let (rp, rl) = &right[i];
        out.push_str(&format!(
            "{ll:>lw$}  {lp:>2} ──┤{body}├── {rp:>2}  {rl}\n",
            body = " ".repeat(BODY_W),
        ));
    }

    // Bottom border.
    out.push_str(&format!("{}└{}┘\n", " ".repeat(indent), "─".repeat(BODY_W)));

    // Notes.
    out.push('\n');
    if let Some(c) = chip {
        // The flash this chip costs on this board - the same figure
        // `onerom chips` and docs/COMPATIBILITY.md report.
        if let Ok(result) =
            check_chip_set_on_board(*board, c, ChipSetType::Single, 1, default_cs_config(c))
        {
            out.push_str(&format!(
                "  Image size {} (ROM size {}) — the flash One ROM uses to emulate this chip.\n",
                format_size(result.slot_size_bytes),
                format_size(c.size_bytes() as u32),
            ));
        }
    }
    match chip {
        None => out.push_str(
            "  Add --chip-type <chip> to show ROM pin functions (A/D/CS/…) instead of GPIOs.\n",
        ),
        Some(_) if bp > cp => out.push_str(
            "  'overhang' pins are One ROM pins outside the socket — reroute power to One ROM's \
             VCC/5V pin (see COMPATIBILITY.md).\n",
        ),
        Some(_) if bp < cp => out.push_str(
            "  '(empty)' pins are socket positions One ROM does not reach — the socket's VCC is \
             one of them, so power One ROM's own VCC/5V pin instead (shown in the '(VCC)' \
             annotation). '→ X1/X2' address lines need a fly-lead to that One ROM header pin \
             (see COMPATIBILITY.md).\n",
        ),
        Some(_) => {}
    }

    out
}

// ===========================================================================
// GPIO naming
// ===========================================================================
//
// `inspect gpio` asks the board metadata the same questions the two renderers
// above ask, but one GPIO at a time. These helpers are the per-GPIO form, built
// on the very same lookups (`header_role_gpio`, `socket_function`,
// `socket_pin_offset`) so a change to how a pad or a socket pin is named shows
// up in the diagram and the table together.

/// The header pad role(s) an MCU GPIO *is*, e.g. `SEL_A`, `X1`, `A12`.
///
/// Only roles that have a GPIO behind them are named. A pad may carry more than
/// one role — on a Fire 24/28 board the SEL_C and SEL_D pads sit on the SWCLK
/// and SWDIO nets — but SWCLK and SWDIO are dedicated RP2350 pins, not GPIOs, so
/// naming GPIO 25 `SEL_C/SWCLK` would assert something untrue of the GPIO. That
/// a pad shares a net with a debug probe is a fact about the pad; this answer is
/// indexed by GPIO. (The header diagram is indexed by pad and does show every
/// role — see [`render_pin_header`].) Where a GPIO genuinely is more than one
/// role, the roles are joined with `/`.
///
/// `jumper_header` is populated for the Fire 24/28/32 boards but not yet for
/// Fire 40 or any Ice board, so an uncharacterised board falls back to the
/// electrical pin arrays, which name the image-select and X pads without
/// claiming to know where on the header they sit. Callers that show the physical
/// header layout must still check
/// [`Board::jumper_header`](onerom_config::hw::Board::jumper_header) themselves;
/// this function degrades to naming rather than to nothing.
///
/// `None` means no pad carries this GPIO.
#[allow(clippy::wildcard_enum_match_arm)]
pub fn gpio_header_role(board: &Board, gpio: u8) -> Option<String> {
    // The board pin arrays use 255 for "no such pin", and no real GPIO number
    // reaches it, so it must never match one of those sentinels.
    if gpio == 255 {
        return None;
    }

    if let Some(header) = board.jumper_header() {
        let pad = header
            .columns
            .iter()
            .flat_map(|c| [Some(&c.row1), Some(&c.row2), c.row3.as_ref()])
            .flatten()
            .find_map(|slot| match slot {
                HeaderSlot::Roles(roles) => {
                    // Roles with no GPIO of their own (SWCLK, SWDIO, RUN,
                    // BOOTSEL, power, ground) are dropped: they belong to the
                    // pad, not to this GPIO.
                    let named: Vec<String> = roles
                        .iter()
                        .filter(|r| header_role_gpio(board, r) == Some(gpio))
                        .map(header_role_label)
                        .collect();
                    (!named.is_empty()).then(|| named.join("/"))
                }
                _ => None,
            });
        if pad.is_some() {
            return pad;
        }
    }

    // Uncharacterised header, or a GPIO on no pad of a characterised one. The
    // pin arrays are electrical facts and hold either way.
    if let Some(bit) = board.sel_pins().iter().position(|&p| p == gpio) {
        return Some(header_role_label(&HeaderRole::Select(bit as u8)));
    }
    if board.pin_x1() == gpio {
        return Some(header_role_label(&HeaderRole::X1));
    }
    if board.pin_x2() == gpio {
        return Some(header_role_label(&HeaderRole::X2));
    }
    None
}

/// The ROM function of an MCU GPIO under `chip`, e.g. `A5`, `D3`, `CS1`, `BYTE`.
///
/// The board's socket pin numbering and the chip's own differ whenever the two
/// pin counts differ, so the chip pin is recovered through
/// [`socket_pin_offset`] — the same geometry [`render_rom_socket`] draws with.
/// `None` for a GPIO that is not on the ROM socket at all, for a socket pin
/// outside a smaller chip's body, and for a board/chip pin-count combination
/// that has no defined placement.
pub fn gpio_rom_function(board: &Board, chip: ChipType, gpio: u8) -> Option<String> {
    let socket_pin = board.socket_pin_for_gpio(gpio)?;
    let offset = socket_pin_offset(chip.chip_pins(), board.chip_pins())?;
    // socket_pin = chip_pin + offset.
    let chip_pin = i16::from(socket_pin) - offset;
    if chip_pin < 1 || chip_pin > i16::from(chip.chip_pins()) {
        return None;
    }
    socket_function(chip, chip_pin as u8)
}

/// Every One ROM system function of an MCU GPIO; empty if it has none.
///
/// These are the pins the firmware reports as `SYSTEM`: the status LED, the
/// NeoPixel, the USB VBUS sense line and the external flash chip-select. The
/// board data uses 255 as "no such pin", which no real GPIO number reaches.
///
/// A GPIO can carry more than one of them — on `fire-24-f` the status LED and
/// the NeoPixel are both GPIO 29, which is exactly why the RGB plugin reflects
/// the status-LED state on that shared LED — so this answers with all of them.
/// Stopping at the first would report half the truth about the pin most likely
/// to be driven by accident.
pub fn gpio_system_functions(board: &Board, gpio: u8) -> Vec<&'static str> {
    let mut functions = Vec::new();
    if gpio == 255 {
        return functions;
    }
    if board.pin_status() == gpio {
        functions.push("Status LED");
    }
    if board.pin_neo() == Some(gpio) {
        functions.push("RGB LED");
    }
    if board.usb_vbus_pin() == Some(gpio) {
        functions.push("USB VBUS");
    }
    if board.external_flash_cs_pin() == Some(gpio) {
        functions.push("ext flash CS");
    }
    functions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Board {
        Board::try_from_str("fire-24-f").unwrap()
    }

    #[test]
    fn pin_header_flags_adc_select_pins() {
        let s = render_pin_header(&board()).expect("fire-24-f has a header");
        // SEL_A/SEL_B sit behind the RP2350A ADC pins GPIO26/27 → 3.3V-only.
        assert!(s.contains("SEL_A"));
        assert!(s.contains("GPIO26"));
        assert!(s.contains("!!3V3!!"));
        // SEL_C/SEL_D (GPIO25/24) are the SWD-muxed, 5V-tolerant pair.
        assert!(s.contains("SWDIO"));
        assert!(s.contains("GPIO24"));
        assert!(s.contains("5V = 5V-tolerant"));
        // X pins appear on the third row.
        assert!(s.contains("X1"));
        assert!(s.contains("GPIO9"));
    }

    #[test]
    fn pin_header_keeps_unpopulated_column_in_place_without_numbers() {
        // fire-24-usb-b's left column is unpopulated (n/p): it is drawn as empty
        // space in its physical position (the header is NOT shifted left), the
        // marker still marks the pin-1 corner, and pads carry no pin numbers.
        let b = Board::try_from_str("fire-24-usb-b").unwrap();
        let s = render_pin_header(&b).expect("fire-24-usb-b has a header");
        assert!(!s.contains("n/p"));
        assert!(s.contains("  ◄ pin 1\n"));
        // Roles sit flush-left in their cell, with no pin number ahead of them.
        assert!(s.contains("│ SEL_D"));
        // The unpopulated first column keeps its position, so the top border is
        // indented past the left margin rather than starting flush at it.
        let border = s.lines().find(|l| l.contains('┌')).unwrap();
        assert!(!border.starts_with("  ┌"));
    }

    #[test]
    fn pin_header_populated_first_column_starts_flush() {
        // fire-24-f has a populated first column, so its top border starts flush
        // at the left margin, and roles are flush-left (no pin numbers).
        let s = render_pin_header(&board()).expect("fire-24-f has a header");
        let border = s.lines().find(|l| l.contains('┌')).unwrap();
        assert!(border.starts_with("  ┌"));
        assert!(s.contains("│ 5V"));
        assert!(s.contains("│ SEL_A"));
    }

    #[test]
    fn pin_header_keeps_not_connected_pads_with_legend() {
        // fire-24-a fits pads that are wired to nothing (n/c) - these stay, and
        // get a legend line explaining them.
        let b = Board::try_from_str("fire-24-a").unwrap();
        let s = render_pin_header(&b).expect("fire-24-a has a header");
        assert!(s.contains("n/c"));
        assert!(s.contains("n/c = pad fitted but not connected"));
    }

    #[test]
    fn pin_header_row3_pads_carry_no_pin_marker() {
        // Third-row pads are labelled by role only (X1/X2 or A<n>), with no
        // stray "X" pin marker in the pad's number position.
        let s = render_pin_header(&board()).expect("fire-24-f has a header");
        assert!(!s.contains("│ X "));
        assert!(s.contains("X1"));
        assert!(s.contains("X2"));
    }

    #[test]
    fn pin_header_none_when_uncharacterised() {
        // Ice boards have no jumper_header descriptor yet.
        let ice = Board::try_from_str("ice-24-f").unwrap();
        assert!(ice.jumper_header().is_none());
        assert!(render_pin_header(&ice).is_none());
    }

    #[test]
    fn socket_gpio_map_has_no_functions() {
        let s = render_rom_socket(&board(), None, false);
        assert!(s.contains("GPIO16"));
        assert!(s.contains("GPIO map"));
        // Power pins still labelled from the board's accepted chip types.
        assert!(s.contains("VCC"));
        assert!(s.contains("GND"));
        // No ROM functions without a --type.
        assert!(!s.contains(" A7 "));
        assert!(!s.contains("CS1"));
    }

    #[test]
    fn socket_function_view_matches_2364_pinout() {
        let s = render_rom_socket(&board(), Some(ChipType::Chip2364), false);
        assert!(s.contains("as 2364"));
        // Canonical 2364 pinout landmarks.
        assert!(s.contains("A7")); // pin 1
        assert!(s.contains("CS1")); // pin 20
        assert!(s.contains("A12")); // pin 21
        assert!(s.contains("VCC")); // pin 24
        // No GPIOs unless --gpio.
        assert!(!s.contains("GPIO"));
    }

    #[test]
    fn socket_function_with_gpio_overlays_both() {
        let s = render_rom_socket(&board(), Some(ChipType::Chip2364), true);
        assert!(s.contains("with GPIOs"));
        assert!(s.contains("A7 (GPIO16)"));
        assert!(s.contains("CS1 (GPIO10)"));
    }

    #[test]
    fn socket_no_notch_or_footer() {
        let s = render_rom_socket(&board(), None, false);
        assert!(!s.contains('∪'));
        assert!(!s.contains("top→bottom"));
    }

    #[test]
    fn socket_overhang_smaller_chip_on_larger_board() {
        // A 24-pin 2364 on a 28-pin board: One ROM overhangs the 24-pin socket,
        // so the chip sits at pins 3-26 and pins 1,2,27,28 are 'overhang'.
        let b = Board::try_from_str("fire-28-c").unwrap();
        let s = render_rom_socket(&b, Some(ChipType::Chip2364), true);
        assert!(s.contains("One ROM overhangs the 24-pin socket"));
        assert!(s.contains("overhang"));
        // The 2364 is bottom-justified: its A7 (chip pin 1) lands on board pin 3.
        let a7 = s.lines().find(|l| l.contains(" A7 ")).unwrap();
        assert!(a7.contains("  3 "));
        // Overhang pins still show their board GPIO when --gpio is set.
        assert!(s.contains("overhang (GPIO"));
        // One ROM's own VCC pin lands on an overhang pin, and is annotated so.
        assert!(s.contains("overhang (VCC)"));
    }

    #[test]
    fn socket_flylead_larger_chip_on_smaller_board() {
        // A 28-pin 2764 on a 24-pin board: the socket is 28-pin, One ROM sits at
        // pins 3-26, and the 2764's A12 overhangs → fly-lead to X1.
        let b = Board::try_from_str("fire-24-f").unwrap();
        let s = render_rom_socket(&b, Some(ChipType::Chip2764), true);
        assert!(s.contains("28-pin socket; One ROM at pins 3–26"));
        assert!(s.contains("A12 → X1"));
        assert!(s.contains("(empty)"));
        // The socket's VCC is unreachable; One ROM's own VCC lands on the ROM's
        // NC pin and is annotated, and the note explains the power reroute.
        assert!(s.contains("NC (VCC)"));
        assert!(s.contains("power One ROM's own VCC/5V pin"));
    }

    #[test]
    fn socket_2316_shows_three_chip_selects() {
        // The 2316 relabels pins 18/21 (A11/A12 on the 2364) as CS2/CS3.
        let s = render_rom_socket(&board(), Some(ChipType::Chip2316), false);
        assert!(s.contains("CS1"));
        assert!(s.contains("CS2"));
        assert!(s.contains("CS3"));
    }

    #[test]
    fn pin_header_32pin_flags_rp235xb_adc_and_address_lines() {
        // fire-32-b is an RP235xB board: its ADC pins are GPIO40-47, and its
        // header breaks out high address lines on the third row.
        let b = Board::try_from_str("fire-32-b").unwrap();
        let s = render_pin_header(&b).expect("fire-32-b has a header");
        // SEL_A/SEL_B behind GPIO40/41 (RP235xB ADC) are flagged 3.3V-only.
        assert!(s.contains("GPIO40"));
        assert!(s.contains("!!3V3!!"));
        // Address lines, not X pins, on the third row → the addr legend, not X.
        assert!(s.contains("high address line broken out"));
        assert!(!s.contains("jumper X pins"));
    }

    #[test]
    fn socket_27c400_is_16bit_with_byte_pin() {
        // 27C400 is a 40-pin 16-bit EPROM: D0-D15, a /BYTE pin, and CE/OE.
        let b = Board::try_from_str("fire-40-b").unwrap();
        let s = render_rom_socket(&b, Some(ChipType::Chip27C400), false);
        assert!(s.contains("D15"));
        assert!(s.contains("BYTE"));
        assert!(s.contains("CE"));
        assert!(s.contains("OE"));
    }

    #[test]
    fn gpio_header_role_names_only_what_the_gpio_is() {
        let b = board();
        assert_eq!(gpio_header_role(&b, 26).as_deref(), Some("SEL_A"));
        assert_eq!(gpio_header_role(&b, 9).as_deref(), Some("X1"));
        assert_eq!(gpio_header_role(&b, 8).as_deref(), Some("X2"));
        // GPIO25/24 sit on the pads that also carry the SWCLK/SWDIO nets, but
        // those are dedicated RP2350 pins rather than GPIOs, so the GPIO is
        // named SEL_C/SEL_D alone. The pad-indexed header diagram still shows
        // both roles.
        assert_eq!(gpio_header_role(&b, 25).as_deref(), Some("SEL_C"));
        assert_eq!(gpio_header_role(&b, 24).as_deref(), Some("SEL_D"));
        let header = render_pin_header(&b).expect("fire-24-f has a header");
        assert!(header.contains("SWDIO"), "{header}");
        // A data pin is on no header pad.
        assert_eq!(gpio_header_role(&b, 0), None);
    }

    #[test]
    fn gpio_header_role_degrades_without_a_jumper_header() {
        // Some Ice boards still have no jumper_header descriptor, so the pad
        // names come from the electrical pin arrays instead of nothing.
        let b = Board::try_from_str("ice-24-d").unwrap();
        assert!(b.jumper_header().is_none());
        let sel_a = b.sel_pins()[0];
        assert_eq!(gpio_header_role(&b, sel_a).as_deref(), Some("SEL_A"));
        // ice-24-d has no X pins, so nothing invents one.
        assert_eq!(b.pin_x1(), 255);
        assert_eq!(gpio_header_role(&b, 255), None);
    }

    #[test]
    fn gpio_rom_function_matches_the_socket_diagram() {
        let b = board();
        // The same facts the socket view asserts: A7 on GPIO16, CS1 on GPIO10.
        assert_eq!(
            gpio_rom_function(&b, ChipType::Chip2364, 16).as_deref(),
            Some("A7")
        );
        assert_eq!(
            gpio_rom_function(&b, ChipType::Chip2364, 10).as_deref(),
            Some("CS1")
        );
        // A GPIO that is not on the socket at all.
        assert_eq!(gpio_rom_function(&b, ChipType::Chip2364, 29), None);
    }

    #[test]
    fn gpio_rom_function_follows_the_socket_pin_offset() {
        // A 24-pin 2364 on a 28-pin board sits at socket pins 3-26, so the
        // chip's pin 1 (A7) is two pins along from the board's.
        let b = Board::try_from_str("fire-28-c").unwrap();
        let socket_pin_3_gpio = b.gpios_for_socket_pin(3)[0];
        assert_eq!(
            gpio_rom_function(&b, ChipType::Chip2364, socket_pin_3_gpio).as_deref(),
            Some("A7")
        );
        // Socket pin 1 is outside the 24-pin chip - One ROM overhangs it.
        let socket_pin_1_gpio = b.gpios_for_socket_pin(1)[0];
        assert_eq!(
            gpio_rom_function(&b, ChipType::Chip2364, socket_pin_1_gpio),
            None
        );
    }

    #[test]
    fn gpio_system_functions_name_the_firmwares_system_pins() {
        let b = board();
        // fire-24-f puts the status LED and the NeoPixel on the same GPIO, so
        // both must be named - reporting only the first hides half of what
        // driving GPIO 29 disturbs.
        assert_eq!(b.pin_status(), 29);
        assert_eq!(b.pin_neo(), Some(29));
        assert_eq!(gpio_system_functions(&b, 29), ["Status LED", "RGB LED"]);
        assert!(gpio_system_functions(&b, 0).is_empty());

        // A board with an external flash and a NeoPixel on distinct GPIOs.
        let b32 = Board::try_from_str("fire-32-b").unwrap();
        assert_eq!(gpio_system_functions(&b32, 44), ["RGB LED"]);
        assert_eq!(gpio_system_functions(&b32, 47), ["ext flash CS"]);
        assert_eq!(gpio_system_functions(&b32, 45), ["Status LED"]);

        // Ice boards report 255 for "no status LED"; 255 is not a GPIO.
        let ice = Board::try_from_str("ice-24-d").unwrap();
        assert_eq!(ice.pin_status(), 255);
        assert!(gpio_system_functions(&ice, 255).is_empty());
    }

    /// Naming every GPIO of every board, under every chip type that board
    /// accepts, must not panic. `inspect gpio` walks the whole device, so a
    /// board/chip combination no hand-written test covers still gets asked.
    #[test]
    fn every_gpio_of_every_board_can_be_named() {
        use onerom_config::hw::BOARDS;
        for board in BOARDS {
            let board = &board;
            for gpio in 0u8..48 {
                let _ = gpio_header_role(board, gpio);
                let _ = gpio_system_functions(board, gpio);
                for &chip in CHIP_TYPES {
                    let _ = gpio_rom_function(board, chip, gpio);
                }
            }
        }
    }

    /// Exhaustive smoke test: rendering every board's header and every
    /// board×chip socket (in all modes) must not panic and must produce
    /// non-empty output naming the board. Guards against a metadata change (new
    /// board, populated `jumper_header`, new chip type) breaking a combination
    /// no hand-written test covers.
    #[test]
    fn all_boards_and_chips_render_without_panic() {
        use onerom_config::hw::BOARDS;
        for board in BOARDS {
            let board = &board;
            if let Some(h) = render_pin_header(board) {
                assert!(h.contains(board.description()), "header for {board:?}");
            }
            let gpio_map = render_rom_socket(board, None, false);
            assert!(
                gpio_map.contains(board.description()),
                "gpio map for {board:?}"
            );
            for &chip in CHIP_TYPES {
                for show_gpio in [false, true] {
                    let s = render_rom_socket(board, Some(chip), show_gpio);
                    assert!(s.contains(board.description()), "socket {board:?} {chip:?}");
                }
            }
        }
    }

    /// Every rendered socket is a structurally sound DIP: no notch, exactly N/2
    /// pin rows, and the body walls / border corners line up in the same column
    /// throughout. Runs across every board×chip so no combination can drift out
    /// of alignment unnoticed.
    #[test]
    fn socket_diagrams_are_structurally_aligned() {
        use onerom_config::hw::BOARDS;

        fn col(line: &str, ch: char) -> Option<usize> {
            line.chars().position(|c| c == ch)
        }
        fn check(s: &str, expected_rows: usize) {
            assert!(!s.contains('∪'), "socket must have no notch");
            let lines: Vec<&str> = s.lines().collect();
            let top = lines.iter().find(|l| l.contains('┌')).expect("top border");
            let bot = lines
                .iter()
                .find(|l| l.contains('└'))
                .expect("bottom border");
            let pin_rows: Vec<&&str> = lines
                .iter()
                .filter(|l| l.contains('┤') && l.contains('├'))
                .collect();
            assert_eq!(pin_rows.len(), expected_rows, "pin row count");
            let l_wall = col(top, '┌').unwrap();
            let r_wall = col(top, '┐').unwrap();
            assert_eq!(col(bot, '└'), Some(l_wall), "bottom-left corner");
            assert_eq!(col(bot, '┘'), Some(r_wall), "bottom-right corner");
            for row in pin_rows {
                assert_eq!(col(row, '┤'), Some(l_wall), "left wall: {row}");
                assert_eq!(col(row, '├'), Some(r_wall), "right wall: {row}");
            }
        }

        for board in BOARDS {
            let board = &board;
            let bp = board.chip_pins() as usize;
            check(&render_rom_socket(board, None, false), bp / 2);
            for &chip in CHIP_TYPES {
                let n = bp.max(chip.chip_pins() as usize);
                check(&render_rom_socket(board, Some(chip), true), n / 2);
            }
        }
    }
}
