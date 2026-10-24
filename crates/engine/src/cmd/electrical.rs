//! Single-drawing electrical device associations, symbol pins, wire connectivity and physical BOMs.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_doc::electrical::{
    APP, Device, Junction, MAX_DEVICES, MAX_TEXT, PinDef, Representation, Role, WIRE_TOLERANCE, all_instance_pins, assignable, compute_nets,
    instance_pins, validate_pins,
};
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_geom::Vec2;
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
        CommandSpec::new("electrical.pin.define", "Define Symbol Pin", pin_define)
            .params("{block?: symbol name, handle?: INSERT handle, pins?: [{name, at: [x,y]}], name?: string, at?: [x,y]} → {block, pins}"),
        CommandSpec::new("electrical.pin.remove", "Remove Symbol Pin", pin_remove)
            .params("{block?: symbol name, handle?: INSERT handle, name?: pin name} → {block, removed}"),
        CommandSpec::new("electrical.pins", "Query Symbol Pins", pins_query)
            .params("{block?: symbol name, handle?: INSERT handle} → {symbols?, pins?}")
            .noundo(),
        CommandSpec::new("electrical.wire", "Draw Electrical Wire", wire)
            .params("{points: [[x,y], ...], layer?: string} → {handles}"),
        CommandSpec::new("electrical.wire.assign", "Assign Wire Entities", wire_assign)
            .params("{handles: [string]} → {wires}"),
        CommandSpec::new("electrical.wire.detach", "Detach Wire Entities", wire_detach)
            .params("{handles: [string]} → {detached}"),
        CommandSpec::new("electrical.junction", "Create Electrical Junction", junction)
            .params("{at?: [x,y], handle?: string, layer?: string} → {at, handle}"),
        CommandSpec::new("electrical.junction.remove", "Remove Electrical Junction", junction_remove)
            .params("{at?: [x,y], handle?: string} → {removed}"),
        CommandSpec::new("electrical.junctions", "List Electrical Junctions", junctions_list)
            .params("{} → {junctions: [{at, handle?}]}")
            .noundo(),
        CommandSpec::new("electrical.net.inspect", "Inspect Electrical Net", net_inspect)
            .alias(&["electrical.net"])
            .params("{handle?, pin?: {handle, name}, device?, point?, net?: number, highlight?: bool} → {net, pins, wires, junctions, devices}")
            .noundo(),
        CommandSpec::new("electrical.net.highlight", "Highlight Electrical Net", net_highlight)
            .params("{handle?, pin?: {handle, name}, device?, point?, net?: number} → {net, pins, wires, junctions, devices}")
            .noundo(),
        CommandSpec::new("electrical.nets", "List Electrical Nets", nets_list)
            .params("{} → {nets, unconnectedPins}")
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

fn point_2d(cmd: &str, v: &Value) -> Result<Vec2> {
    let arr = v.as_array().ok_or_else(|| bad(cmd, "point must be a 2-element array [x, y]"))?;
    if arr.len() != 2 {
        return Err(bad(cmd, "point must be a 2-element array [x, y]"));
    }
    let x = arr.first().and_then(Value::as_f64).filter(|x| x.is_finite()).ok_or_else(|| bad(cmd, "coordinate must be a finite number"))?;
    let y = arr.get(1).and_then(Value::as_f64).filter(|y| y.is_finite()).ok_or_else(|| bad(cmd, "coordinate must be a finite number"))?;
    Ok(Vec2::new(x, y))
}

fn parse_pin_def(cmd: &str, v: &Value) -> Result<PinDef> {
    let obj = v.as_object().ok_or_else(|| bad(cmd, "pin definition must be an object"))?;
    let name = obj.get("name").and_then(Value::as_str).ok_or_else(|| bad(cmd, "pin must have a name"))?;
    let at_val = obj.get("at").ok_or_else(|| bad(cmd, "pin must have an `at` coordinate [x, y]"))?;
    let at = point_2d(cmd, at_val)?;
    let pin = PinDef::new(name, at);
    pin.validate().map_err(|e| bad(cmd, e))?;
    Ok(pin)
}

