//! Versioned electrical xdata on top-level INSERTs, wires and junctions.
//! Data is repeated per representation so another program's handle renumbering cannot
//! disconnect a device from its views. Supported payloads are consumed into the drawing
//! registry; foreign/future data stays raw.

use cadcraft_doc::electrical::{
    APP, Device, Envelope, Junction, MAX_DEVICES, MAX_PAYLOAD, MAX_TEXT, PinDef, Representation, Role, SCHEMA, assignable,
};
use cadcraft_doc::{Drawing, EntityKind, Handle, RawTag};
use cadcraft_dxf::Tag;

use crate::{IoError, Result};

pub(crate) fn validate(d: &Drawing) -> Result<()> {
    if d.electrical.devices.len() > MAX_DEVICES
        || d.electrical.representations.len() > MAX_DEVICES
        || d.electrical.symbols.len() > MAX_DEVICES
        || d.electrical.junctions.len() > MAX_DEVICES
        || d.electrical.wires.len() > MAX_DEVICES
    {
        return Err(IoError::Format("electrical data exceeds limits".into()));
    }
    for (h, rep) in &d.electrical.representations {
        if !assignable(d, *h) {
            continue;
        }
        let props = d.electrical.devices.get(&rep.device).ok_or_else(|| IoError::Format("electrical representation has no device".into()))?;
        Envelope { schema: SCHEMA, device: rep.device.clone(), properties: props.clone(), role: rep.role }.validate().map_err(IoError::Format)?;
    }
    for pins in d.electrical.symbols.values() {
        cadcraft_doc::electrical::validate_pins(pins).map_err(IoError::Format)?;
    }
    for junc in &d.electrical.junctions {
        junc.validate().map_err(IoError::Format)?;
    }
    Ok(())
}

pub(crate) fn tags(d: &Drawing, h: Handle) -> Vec<Tag> {
    if d.electrical.wires.contains(&h) {
        return vec![Tag::s(1001, APP), Tag::i(1070, i64::from(SCHEMA)), Tag::s(1000, "WIRE")];
    }
    if d.electrical.junctions.iter().any(|j| j.handle == Some(h)) {
        return vec![Tag::s(1001, APP), Tag::i(1070, i64::from(SCHEMA)), Tag::s(1000, "JUNCTION")];
    }
    let Some(rep) = d.electrical.representations.get(&h) else { return Vec::new() };
    let Some(props) = d.electrical.devices.get(&rep.device) else { return Vec::new() };
    let mut out = vec![Tag::s(1001, APP), Tag::i(1070, i64::from(SCHEMA)), Tag::s(1000, &rep.device), Tag::i(1070, role_code(rep.role))];
    for text in [&props.tag, &props.family, &props.manufacturer, &props.catalog, &props.description, &props.installation, &props.location] {
        out.push(Tag::s(1002, "{"));
        // Split at DXF's 255-byte string limit, respecting UTF-8 boundaries.
        let mut rest = text.as_str();
        while !rest.is_empty() {
            let chunk = crate::dxf_ext::xdata_str(rest);
            out.push(Tag::s(1000, chunk));
            rest = rest.get(chunk.len()..).unwrap_or_default();
        }
        out.push(Tag::s(1002, "}"));
    }
    if let Some(e) = d.entity(h)
        && let EntityKind::Insert(ref ins) = e.kind
        && let Some(pins) = d.electrical.symbols.get(&ins.block.to_ascii_uppercase())
        && !pins.is_empty()
    {
        out.push(Tag::s(1002, "{"));
        out.push(Tag::s(1000, "PINS"));
        for pin in pins {
            out.push(Tag::s(1002, "{"));
            out.push(Tag::s(1000, &pin.name));
            out.push(Tag::s(1000, pin.at.x.to_string()));
            out.push(Tag::s(1000, pin.at.y.to_string()));
            out.push(Tag::s(1002, "}"));
        }
        out.push(Tag::s(1002, "}"));
    }
    out
}

