// Copyright (c) 2026 Piers Finlayson <piers@piers.rocks>
//
// MIT License

//! What the device says it is, which is all a host has to go on.
//!
//! A host reads these bytes once, before anything else works, and everything
//! that follows depends on them: which driver binds, whether Windows finds the
//! device without Zadig, whether picotool recognises it.  Nothing else in this
//! repository checks them.
//!
//! The bugs worth catching here are two files disagreeing.  A descriptor that
//! is wrong on its own is usually wrong loudly — a device that will not
//! enumerate at all.  A descriptor that contradicts another file enumerates
//! fine and fails somewhere else entirely, so most scenarios below assert one
//! file against another rather than a field against a constant.

use crate::device::Device;
use crate::{Ctx, Scenario};
use onerom_plugin_tester::run::Outcome;

// Control transfer stages, from tinyusb's tusb_types.h.
const STAGE_SETUP: u8 = 1;
const STAGE_DATA: u8 = 2;
const STAGE_ACK: u8 = 3;

// bmRequestType: device-to-host, addressed to the device, of the given type.
const REQ_TYPE_STANDARD: u8 = 0x80;
const REQ_TYPE_VENDOR: u8 = 0xC0;

// Descriptor types, from tusb_types.h.
const DESC_DEVICE: u8 = 0x01;
const DESC_CONFIGURATION: u8 = 0x02;
const DESC_STRING: u8 = 0x03;
const DESC_INTERFACE: u8 = 0x04;
const DESC_ENDPOINT: u8 = 0x05;

// From usb_descriptors.h.
const VENDOR_REQUEST_MICROSOFT: u8 = 1;
const MS_OS_20_DESC_LEN: usize = 0xB2;

/// The wIndex Windows asks the MS OS 2.0 descriptor with, from usb_main.c.
const MS_OS_20_WINDEX: u16 = 7;

/// What Studio matches on to recognise a running One ROM — see `FIRE_VID` and
/// `FIRE_RUN_PID` in `rust/studio/src/device/usb.rs`.
const FIRE_VID: u16 = 0x1209;
const FIRE_RUN_PID: u16 = 0xF542;

/// The interface picoboot must be given, because picotool will not look
/// anywhere else when a device has more than one.
const PICOBOOT_INTERFACE: u8 = 1;

/// The chip ID the shim reports, which is what the serial is derived from.
const CHIP_ID: &str = "0123456789ABCDEF";

// ---------------------------------------------------------------------------
// Reading descriptors
// ---------------------------------------------------------------------------

/// One descriptor inside a block: its type and its bytes, header included.
struct Item<'a> {
    kind: u8,
    bytes: &'a [u8],
}

/// Split a block into the descriptors it is made of.
///
/// Each states its own length, so this is also the check that they tile the
/// block exactly: a block whose last descriptor runs past the end, or which has
/// bytes left over, is one no host can read.
fn split(block: &[u8]) -> Result<Vec<Item<'_>>, String> {
    let mut items = Vec::new();
    let mut at = 0usize;

    while at < block.len() {
        if block.len() - at < 2 {
            return Err(format!(
                "{} bytes left over at offset {at}, too few for a descriptor header",
                block.len() - at
            ));
        }
        let len = usize::from(block[at]);
        if len < 2 {
            return Err(format!("a descriptor at offset {at} declares {len} bytes"));
        }
        if at + len > block.len() {
            return Err(format!(
                "a descriptor at offset {at} declares {len} bytes, running {} past the end",
                at + len - block.len()
            ));
        }
        items.push(Item {
            kind: block[at + 1],
            bytes: &block[at..at + len],
        });
        at += len;
    }

    Ok(items)
}

/// A little-endian u16 at `at`.
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

/// The serial the device's metadata overrides its chip ID with, if any.
///
/// Read from the firmware rather than from the config file, because that is
/// where the plugin reads it: a scenario taking it from the config would agree
/// with itself while the device disagreed with both.
fn serial_override(dev: &Device) -> Result<Option<String>, String> {
    match dev.emulator().get_metadata_str(
        onerom_fw_emulator::ffi::ora_metadata_key_t_ORA_METADATA_KEY_SERIAL_OVERRIDE,
    ) {
        (onerom_fw_emulator::OraResult::Ok, over) => Ok(over.filter(|s| !s.is_empty())),
        (r, _) => Err(format!("could not read the serial override: {r:?}")),
    }
}