fn resolve_block_name(d: &Drawing, cmd: &str, p: &Value) -> Result<String> {
    if p.get("block").is_some() {
        let name = text(cmd, p, "block")?;
        return Ok(name.to_string());
    }
    if p.get("handle").is_some() {
        let h = handle(cmd, p)?;
        let e = d.entity(h).ok_or_else(|| bad(cmd, "handle not found"))?;
        if let EntityKind::Insert(ref ins) = e.kind {
            return Ok(ins.block.clone());
        }
        return Err(bad(cmd, "handle must reference a block insertion"));
    }
    Err(bad(cmd, "must specify `block` name or INSERT `handle`"))
}

fn pin_define(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.pin.define";
    object(cmd, p, &["block", "handle", "pins", "name", "at"])?;
    writable(s, cmd)?;
    let block = resolve_block_name(s.doc()?, cmd, p)?;
    let mut new_pins = Vec::new();
    if let Some(pins_val) = p.get("pins") {
        let arr = pins_val.as_array().ok_or_else(|| bad(cmd, "`pins` must be an array"))?;
        for v in arr {
            new_pins.push(parse_pin_def(cmd, v)?);
        }
    } else if p.get("name").is_some() && p.get("at").is_some() {
        let name = text(cmd, p, "name")?;
        let at_val = p.get("at").ok_or_else(|| bad(cmd, "missing `at` coordinate"))?;
        let at = point_2d(cmd, at_val)?;
        new_pins.push(PinDef::new(name, at));
    } else {
        return Err(bad(cmd, "must specify `pins` array or `name` and `at`"));
    }
    validate_pins(&new_pins).map_err(|e| bad(cmd, e))?;

    let key = block.to_ascii_uppercase();
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    let entry = data.symbols.entry(key).or_default();
    for np in new_pins {
        entry.retain(|existing| !existing.name.eq_ignore_ascii_case(&np.name));
        entry.push(np);
    }
    validate_pins(entry).map_err(|e| bad(cmd, e))?;
    let current_pins = entry.clone();
    Ok(json!({"block": block, "pins": current_pins}))
}

fn pin_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.pin.remove";
    object(cmd, p, &["block", "handle", "name"])?;
    writable(s, cmd)?;
    let block = resolve_block_name(s.doc()?, cmd, p)?;
    let key = block.to_ascii_uppercase();
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    let mut removed = Vec::new();
    if let Some(nv) = p.get("name") {
        let name = nv.as_str().ok_or_else(|| bad(cmd, "`name` must be a string"))?;
        if let Some(pins) = data.symbols.get_mut(&key) {
            pins.retain(|pin| {
                if pin.name.eq_ignore_ascii_case(name) {
                    removed.push(pin.name.clone());
                    false
                } else {
                    true
                }
            });
        }
    } else if let Some(pins) = data.symbols.remove(&key) {
        removed = pins.into_iter().map(|pin| pin.name).collect();
    }
    Ok(json!({"block": block, "removed": removed}))
}

fn pins_query(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.pins";
    object(cmd, p, &["block", "handle"])?;
    let d = s.doc()?;
    bounded(d, cmd)?;
    if p.get("handle").is_some() {
        let h = handle(cmd, p)?;
        let e = d.entity(h).ok_or_else(|| bad(cmd, "handle not found"))?;
        let EntityKind::Insert(ref ins) = e.kind else {
            return Err(bad(cmd, "handle must reference a block reference (INSERT)"));
        };
        let resolved = instance_pins(d, h);
        return Ok(json!({
            "handle": h.hex(),
            "block": ins.block,
            "pins": resolved
        }));
    }
    if p.get("block").is_some() {
        let name = text(cmd, p, "block")?;
        let pins = d.electrical.symbols.get(&name.to_ascii_uppercase()).cloned().unwrap_or_default();
        return Ok(json!({
            "block": name,
            "pins": pins
        }));
    }
    let all_pins = all_instance_pins(d);
    let symbols: Vec<Value> = d.electrical.symbols.iter().map(|(blk, pins)| json!({"block": blk, "pins": pins})).collect();
    Ok(json!({
        "symbols": symbols,
        "pins": all_pins
    }))
}

