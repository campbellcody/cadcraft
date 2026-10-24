//! Versioned electrical xdata on top-level INSERTs. Data is repeated per representation
//! so another program's handle renumbering cannot disconnect a device from its views.
//! Supported payloads are consumed into the drawing registry; foreign/future data stays raw.

use cadcraft_doc::electrical::{APP, Device, Envelope, MAX_DEVICES, MAX_PAYLOAD, MAX_TEXT, Representation, Role, SCHEMA, assignable};
use cadcraft_doc::{Drawing, Handle, RawTag};
use cadcraft_dxf::Tag;

use crate::{IoError, Result};

pub(crate) fn validate(d: &Drawing) -> Result<()> {
    if d.electrical.devices.len() > MAX_DEVICES || d.electrical.representations.len() > MAX_DEVICES {
        return Err(IoError::Format("electrical data exceeds limits".into()));
    }
    for (h, rep) in &d.electrical.representations {
        if !assignable(d, *h) {
            continue;
        }
        let props = d.electrical.devices.get(&rep.device).ok_or_else(|| IoError::Format("electrical representation has no device".into()))?;
        Envelope { schema: SCHEMA, device: rep.device.clone(), properties: props.clone(), role: rep.role }.validate().map_err(IoError::Format)?;
    }
    Ok(())
}

pub(crate) fn tags(d: &Drawing, h: Handle) -> Vec<Tag> {
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

fn decode(tags: &[RawTag]) -> std::result::Result<Envelope, String> {
    if tags.len() > 100 || tags.iter().fold(0usize, |n, t| n.saturating_add(t.value.len())) > MAX_PAYLOAD {
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
    if iter.next().is_some() {
        return Err("unexpected electrical data".into());
    }
    let envelope = Envelope { schema, device, properties: props, role };
    envelope.validate()?;
    Ok(envelope)
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
        let parsed = if duplicate || !assignable(d, h) {
            Err("unsupported or duplicate electrical representation".into())
        } else {
            decode(raw.get(start + 1..end).unwrap_or_default())
        };
        let env = match parsed {
            Ok(env) => env,
            Err(why) => {
                // Keep the uninterpreted application intact for a later compatible reader.
                d.read_notes.push(format!("Electrical metadata on {} kept uninterpreted: {why}", h.hex()));
                continue;
            }
        };
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
        let data = std::sync::Arc::make_mut(&mut d.electrical);
        data.devices.insert(env.device.clone(), env.properties);
        data.representations.insert(h, Representation { device: env.device, role: env.role });
        d.modify_entity(h, |e| {
            e.common.xdata.drain(start..end);
        })
        .map_err(|e| IoError::Format(e.to_string()))?;
    }
    Ok(())
}