/// The serial the device presents, as text.
fn presented_serial(dev: &Device) -> Result<String, String> {
    let d = dev.device_descriptor()?;
    let index = d[16];
    if index == 0 {
        return Err("the device advertises no serial number string".to_string());
    }

    let units = dev
        .string_descriptor(index)?
        .ok_or(format!("the device refuses string {index}, its own serial"))?;

    if units[0] >> 8 != u16::from(DESC_STRING) {
        return Err(format!(
            "string {index} says it is descriptor type {:#04x}, not a string",
            units[0] >> 8
        ));
    }
    if usize::from(units[0] & 0xff) != units.len() * 2 {
        return Err(format!(
            "string {index} declares {} bytes and carries {}",
            units[0] & 0xff,
            units.len() * 2
        ));
    }

    Ok(units[1..]
        .iter()
        .map(|&u| char::from_u32(u32::from(u)).unwrap_or('?'))
        .collect())
}

/// The MS OS 2.0 descriptor, fetched the way Windows fetches it.
fn ms_os_descriptor(dev: &mut Device) -> Result<Vec<u8>, String> {
    if !dev.vendor_control(
        STAGE_SETUP,
        REQ_TYPE_VENDOR,
        VENDOR_REQUEST_MICROSOFT,
        MS_OS_20_WINDEX,
    ) {
        return Err("the device would not answer the Microsoft descriptor request".to_string());
    }
    dev.take_control_xfer()
}

// ---------------------------------------------------------------------------

/// The device identifies itself as the One ROM its own tools look for.
///
/// Studio finds a running device by vendor and product id and nothing else, so
/// changing either here silently stops it recognising One ROM at all.  The
/// literals are written out rather than taken from the header for exactly that
/// reason — the point is to make the change deliberate.
fn the_device_is_the_one_studio_looks_for(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let d = dev.device_descriptor()?;

    if d.len() != 18 || d[1] != DESC_DEVICE {
        return Err(format!(
            "the device descriptor is {} bytes of type {:#04x}, not 18 of type {DESC_DEVICE:#04x}",
            d.len(),
            d[1]
        ));
    }

    let vid = u16_at(&d, 8);
    let pid = u16_at(&d, 10);
    if vid != FIRE_VID || pid != FIRE_RUN_PID {
        return Err(format!(
            "the device presents {vid:04x}:{pid:04x}, not the {FIRE_VID:04x}:{FIRE_RUN_PID:04x} \
             Studio looks for"
        ));
    }

    if d[17] != 1 {
        return Err(format!(
            "the device offers {} configurations, and everything here assumes one",
            d[17]
        ));
    }

    Ok(Outcome::Pass)
}

/// The device claims USB 2.1, which is what makes a host ask for the BOS.
///
/// Below 2.1 a host never fetches the BOS descriptor, so the Microsoft
/// descriptor behind it is never asked for either — the device still
/// enumerates, and Windows silently needs Zadig again.
fn the_device_asks_for_usb_2_1(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let d = dev.device_descriptor()?;
    let bcd_usb = u16_at(&d, 2);
    if bcd_usb < 0x0210 {
        return Err(format!(
            "the device claims USB {bcd_usb:04x}, and a host asks for the BOS only from 0210"
        ));
    }

    Ok(Outcome::Pass)
}

/// The configuration descriptor is as long as it says it is.
///
/// A host reads `wTotalLength` bytes and no more.  Add an interface and forget
/// the total, and the host silently never sees the last descriptor — so this
/// compares the declared length against the array's real size, which is the one
/// thing the descriptor cannot be asked about.
fn the_configuration_declares_the_length_it_sends(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let real = dev.configuration_desc_size() as usize;
    let cfg = dev.configuration_descriptor(0)?;

    if cfg[1] != DESC_CONFIGURATION {
        return Err(format!(
            "the configuration descriptor says it is type {:#04x}",
            cfg[1]
        ));
    }
    if cfg.len() != real {
        return Err(format!(
            "the configuration descriptor declares {} bytes but occupies {real}",
            cfg.len()
        ));
    }

    // And the descriptors inside it tile that length exactly.
    let items = split(&cfg)?;

    let declared = usize::from(cfg[4]);
    let found = items.iter().filter(|i| i.kind == DESC_INTERFACE).count();
    if found != declared {
        return Err(format!(
            "the configuration says it has {declared} interfaces and carries {found}"
        ));
    }

    Ok(Outcome::Pass)
}