pub(crate) fn block_tags(d: &Drawing, block_name: &str) -> Vec<Tag> {
    let Some(pins) = d.electrical.symbols.get(&block_name.to_ascii_uppercase()) else { return Vec::new() };
    if pins.is_empty() {
        return Vec::new();
    }
    let mut out = vec![Tag::s(1001, APP), Tag::i(1070, i64::from(SCHEMA)), Tag::s(1000, "PINS"), Tag::s(1002, "{")];
    for pin in pins {
        out.push(Tag::s(1002, "{"));
        out.push(Tag::s(1000, &pin.name));
        out.push(Tag::s(1000, pin.at.x.to_string()));
        out.push(Tag::s(1000, pin.at.y.to_string()));
        out.push(Tag::s(1002, "}"));
    }
    out.push(Tag::s(1002, "}"));
    out
}

fn role_code(role: Role) -> i64 {
    match role {
        Role::Component => 0,
        Role::Coil => 1,
        Role::Contact => 2,
        Role::Footprint => 3,
    }
}

struct DecodedRep {
    envelope: Envelope,
    pins: Vec<PinDef>,
}

fn decode(tags: &[RawTag]) -> std::result::Result<DecodedRep, String> {
    if tags.len() > 300 || tags.iter().fold(0usize, |n, t| n.saturating_add(t.value.len())) > MAX_PAYLOAD {
        return Err("electrical payload exceeds limits".into());
    }
    let mut iter = tags.iter();
    let schema = iter.next().filter(|t| t.code == 1070).and_then(|t| t.value.parse::<u32>().ok()).ok_or("invalid electrical schema")?;
    let device = iter.next().filter(|t| t.code == 1000).ok_or("missing device identity")?.value.clone();
    let role = match iter.next().filter(|t| t.code == 1070).map(|t| t.value.trim()) {
        Some("0") => Role::Component,
        Some("1") => Role::Coil,
        Some("2") => Role::Contact,
        Some("3") => Role::Footprint,
        _ => return Err("invalid electrical representation role".into()),
    };
    let mut props = Device::default();
    for out in [
        &mut props.tag,
        &mut props.family,
        &mut props.manufacturer,
        &mut props.catalog,
        &mut props.description,
        &mut props.installation,
        &mut props.location,
    ] {
        if !iter.next().is_some_and(|t| t.code == 1002 && t.value == "{") {
            return Err("invalid electrical field start".into());
        }
        loop {
            let t = iter.next().ok_or("unterminated electrical field")?;
            if t.code == 1002 && t.value == "}" {
                break;
            }
            if t.code != 1000 || t.value.len() > 255 || out.len().saturating_add(t.value.len()) > MAX_TEXT {
                return Err("invalid or oversized electrical field".into());
            }
            out.push_str(&t.value);
        }
    }
    let mut pins = Vec::new();
    if let Some(t) = iter.next() {
        if t.code == 1002 && t.value == "{" {
            if !iter.next().is_some_and(|t| t.code == 1000 && t.value == "PINS") {
                return Err("unexpected electrical section".into());
            }
            loop {
                let Some(t) = iter.next() else { return Err("unterminated pins section".into()) };
                if t.code == 1002 && t.value == "}" {
                    break;
                }
                if t.code == 1002 && t.value == "{" {
                    let name = iter.next().filter(|t| t.code == 1000).ok_or("missing pin name")?.value.clone();
                    let x_str = iter.next().filter(|t| t.code == 1000 || t.code == 1040).ok_or("missing pin x")?.value.clone();
                    let y_str = iter.next().filter(|t| t.code == 1000 || t.code == 1040).ok_or("missing pin y")?.value.clone();
                    let close = iter.next().filter(|t| t.code == 1002 && t.value == "}").ok_or("unterminated pin definition")?;
                    let _ = close;
                    let x: f64 = x_str.trim().parse().map_err(|_| "invalid pin x coordinate")?;
                    let y: f64 = y_str.trim().parse().map_err(|_| "invalid pin y coordinate")?;
                    let pin = PinDef::new(name, cadcraft_geom::Vec2::new(x, y));
                    pin.validate()?;
                    pins.push(pin);
                } else {
                    return Err("unexpected pin group".into());
                }
            }
            if iter.next().is_some() {
                return Err("unexpected electrical data".into());
            }
        } else {
            return Err("unexpected electrical data".into());
        }
    }
    let envelope = Envelope { schema, device, properties: props, role };
    envelope.validate()?;
    Ok(DecodedRep { envelope, pins })
}

