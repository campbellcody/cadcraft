//! Single-drawing electrical device associations and physical-device bills of materials.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::electrical::{APP, Device, MAX_DEVICES, MAX_TEXT, Representation, Role, assignable};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use serde_json::{Value, json};

use super::{CommandSpec, bad};
use crate::{EngineError, Result, Session};

const FIELDS: &[&str] = &["tag", "family", "manufacturer", "catalog", "description", "installation", "location"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("electrical.component.assign", "Assign Electrical Component", assign)
            .params("{handle, device?: existing device id, role?: component|coil|contact|footprint, tag?, family?, manufacturer?, catalog?, description?, installation?, location?} → {device, handle, role}; a new device requires a tag; linking an existing device accepts no property fields"),
        CommandSpec::new("electrical.component.update", "Edit Electrical Device", update)
            .params("{device, tag?, family?, manufacturer?, catalog?, description?, installation?, location?} → {device, properties}; updates every representation's shared metadata"),
        CommandSpec::new("electrical.component.detach", "Detach Electrical Component", detach)
            .params("{handle} → {device, handle}; leaves geometry and attributes unchanged"),
        CommandSpec::new("electrical.components", "List Electrical Components", list)
            .params("{} → {devices: [{device, properties, representations: [{handle, role}]}], unassigned: [handle]}; top-level model and paper space INSERTs only")
            .noundo(),
        CommandSpec::new("electrical.bom", "Electrical Bill of Materials", bom)
            .params("{file?: CSV destination} → {columns, rows, devices, unassigned, missingCatalog, missingPrimary, file?}; counts distinct physical devices, not graphical representations")
            .noundo(),
    ]
}

fn object<'a>(cmd: &str, p: &'a Value, allowed: &[&str]) -> Result<&'a serde_json::Map<String, Value>> {
    let obj = p.as_object().ok_or_else(|| bad(cmd, "parameters must be an object"))?;
    if obj.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(bad(cmd, "unknown parameter"));
    }
    Ok(obj)
}

fn text<'a>(cmd: &str, p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
        .ok_or_else(|| bad(cmd, format!("`{key}` must be a non-empty string of at most 4096 bytes without control characters")))
}

fn handle(cmd: &str, p: &Value) -> Result<Handle> {
    Handle::parse_hex(text(cmd, p, "handle")?).ok_or_else(|| bad(cmd, "invalid hexadecimal handle"))
}

fn bounded(d: &Drawing, cmd: &str) -> Result<()> {
    if d.entity_count() > 100_000 || d.electrical.devices.len() > MAX_DEVICES || d.electrical.representations.len() > MAX_DEVICES {
        return Err(bad(cmd, format!("electrical commands support at most 100000 drawing entities and {MAX_DEVICES} devices/representations")));
    }
    Ok(())
}

fn writable(s: &Session, cmd: &str) -> Result<()> {
    bounded(s.doc()?, cmd)?;
    if s.state()?.block_edit.is_some() || s.doc()?.refedit.is_some() {
        return Err(bad(cmd, "finish block/reference editing before changing electrical associations"));
    }
    Ok(())
}

fn properties(cmd: &str, p: &Value, base: &Device) -> Result<Device> {
    let mut d = base.clone();
    for (key, out) in [
        ("tag", &mut d.tag),
        ("family", &mut d.family),
        ("manufacturer", &mut d.manufacturer),
        ("catalog", &mut d.catalog),
        ("description", &mut d.description),
        ("installation", &mut d.installation),
        ("location", &mut d.location),
    ] {
        if let Some(v) = p.get(key) {
            let text = v.as_str().ok_or_else(|| bad(cmd, format!("`{key}` must be a string")))?;
            if text.len() > MAX_TEXT || text.chars().any(char::is_control) {
                return Err(bad(cmd, "component field exceeds length limit or contains control characters"));
            }
            *out = text.trim().to_string();
        }
    }
    d.validate().map_err(|e| bad(cmd, e))?;
    Ok(d)
}

fn unique(d: &Drawing, cmd: &str, id: &str, device: &Device) -> Result<()> {
    let active = index(d);
    if d.electrical.devices.iter().any(|(other, props)| other != id && props.same_tag_scope(device) && active.contains_key(other.as_str())) {
        return Err(bad(cmd, "device tag already exists in this installation/location"));
    }
    Ok(())
}

fn live<'a>(d: &'a Drawing, id: &'a str) -> impl Iterator<Item = (Handle, Role)> + 'a {
    d.electrical.representations.iter().filter(move |(h, r)| r.device == id && assignable(d, **h)).map(|(h, r)| (*h, r.role))
}