fn wire(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.wire";
    object(cmd, p, &["points", "layer"])?;
    writable(s, cmd)?;
    let pts_val = p.get("points").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`points` must be an array of at least 2 points"))?;
    if pts_val.len() < 2 {
        return Err(bad(cmd, "`points` must have at least 2 points"));
    }
    let mut points = Vec::new();
    for pv in pts_val {
        points.push(point_2d(cmd, pv)?);
    }
    let layer = p.get("layer").and_then(Value::as_str).map(str::trim).unwrap_or("WIRES");
    if layer.is_empty() || layer.len() > MAX_TEXT || layer.chars().any(char::is_control) {
        return Err(bad(cmd, "invalid layer name"));
    }
    if let Some(l) = s.doc()?.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer))
        && l.locked
    {
        return Err(bad(cmd, "wire layer is locked"));
    }
    s.doc_mut()?.ensure_layer(layer);
    let mut handles = Vec::new();
    for w in points.windows(2) {
        let (Some(&p1), Some(&p2)) = (w.first(), w.get(1)) else { break };
        if p1.dist(p2) <= WIRE_TOLERANCE {
            continue;
        }
        let kind =
            EntityKind::Line(cadcraft_doc::Line { a: cadcraft_geom::Vec3::new(p1.x, p1.y, 0.0), b: cadcraft_geom::Vec3::new(p2.x, p2.y, 0.0) });
        let h = s.add_entity(kind)?;
        s.doc_mut()?.modify_entity(h, |e| e.common.layer = layer.to_string())?;
        handles.push(h);
    }
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    for h in &handles {
        data.wires.insert(*h);
    }
    let hex_handles: Vec<String> = handles.iter().map(|h| h.hex()).collect();
    Ok(json!({"handles": hex_handles}))
}

fn wire_assign(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.wire.assign";
    object(cmd, p, &["handles"])?;
    writable(s, cmd)?;
    let arr = p.get("handles").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`handles` must be an array of handle strings"))?;
    let mut handles = Vec::new();
    let d = s.doc()?;
    for v in arr {
        let hex = v.as_str().ok_or_else(|| bad(cmd, "handle must be a string"))?;
        let h = Handle::parse_hex(hex).ok_or_else(|| bad(cmd, "invalid hexadecimal handle"))?;
        let e = d.entity(h).ok_or_else(|| bad(cmd, format!("entity {hex} not found")))?;
        if !matches!(e.kind, EntityKind::Line(_) | EntityKind::LwPolyline(_)) {
            return Err(bad(cmd, format!("entity {hex} is not a LINE or LWPOLYLINE")));
        }
        if super::curves::is_locked(s, h) {
            return Err(bad(cmd, format!("entity {hex} is on a locked layer")));
        }
        handles.push(h);
    }
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    for h in &handles {
        data.wires.insert(*h);
    }
    Ok(json!({"wires": handles.iter().map(|h| h.hex()).collect::<Vec<_>>()}))
}

fn wire_detach(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.wire.detach";
    object(cmd, p, &["handles"])?;
    writable(s, cmd)?;
    let arr = p.get("handles").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`handles` must be an array of handle strings"))?;
    let mut detached = Vec::new();
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    for v in arr {
        let hex = v.as_str().ok_or_else(|| bad(cmd, "handle must be a string"))?;
        let h = Handle::parse_hex(hex).ok_or_else(|| bad(cmd, "invalid hexadecimal handle"))?;
        if data.wires.remove(&h) {
            detached.push(h.hex());
        }
    }
    Ok(json!({"detached": detached}))
}

fn junction(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.junction";
    object(cmd, p, &["at", "handle", "layer"])?;
    writable(s, cmd)?;
    if p.get("at").is_none() && p.get("handle").is_none() {
        return Err(bad(cmd, "must specify `at` coordinate or entity `handle`"));
    }
    if let Some(at_val) = p.get("at") {
        let at = point_2d(cmd, at_val)?;
        let layer = p.get("layer").and_then(Value::as_str).map(str::trim).unwrap_or("JUNCTIONS");
        if layer.is_empty() || layer.len() > MAX_TEXT || layer.chars().any(char::is_control) {
            return Err(bad(cmd, "invalid layer name"));
        }
        if let Some(l) = s.doc()?.layers.iter().find(|l| l.name.eq_ignore_ascii_case(layer))
            && l.locked
        {
            return Err(bad(cmd, "junction layer is locked"));
        }
        s.doc_mut()?.ensure_layer(layer);
        let kind = EntityKind::Point(cadcraft_doc::Point { p: cadcraft_geom::Vec3::new(at.x, at.y, 0.0), angle: 0.0 });
        let h = s.add_entity(kind)?;
        s.doc_mut()?.modify_entity(h, |e| e.common.layer = layer.to_string())?;
        let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
        data.junctions.push(Junction { at, handle: Some(h) });
        Ok(json!({"at": [at.x, at.y], "handle": h.hex()}))
    } else {
        let h = handle(cmd, p)?;
        let d = s.doc()?;
        let e = d.entity(h).ok_or_else(|| bad(cmd, "entity not found"))?;
        let at = match &e.kind {
            EntityKind::Point(pt) => pt.p.xy(),
            EntityKind::Circle(c) => c.center.xy(),
            _ => return Err(bad(cmd, "entity must be a POINT or CIRCLE")),
        };
        let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
        data.junctions.push(Junction { at, handle: Some(h) });
        Ok(json!({"at": [at.x, at.y], "handle": h.hex()}))
    }
}