pub(crate) fn read(d: &mut Drawing) -> Result<()> {
    // Only entities with our application consume memory in addition to the existing store.
    let handles: Vec<Handle> = d
        .model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .filter(|e| e.common.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(APP)))
        .take(MAX_DEVICES + 1)
        .map(|e| e.handle)
        .collect();
    if handles.len() > MAX_DEVICES {
        return Err(IoError::Format("electrical representation limit exceeded".into()));
    }
    let mut scopes = std::collections::BTreeMap::new();
    let mut primaries = std::collections::BTreeSet::new();
    for h in handles {
        let Some(e) = d.entity(h) else { continue };
        let raw = &e.common.xdata;
        let Some(start) = raw.iter().position(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(APP)) else { continue };
        let end = raw.iter().enumerate().skip(start + 1).find(|(_, t)| t.code == 1001).map(|(i, _)| i).unwrap_or(raw.len());
        let duplicate = raw.iter().skip(end).any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(APP));

        let payload_tags = raw.get(start + 1..end).unwrap_or_default();
        let tag1000 = payload_tags.iter().find(|t| t.code == 1000).map(|t| t.value.as_str());

        if tag1000 == Some("WIRE") {
            let data = std::sync::Arc::make_mut(&mut d.electrical);
            data.wires.insert(h);
            d.modify_entity(h, |e| {
                e.common.xdata.drain(start..end);
            })
            .map_err(|e| IoError::Format(e.to_string()))?;
            continue;
        }

        if tag1000 == Some("JUNCTION") {
            let pt = match &e.kind {
                EntityKind::Point(p) => Some(p.p.xy()),
                EntityKind::Circle(c) => Some(c.center.xy()),
                _ => None,
            };
            if let Some(pos) = pt {
                let data = std::sync::Arc::make_mut(&mut d.electrical);
                data.junctions.push(Junction { at: pos, handle: Some(h) });
            }
            d.modify_entity(h, |e| {
                e.common.xdata.drain(start..end);
            })
            .map_err(|e| IoError::Format(e.to_string()))?;
            continue;
        }

        let parsed =
            if duplicate || !assignable(d, h) { Err("unsupported or duplicate electrical representation".into()) } else { decode(payload_tags) };
        let decoded = match parsed {
            Ok(rep) => rep,
            Err(why) => {
                // Keep the uninterpreted application intact for a later compatible reader.
                d.read_notes.push(format!("Electrical metadata on {} kept uninterpreted: {why}", h.hex()));
                continue;
            }
        };
        let env = decoded.envelope;
        if let Some(old) = d.electrical.devices.get(&env.device)
            && old != &env.properties
        {
            return Err(IoError::Format(format!("conflicting electrical properties for device {}", env.device)));
        }
        let key =
            (env.properties.installation.to_ascii_uppercase(), env.properties.location.to_ascii_uppercase(), env.properties.tag.to_ascii_uppercase());
        if scopes.get(&key).is_some_and(|other| other != &env.device) {
            return Err(IoError::Format("duplicate electrical tag in installation/location".into()));
        }
        scopes.insert(key, env.device.clone());
        if env.role.is_primary() && !primaries.insert(env.device.clone()) {
            return Err(IoError::Format("electrical device has duplicate primary representations".into()));
        }
        let block_name = if let Some(e) = d.entity(h)
            && let EntityKind::Insert(ref ins) = e.kind
        {
            Some(ins.block.to_ascii_uppercase())
        } else {
            None
        };
        let data = std::sync::Arc::make_mut(&mut d.electrical);
        data.devices.insert(env.device.clone(), env.properties);
        data.representations.insert(h, Representation { device: env.device, role: env.role });
        if !decoded.pins.is_empty()
            && let Some(name) = block_name
        {
            data.symbols.entry(name).or_insert(decoded.pins);
        }
        d.modify_entity(h, |e| {
            e.common.xdata.drain(start..end);
        })
        .map_err(|e| IoError::Format(e.to_string()))?;
    }
    Ok(())
}