/// picoboot is interface 1, behind a dummy interface 0.
///
/// picotool will not recognise a device with more than one interface unless
/// picoboot is the second, and the first is vendor-specific with no subclass or
/// protocol.  Nothing enforces it but this.
fn picoboot_is_the_second_interface(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let cfg = dev.configuration_descriptor(0)?;
    let items = split(&cfg)?;

    let interfaces: Vec<&[u8]> = items
        .iter()
        .filter(|i| i.kind == DESC_INTERFACE)
        .map(|i| i.bytes)
        .collect();

    let dummy = interfaces
        .iter()
        .find(|i| i[2] == 0)
        .ok_or("the configuration has no interface 0")?;
    if dummy[5] != 0xFF || dummy[6] != 0x00 || dummy[7] != 0x00 {
        return Err(format!(
            "interface 0 is class {:#04x}/{:#04x}/{:#04x}, and picotool needs ff/00/00",
            dummy[5], dummy[6], dummy[7]
        ));
    }

    let picoboot = interfaces
        .iter()
        .find(|i| i[2] == PICOBOOT_INTERFACE)
        .ok_or(format!(
            "the configuration has no interface {PICOBOOT_INTERFACE}, which is where \
             picotool looks for picoboot"
        ))?;
    if picoboot[5] != 0xFF {
        return Err(format!(
            "interface {PICOBOOT_INTERFACE} is class {:#04x}, not the vendor-specific ff \
             picoboot needs",
            picoboot[5]
        ));
    }
    if picoboot[4] != 2 {
        return Err(format!(
            "interface {PICOBOOT_INTERFACE} has {} endpoints, and picoboot needs its in and out",
            picoboot[4]
        ));
    }

    Ok(Outcome::Pass)
}

/// Every interface names a string the device can produce.
///
/// The index and the table are in the same file but nothing ties them, so an
/// interface can name a string that was never added.  A host asking for it gets
/// a stall in the middle of enumeration.
fn every_interface_names_a_string_that_exists(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let cfg = dev.configuration_descriptor(0)?;
    let items = split(&cfg)?;

    let named: Vec<u8> = items
        .iter()
        .filter(|i| i.kind == DESC_INTERFACE)
        .map(|i| i.bytes[8])
        .filter(|&index| index != 0)
        .collect();

    if named.is_empty() {
        return Err("no interface names a string, so this scenario proves nothing".to_string());
    }

    for index in named {
        match dev.string_descriptor(index)? {
            None => {
                return Err(format!(
                    "an interface names string {index}, which the device refuses to produce"
                ));
            }
            Some(units) if units.len() < 2 => {
                return Err(format!("string {index} is empty"));
            }
            Some(_) => {}
        }
    }

    Ok(Outcome::Pass)
}

/// No endpoint address is used twice.
///
/// Two interfaces given the same address is a device that enumerates and then
/// mixes two streams together on one endpoint.
fn no_endpoint_is_used_twice(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let cfg = dev.configuration_descriptor(0)?;
    let items = split(&cfg)?;

    let mut seen: Vec<u8> = Vec::new();
    for ep in items.iter().filter(|i| i.kind == DESC_ENDPOINT) {
        let addr = ep.bytes[2];
        if seen.contains(&addr) {
            return Err(format!("endpoint {addr:#04x} is claimed twice"));
        }
        seen.push(addr);
    }

    if seen.len() < 4 {
        return Err(format!(
            "the configuration declares {} endpoints, too few for picoboot and the serial port",
            seen.len()
        ));
    }

    Ok(Outcome::Pass)
}

/// The BOS descriptor points Windows at the vendor request the device answers.
///
/// The request code and the descriptor length live in the BOS, and the code
/// that answers is in another file.  If they drift, Windows asks with a code the
/// device ignores and falls back to needing a driver installed by hand.
fn the_bos_points_at_a_request_the_device_answers(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let real = dev.bos_desc_size() as usize;
    let bos = dev.bos_descriptor()?;

    if bos.len() != real {
        return Err(format!(
            "the BOS descriptor declares {} bytes but occupies {real}",
            bos.len()
        ));
    }
    if bos[4] != 1 {
        return Err(format!(
            "the BOS declares {} capabilities, and only the Microsoft one is expected",
            bos[4]
        ));
    }

    // The platform capability follows the 5 byte BOS header.  Its descriptor
    // set length and vendor request code are the last four bytes of it.
    let cap = &bos[5..];
    let set_len = usize::from(u16_at(cap, cap.len() - 4));
    let vendor_code = cap[cap.len() - 2];

    if vendor_code != VENDOR_REQUEST_MICROSOFT {
        return Err(format!(
            "the BOS tells Windows to ask with request {vendor_code}, and the device answers \
             {VENDOR_REQUEST_MICROSOFT}"
        ));
    }

    let ms_os = ms_os_descriptor(dev)?;
    if set_len != ms_os.len() {
        return Err(format!(
            "the BOS promises {set_len} bytes of Microsoft descriptor and the device sends {}",
            ms_os.len()
        ));
    }

    Ok(Outcome::Pass)
}