fn assign(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.component.assign";
    object(cmd, p, &["handle", "device", "role", "tag", "family", "manufacturer", "catalog", "description", "installation", "location"])?;
    writable(s, cmd)?;
    let h = handle(cmd, p)?;
    let d = s.doc()?;
    if d.electrical.representations.len() >= MAX_DEVICES && !d.electrical.representations.contains_key(&h) {
        return Err(bad(cmd, "electrical representation limit reached"));
    }
    if !assignable(d, h) {
        return Err(bad(cmd, "handle must name a single top-level non-xref block reference"));
    }
    if super::curves::is_locked(s, h) {
        return Err(bad(cmd, "component is on a locked layer"));
    }
    let e = d.entity(h).ok_or_else(|| bad(cmd, "block reference not found"))?;
    if e.common.xdata.iter().any(|t| t.code == 1001 && t.value.eq_ignore_ascii_case(APP)) {
        return Err(bad(cmd, "block carries unsupported electrical metadata; preserve or migrate it before assigning"));
    }
    let role = match p.get("role") {
        Some(v) => serde_json::from_value::<Role>(v.clone()).map_err(|_| bad(cmd, "role must be component, coil, contact or footprint"))?,
        None => Role::Component,
    };
    let (id, props) = if p.get("device").is_some() {
        if FIELDS.iter().any(|k| p.get(k).is_some()) {
            return Err(bad(cmd, "link by device identity without property fields; use electrical.component.update to edit shared properties"));
        }
        let id = text(cmd, p, "device")?;
        let props = d.electrical.devices.get(id).filter(|_| live(d, id).next().is_some()).ok_or_else(|| bad(cmd, "unknown device"))?;
        (id.to_string(), props.clone())
    } else {
        if !role.is_primary() {
            return Err(bad(cmd, "a new device requires a component or coil; link contacts/footprints to an existing device"));
        }
        if d.electrical.devices.len() >= MAX_DEVICES {
            return Err(bad(cmd, "electrical device limit reached"));
        }
        let props = properties(cmd, p, &Device::default())?;
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| EngineError::Other(format!("could not allocate electrical identity: {e}")))?;
        let id = format!("{:032x}", u128::from_be_bytes(bytes));
        if d.electrical.devices.contains_key(&id) {
            return Err(bad(cmd, "device identity collision; retry assignment"));
        }
        (id, props)
    };
    unique(d, cmd, &id, &props)?;
    if role.is_primary() && live(d, &id).any(|(other, role)| other != h && role.is_primary()) {
        return Err(bad(cmd, "device already has a primary component/coil representation"));
    }
    let rep = Representation { device: id.clone(), role };
    if d.electrical.representations.get(&h) != Some(&rep) {
        let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
        data.devices.insert(id.clone(), props);
        data.representations.insert(h, rep);
        prune(data);
    }
    Ok(json!({"device": id, "handle": h.hex(), "role": role}))
}

fn prune(data: &mut cadcraft_doc::electrical::ElectricalData) {
    let used: BTreeSet<&str> = data.representations.values().map(|r| r.device.as_str()).collect();
    data.devices.retain(|id, _| used.contains(id.as_str()));
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.component.update";
    object(cmd, p, &["device", "tag", "family", "manufacturer", "catalog", "description", "installation", "location"])?;
    writable(s, cmd)?;
    let id = text(cmd, p, "device")?;
    let d = s.doc()?;
    let old = d.electrical.devices.get(id).filter(|_| live(d, id).next().is_some()).ok_or_else(|| bad(cmd, "unknown device"))?;
    if live(d, id).any(|(h, _)| super::curves::is_locked(s, h)) {
        return Err(bad(cmd, "a device representation is on a locked layer"));
    }
    let props = properties(cmd, p, old)?;
    unique(d, cmd, id, &props)?;
    if &props != old {
        std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical).devices.insert(id.to_string(), props.clone());
    }
    Ok(json!({"device": id, "properties": props}))
}

fn detach(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.component.detach";
    object(cmd, p, &["handle"])?;
    writable(s, cmd)?;
    let h = handle(cmd, p)?;
    let rep = s.doc()?.electrical.representations.get(&h).ok_or_else(|| bad(cmd, "block has no electrical association"))?.clone();
    if super::curves::is_locked(s, h) {
        return Err(bad(cmd, "component is on a locked layer"));
    }
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    data.representations.remove(&h);
    prune(data);
    Ok(json!({"device": rep.device, "handle": h.hex()}))
}