fn junction_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.junction.remove";
    object(cmd, p, &["at", "handle"])?;
    writable(s, cmd)?;
    let target_handle = if p.get("handle").is_some() { Some(handle(cmd, p)?) } else { None };
    let target_pt = if let Some(v) = p.get("at") { Some(point_2d(cmd, v)?) } else { None };
    if target_handle.is_none() && target_pt.is_none() {
        return Err(bad(cmd, "must specify `at` coordinate or entity `handle`"));
    }
    let data = std::sync::Arc::make_mut(&mut s.doc_mut()?.electrical);
    let mut removed_handle = None;
    let before = data.junctions.len();
    data.junctions.retain(|j| {
        let matches_h = target_handle.is_some_and(|th| j.handle == Some(th));
        let matches_pt = target_pt.is_some_and(|tp| tp.dist(j.at) <= WIRE_TOLERANCE);
        if matches_h || matches_pt {
            if let Some(h) = j.handle {
                removed_handle = Some(h);
            }
            false
        } else {
            true
        }
    });
    let removed = data.junctions.len() < before;
    if let Some(h) = removed_handle {
        let _ = s.doc_mut()?.remove_entity(h);
    }
    Ok(json!({"removed": removed}))
}

fn junctions_list(s: &mut Session, p: &Value) -> Result<Value> {
    object("electrical.junctions", p, &[])?;
    let d = s.doc()?;
    bounded(d, "electrical.junctions")?;
    let list: Vec<Value> = d
        .electrical
        .junctions
        .iter()
        .map(|j| {
            json!({
                "at": [j.at.x, j.at.y],
                "handle": j.handle.map(|h| h.hex())
            })
        })
        .collect();
    Ok(json!({"junctions": list}))
}