/// The Microsoft descriptor binds WinUSB to the interface picoboot is on.
///
/// The interface number appears in two files, and the one Windows acts on is
/// this one.  Point it at the wrong interface and WinUSB binds to the serial
/// port instead, which no host tool can then reach.
fn the_windows_descriptor_names_the_vendor_interface(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let cfg = dev.configuration_descriptor(0)?;
    let items = split(&cfg)?;
    let vendor = items
        .iter()
        .filter(|i| i.kind == DESC_INTERFACE)
        .find(|i| i.bytes[2] == PICOBOOT_INTERFACE)
        .ok_or(format!(
            "no interface {PICOBOOT_INTERFACE} to compare against"
        ))?
        .bytes[2];

    let ms_os = ms_os_descriptor(dev)?;

    // Set header (10 bytes), configuration subset header (8), then the function
    // subset header, whose third byte is the first interface it covers.
    let first_interface = ms_os[10 + 8 + 4];
    if first_interface != vendor {
        return Err(format!(
            "the Microsoft descriptor binds WinUSB to interface {first_interface}, and picoboot \
             is on {vendor}"
        ));
    }

    Ok(Outcome::Pass)
}

/// The Microsoft descriptor's nested lengths span it exactly.
///
/// It is a set of nested subsets, each declaring its own length by hand.  A
/// compile-time check pins the total, and nothing pins the parts.
fn the_windows_descriptor_subsets_span_it(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    let ms_os = ms_os_descriptor(dev)?;

    if ms_os.len() != MS_OS_20_DESC_LEN {
        return Err(format!(
            "the Microsoft descriptor is {} bytes, not the {MS_OS_20_DESC_LEN} declared",
            ms_os.len()
        ));
    }

    let set_total = usize::from(u16_at(&ms_os, 8));
    if set_total != ms_os.len() {
        return Err(format!(
            "the set header says the whole thing is {set_total} bytes, and it is {}",
            ms_os.len()
        ));
    }

    // The configuration subset covers everything after the 10 byte set header,
    // and the function subset everything after its own 8 byte header.
    let config_subset = usize::from(u16_at(&ms_os, 10 + 6));
    if config_subset != ms_os.len() - 10 {
        return Err(format!(
            "the configuration subset claims {config_subset} bytes of the {} that follow the \
             set header",
            ms_os.len() - 10
        ));
    }

    let function_subset = usize::from(u16_at(&ms_os, 10 + 8 + 6));
    if function_subset != ms_os.len() - 10 - 8 {
        return Err(format!(
            "the function subset claims {function_subset} bytes of the {} that follow it",
            ms_os.len() - 10 - 8
        ));
    }

    // The compatible ID that makes Windows load WinUSB at all.
    let compat = &ms_os[10 + 8 + 8 + 4..10 + 8 + 8 + 4 + 6];
    if compat != b"WINUSB" {
        return Err(format!(
            "the compatible ID is {:?}, not WINUSB",
            String::from_utf8_lossy(compat)
        ));
    }

    Ok(Outcome::Pass)
}

/// The serial string the device advertises is the one built from the chip ID.
///
/// The device descriptor names which string index carries the serial, and the
/// string callback has a case for it.  A device naming an index its callback
/// does not treat as the serial answers a host with the wrong string, and every
/// tool that selects on serial then sees every device as the same one.
fn the_advertised_serial_index_carries_the_chip_id(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    if let Some(over) = serial_override(dev)? {
        return Ok(Outcome::Skip(format!(
            "this device's metadata overrides its serial with {over:?}"
        )));
    }

    let serial = presented_serial(dev)?;
    if serial != CHIP_ID {
        return Err(format!(
            "the device serial is {serial:?}, not the {CHIP_ID:?} its chip ID makes"
        ));
    }

    Ok(Outcome::Pass)
}