fn unassigned(d: &Drawing) -> Vec<String> {
    d.model
        .iter()
        .chain(d.layouts.iter().flat_map(|l| l.entities.iter()))
        .filter(|e| matches!(e.kind, EntityKind::Insert(_)) && !d.electrical.representations.contains_key(&e.handle))
        .map(|e| e.handle.hex())
        .collect()
}

fn index(d: &Drawing) -> BTreeMap<&str, Vec<(Handle, Role)>> {
    let mut out: BTreeMap<&str, Vec<(Handle, Role)>> = BTreeMap::new();
    for (h, rep) in &d.electrical.representations {
        if assignable(d, *h) {
            out.entry(&rep.device).or_default().push((*h, rep.role));
        }
    }
    out
}

fn devices(d: &Drawing) -> Vec<Value> {
    let index = index(d);
    d.electrical
        .devices
        .iter()
        .filter_map(|(id, props)| {
            let reps: Vec<Value> = index.get(id.as_str()).into_iter().flatten().map(|(h, role)| json!({"handle": h.hex(), "role": role})).collect();
            (!reps.is_empty()).then(|| json!({"device": id, "properties": props, "representations": reps}))
        })
        .collect()
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    object("electrical.components", p, &[])?;
    let d = s.doc()?;
    bounded(d, "electrical.components")?;
    Ok(json!({"devices": devices(d), "unassigned": unassigned(d)}))
}

fn bom(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.bom";
    object(cmd, p, &["file"])?;
    let file = if p.get("file").is_some() { Some(text(cmd, p, "file")?) } else { None };
    if let Some(path) = file
        && !std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        return Err(bad(cmd, "BOM destination must have a .csv extension"));
    }
    let d = s.doc()?;
    bounded(d, cmd)?;
    // A missing catalog is kept per device: unknown parts must not be combined as one SKU.
    let mut groups: BTreeMap<(Device, String), Vec<String>> = BTreeMap::new();
    let mut missing_catalog = Vec::new();
    let mut missing_primary = Vec::new();
    let index = index(d);
    for (id, props) in &d.electrical.devices {
        let Some(reps) = index.get(id.as_str()) else { continue };
        if !reps.iter().any(|(_, role)| role.is_primary()) {
            missing_primary.push(id.clone());
        }
        let missing = props.catalog.is_empty() || props.manufacturer.is_empty();
        if missing {
            missing_catalog.push(id.clone());
        }
        let mut key = props.clone();
        key.tag.clear();
        groups.entry((key, if missing { id.clone() } else { String::new() })).or_default().push(id.clone());
    }
    let columns = ["Quantity", "Manufacturer", "Catalog", "Description", "Family", "Installation", "Location", "Tags"];
    let mut rows = Vec::new();
    let mut source_devices = Vec::new();
    for ((props, _), ids) in groups {
        let mut tags: Vec<&str> = ids.iter().filter_map(|id| d.electrical.devices.get(id).map(|p| p.tag.as_str())).collect();
        tags.sort();
        rows.push(json!([
            ids.len(),
            props.manufacturer,
            props.catalog,
            props.description,
            props.family,
            props.installation,
            props.location,
            tags.join("; ")
        ]));
        source_devices.push(ids);
    }
    let mut result = json!({"columns": columns, "rows": rows, "devices": source_devices,
        "unassigned": unassigned(d), "missingCatalog": missing_catalog, "missingPrimary": missing_primary});
    if let Some(path) = file {
        let mut csv = columns.join(",");
        csv.push_str("\r\n");
        for row in rows {
            if let Some(cells) = row.as_array() {
                let cells: Vec<String> = cells
                    .iter()
                    .map(|v| match v.as_str() {
                        Some(text) => csv_field(text),
                        None => v.to_string(),
                    })
                    .collect();
                csv.push_str(&cells.join(","));
                csv.push_str("\r\n");
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::fs::write(path, csv).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
        #[cfg(target_arch = "wasm32")]
        {
            let deliver = super::file::deliver().ok_or_else(|| EngineError::Other("file downloads unavailable in this host".into()))?;
            deliver(path, "text/csv;charset=utf-8", csv.as_bytes()).map_err(EngineError::Other)?;
        }
        result["file"] = json!(path);
    }
    Ok(result)
}

fn csv_field(text: &str) -> String {
    // Spreadsheet consumers must not evaluate user-controlled fields as formulas.
    let safe = if text.starts_with(['=', '+', '-', '@']) { format!("'{text}") } else { text.to_string() };
    if safe.contains([',', '"', '\r', '\n']) { format!("\"{}\"", safe.replace('"', "\"\"")) } else { safe }
}