fn net_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "electrical.net.inspect";
    object(cmd, p, &["handle", "pin", "device", "point", "net", "highlight"])?;
    let d = s.doc()?;
    bounded(d, cmd)?;

    let target_net_id = p.get("net").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
    let target_handle = if p.get("handle").is_some() { Some(handle(cmd, p)?) } else { None };
    let target_device = p.get("device").and_then(Value::as_str);
    let target_pt = if let Some(pv) = p.get("point") { Some(point_2d(cmd, pv)?) } else { None };
    let target_pin = if let Some(pin_val) = p.get("pin") {
        let obj = pin_val.as_object().ok_or_else(|| bad(cmd, "`pin` must be an object {handle, name}"))?;
        let h_str = obj.get("handle").and_then(Value::as_str).ok_or_else(|| bad(cmd, "missing pin `handle`"))?;
        let h = Handle::parse_hex(h_str).ok_or_else(|| bad(cmd, "invalid pin handle"))?;
        let name = obj.get("name").and_then(Value::as_str).ok_or_else(|| bad(cmd, "missing pin `name`"))?;
        Some((h, name.to_string()))
    } else {
        None
    };

    if target_net_id.is_none() && target_handle.is_none() && target_device.is_none() && target_pt.is_none() && target_pin.is_none() {
        return Err(bad(cmd, "must specify `handle`, `pin`, `device`, `point` or `net` id"));
    }

    let (nets, unconnected) = compute_nets(d);

    let found_net = nets.into_iter().find(|net| {
        if let Some(id) = target_net_id
            && net.id == id
        {
            return true;
        }
        if let Some(th) = target_handle
            && (net.wires.contains(&th)
                || net.pins.iter().any(|p| p.handle == th)
                || d.electrical.junctions.iter().any(|j| j.handle == Some(th) && net.junctions.iter().any(|jp| jp.dist(j.at) <= WIRE_TOLERANCE)))
        {
            return true;
        }
        if let Some(dev) = target_device
            && net.devices.iter().any(|d| d == dev)
        {
            return true;
        }
        if let Some(ref tp) = target_pin
            && net.pins.iter().any(|p| p.handle == tp.0 && p.name.eq_ignore_ascii_case(&tp.1))
        {
            return true;
        }
        if let Some(pt) = target_pt {
            if net.junctions.iter().any(|j| j.dist(pt) <= WIRE_TOLERANCE) || net.pins.iter().any(|p| p.at.dist(pt) <= WIRE_TOLERANCE) {
                return true;
            }
            for &wh in &net.wires {
                if let Some(e) = d.entity(wh)
                    && let EntityKind::Line(l) = &e.kind
                    && cadcraft_geom::Line::new(l.a.xy(), l.b.xy()).dist(pt) <= WIRE_TOLERANCE
                {
                    return true;
                }
            }
        }
        false
    });

    let highlight = p.get("highlight").and_then(Value::as_bool).unwrap_or(false);

    if let Some(net) = found_net {
        if highlight {
            let mut sel: Vec<Handle> = net.wires.clone();
            for p in &net.pins {
                sel.push(p.handle);
            }
            for j in &d.electrical.junctions {
                if let Some(jh) = j.handle
                    && net.junctions.iter().any(|jp| jp.dist(j.at) <= WIRE_TOLERANCE)
                {
                    sel.push(jh);
                }
            }
            s.set_selection(sel);
        }
        let wire_hex: Vec<String> = net.wires.iter().map(|h| h.hex()).collect();
        let junc_coords: Vec<[f64; 2]> = net.junctions.iter().map(|j| [j.x, j.y]).collect();
        return Ok(json!({
            "net": net.id,
            "pins": net.pins,
            "wires": wire_hex,
            "junctions": junc_coords,
            "devices": net.devices,
        }));
    }

    // Check if it matches an unconnected pin.
    if let Some(unconn) = unconnected.into_iter().find(|p| {
        if let Some(th) = target_handle
            && p.handle == th
        {
            return true;
        }
        if let Some(dev) = target_device
            && p.device.as_deref() == Some(dev)
        {
            return true;
        }
        if let Some(ref tp) = target_pin
            && p.handle == tp.0
            && p.name.eq_ignore_ascii_case(&tp.1)
        {
            return true;
        }
        if let Some(pt) = target_pt
            && p.at.dist(pt) <= WIRE_TOLERANCE
        {
            return true;
        }
        false
    }) {
        if highlight {
            s.set_selection(vec![unconn.handle]);
        }
        return Ok(json!({
            "net": null,
            "connected": false,
            "pins": [unconn],
            "wires": [],
            "junctions": [],
            "devices": unconn.device.into_iter().collect::<Vec<_>>()
        }));
    }

    Ok(json!({
        "net": null,
        "pins": [],
        "wires": [],
        "junctions": [],
        "devices": []
    }))
}

fn net_highlight(s: &mut Session, p: &Value) -> Result<Value> {
    let mut params = p.clone();
    if let Some(obj) = params.as_object_mut() {
        obj.insert("highlight".into(), Value::Bool(true));
    }
    net_inspect(s, &params)
}

fn nets_list(s: &mut Session, p: &Value) -> Result<Value> {
    object("electrical.nets", p, &[])?;
    let d = s.doc()?;
    bounded(d, "electrical.nets")?;
    let (nets, unconnected) = compute_nets(d);
    let nets_val: Vec<Value> = nets
        .into_iter()
        .map(|net| {
            let wire_hex: Vec<String> = net.wires.iter().map(|h| h.hex()).collect();
            let junc_coords: Vec<[f64; 2]> = net.junctions.iter().map(|j| [j.x, j.y]).collect();
            json!({
                "id": net.id,
                "pins": net.pins,
                "wires": wire_hex,
                "junctions": junc_coords,
                "devices": net.devices,
            })
        })
        .collect();
    Ok(json!({
        "nets": nets_val,
        "unconnectedPins": unconnected
    }))
}