/// A serial override in the device's metadata is what the device presents.
///
/// The override is why the field exists: two One ROMs in one machine are told
/// apart by serial, and the chip ID is not something a user can choose.  A
/// device that read the override and then sent the chip ID anyway would look
/// exactly like a device with no override set.
fn the_serial_override_replaces_the_chip_id(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let Some(over) = serial_override(dev)? else {
        return Ok(Outcome::Skip(
            "this device's metadata sets no serial override".to_string(),
        ));
    };

    let serial = presented_serial(dev)?;
    if serial == CHIP_ID {
        return Err(format!(
            "the device presents its chip ID, ignoring the {over:?} its metadata sets"
        ));
    }
    if serial != over {
        return Err(format!(
            "the device presents {serial:?}, and its metadata says {over:?}"
        ));
    }

    Ok(Outcome::Pass)
}

/// A string index the device does not have is refused.
///
/// The table is walked by index, so a device that answered anyway would be
/// reading past the end of it — and would hand a host whatever followed.
fn an_unknown_string_index_is_refused(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    // Far beyond any table, and the 0xEE Microsoft OS 1.0 index, which this
    // device does not implement either.
    for index in [200u8, 0xEE] {
        if let Some(units) = dev.string_descriptor(index)? {
            return Err(format!(
                "string {index} does not exist, and the device answered with {} code units",
                units.len()
            ));
        }
    }

    // And the language table, which does, so the refusal above is not simply a
    // device that refuses everything.
    let langs = dev
        .string_descriptor(0)?
        .ok_or("the device refuses string 0, which is its language list")?;
    if langs.len() != 2 || langs[1] != 0x0409 {
        return Err(format!(
            "the device offers languages {:04x?}, not the single English 0409",
            &langs[1..]
        ));
    }

    Ok(Outcome::Pass)
}

/// Windows asks for the Microsoft descriptor and gets it.
fn windows_asks_for_the_descriptor_and_gets_it(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    let claimed = dev.vendor_control(
        STAGE_SETUP,
        REQ_TYPE_VENDOR,
        VENDOR_REQUEST_MICROSOFT,
        MS_OS_20_WINDEX,
    );
    if !claimed {
        return Err("the device stalled the request Windows enumerates it with".to_string());
    }
    if dev.control_xfer_count() != 1 {
        return Err(format!(
            "the device claimed the request and offered {} replies",
            dev.control_xfer_count()
        ));
    }

    let sent = dev.take_control_xfer()?;
    if sent.len() != MS_OS_20_DESC_LEN {
        return Err(format!(
            "the device sent {} bytes, not the {MS_OS_20_DESC_LEN} it declares",
            sent.len()
        ));
    }

    // The data and acknowledgement stages of the same transfer are the
    // device's to complete, and stalling either loses the descriptor it has
    // already begun sending.
    for stage in [STAGE_DATA, STAGE_ACK] {
        if !dev.vendor_control(
            stage,
            REQ_TYPE_VENDOR,
            VENDOR_REQUEST_MICROSOFT,
            MS_OS_20_WINDEX,
        ) {
            return Err(format!(
                "the device abandoned the transfer at stage {stage}"
            ));
        }
    }

    Ok(Outcome::Pass)
}

/// A vendor request the device does not implement is left alone.
///
/// Claiming a request it cannot answer is worse than stalling it: the host
/// waits for a reply that never comes, rather than moving on.
fn an_unknown_vendor_request_is_not_claimed(
    dev: &mut Device,
    _ctx: &Ctx,
) -> Result<Outcome, String> {
    // The right request code, the wrong index.  usb_main.c answers only 7.
    if dev.vendor_control(
        STAGE_SETUP,
        REQ_TYPE_VENDOR,
        VENDOR_REQUEST_MICROSOFT,
        MS_OS_20_WINDEX + 1,
    ) {
        return Err(
            "the device claimed a Microsoft request for an index it does not serve".to_string(),
        );
    }

    // The right index, but a standard request rather than a vendor one, which
    // tinyusb handles itself.
    if dev.vendor_control(
        STAGE_SETUP,
        REQ_TYPE_STANDARD,
        VENDOR_REQUEST_MICROSOFT,
        MS_OS_20_WINDEX,
    ) {
        return Err("the device claimed a standard request as if it were a vendor one".to_string());
    }

    if dev.control_xfer_count() != 0 {
        return Err(format!(
            "the device sent {} replies to requests it declined",
            dev.control_xfer_count()
        ));
    }

    Ok(Outcome::Pass)
}

/// picoboot sees a control request before the plugin's own handler.
///
/// Both want vendor requests on the same interface, so the order is what keeps
/// them apart.  A plugin that looked first would answer a request picoboot had
/// already taken.
fn picoboot_sees_a_control_request_first(dev: &mut Device, _ctx: &Ctx) -> Result<Outcome, String> {
    dev.set_picoboot_claims_control(true);

    let claimed = dev.vendor_control(
        STAGE_SETUP,
        REQ_TYPE_VENDOR,
        VENDOR_REQUEST_MICROSOFT,
        MS_OS_20_WINDEX,
    );
    if !claimed {
        return Err("the request picoboot claimed was reported as unhandled".to_string());
    }
    if dev.control_xfer_count() != 0 {
        return Err(
            "the plugin answered a request picoboot had already claimed, so both replied"
                .to_string(),
        );
    }

    // With picoboot declining again, the same request is the plugin's.
    dev.set_picoboot_claims_control(false);
    if !dev.vendor_control(
        STAGE_SETUP,
        REQ_TYPE_VENDOR,
        VENDOR_REQUEST_MICROSOFT,
        MS_OS_20_WINDEX,
    ) {
        return Err("the device would not answer once picoboot declined".to_string());
    }
    if dev.control_xfer_count() != 1 {
        return Err("the plugin claimed the request without answering it".to_string());
    }

    Ok(Outcome::Pass)
}

pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "descriptors.the_device_is_the_one_studio_looks_for",
        about: "the device presents the vendor and product id Studio matches on",
        run: the_device_is_the_one_studio_looks_for,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_device_asks_for_usb_2_1",
        about: "the device claims the USB version that makes a host fetch the BOS",
        run: the_device_asks_for_usb_2_1,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_configuration_declares_the_length_it_sends",
        about: "wTotalLength matches the bytes there are, and the interface count matches too",
        run: the_configuration_declares_the_length_it_sends,
        before_start: None,
    },
    Scenario {
        name: "descriptors.picoboot_is_the_second_interface",
        about: "a vendor-specific interface 0 puts picoboot on interface 1, where picotool looks",
        run: picoboot_is_the_second_interface,
        before_start: None,
    },
    Scenario {
        name: "descriptors.every_interface_names_a_string_that_exists",
        about: "the string index each interface names can be produced",
        run: every_interface_names_a_string_that_exists,
        before_start: None,
    },
    Scenario {
        name: "descriptors.no_endpoint_is_used_twice",
        about: "no two interfaces claim the same endpoint address",
        run: no_endpoint_is_used_twice,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_bos_points_at_a_request_the_device_answers",
        about: "the BOS names the vendor request code and length the device actually serves",
        run: the_bos_points_at_a_request_the_device_answers,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_windows_descriptor_names_the_vendor_interface",
        about: "WinUSB is bound to the interface picoboot is on",
        run: the_windows_descriptor_names_the_vendor_interface,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_windows_descriptor_subsets_span_it",
        about: "the nested lengths in the Microsoft descriptor add up to the whole",
        run: the_windows_descriptor_subsets_span_it,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_advertised_serial_index_carries_the_chip_id",
        about: "the string index the device names as its serial is the chip ID",
        run: the_advertised_serial_index_carries_the_chip_id,
        before_start: None,
    },
    Scenario {
        name: "descriptors.the_serial_override_replaces_the_chip_id",
        about: "a serial override in the device's metadata is the serial it presents",
        run: the_serial_override_replaces_the_chip_id,
        before_start: None,
    },
    Scenario {
        name: "descriptors.an_unknown_string_index_is_refused",
        about: "an index past the table is refused, while the language list is not",
        run: an_unknown_string_index_is_refused,
        before_start: None,
    },
    Scenario {
        name: "descriptors.windows_asks_for_the_descriptor_and_gets_it",
        about: "the Microsoft request is claimed and answered with the whole descriptor",
        run: windows_asks_for_the_descriptor_and_gets_it,
        before_start: None,
    },
    Scenario {
        name: "descriptors.an_unknown_vendor_request_is_not_claimed",
        about: "a wrong index or a standard request is declined, with nothing sent",
        run: an_unknown_vendor_request_is_not_claimed,
        before_start: None,
    },
    Scenario {
        name: "descriptors.picoboot_sees_a_control_request_first",
        about: "the plugin answers only what picoboot has declined",
        run: picoboot_sees_a_control_request_first,
        before_start: None,
    },
];
