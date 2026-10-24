//! Physical devices are separate from their graphical representations. Ordinary geometry
//! copies have new handles and are deliberately unassigned until explicitly associated.

use std::collections::{BTreeMap, BTreeSet};

use cadcraft_geom::{Line, Vec2};
use serde::{Deserialize, Serialize};

use crate::{Drawing, EntityKind, Handle};

/// Version of the per-INSERT persistence envelope.
pub const SCHEMA: u32 = 2;
/// Registered application reserved for CADCraft's electrical metadata.
pub const APP: &str = "CADCRAFT_ELECTRICAL";
/// Bounds shared by commands and file readers.
pub const MAX_DEVICES: usize = 10_000;
pub const MAX_TEXT: usize = 1024;
pub const MAX_PAYLOAD: usize = 16_384;
/// Geometric tolerance for wire endpoint and junction snapping in drawing units.
pub const WIRE_TOLERANCE: f64 = 1e-4;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElectricalData {
    pub devices: BTreeMap<String, Device>,
    pub representations: BTreeMap<Handle, Representation>,
    /// Symbol pin definitions keyed by upper-case block name.
    pub symbols: BTreeMap<String, Vec<PinDef>>,
    /// Explicit junction points in the drawing.
    pub junctions: Vec<Junction>,
    /// Entity handles explicitly designated as wires.
    pub wires: BTreeSet<Handle>,
    /// Layer names designated as wire layers.
    pub wire_layers: BTreeSet<String>,
}

impl ElectricalData {
    pub fn is_wire_layer(&self, layer: &str) -> bool {
        layer.eq_ignore_ascii_case("WIRES") || layer.eq_ignore_ascii_case("WIRE") || self.wire_layers.iter().any(|l| l.eq_ignore_ascii_case(layer))
    }

    pub fn is_wire(&self, e: &crate::Entity) -> bool {
        if !matches!(e.kind, EntityKind::Line(_) | EntityKind::LwPolyline(_)) {
            return false;
        }
        self.wires.contains(&e.handle) || self.is_wire_layer(&e.common.layer)
    }
}

mod vec2_array {
    use cadcraft_geom::Vec2;
    use serde::ser::SerializeSeq;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(v: &Vec2, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(2))?;
        seq.serialize_element(&v.x)?;
        seq.serialize_element(&v.y)?;
        seq.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec2, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PointRepr {
            Arr([f64; 2]),
            Obj { x: f64, y: f64 },
        }
        let repr = PointRepr::deserialize(deserializer)?;
        match repr {
            PointRepr::Arr([x, y]) => Ok(Vec2::new(x, y)),
            PointRepr::Obj { x, y } => Ok(Vec2::new(x, y)),
        }
    }
}

/// A named connection point on a symbol definition, in block-local coordinates.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PinDef {
    pub name: String,
    #[serde(with = "vec2_array")]
    pub at: Vec2,
}

impl PinDef {
    pub fn new(name: impl Into<String>, at: Vec2) -> Self {
        Self { name: name.into(), at }
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("pin name must not be empty".into());
        }
        if name.len() > 256 || name.chars().any(char::is_control) {
            return Err("pin name must be at most 256 UTF-8 bytes without control characters".into());
        }
        if !self.at.x.is_finite() || !self.at.y.is_finite() {
            return Err("pin coordinates must be finite numbers".into());
        }
        Ok(())
    }
}

pub fn validate_pins(pins: &[PinDef]) -> std::result::Result<(), String> {
    if pins.len() > 256 {
        return Err("a symbol can have at most 256 pins".into());
    }
    let mut names = BTreeSet::new();
    for pin in pins {
        pin.validate()?;
        let key = pin.name.trim().to_ascii_uppercase();
        if !names.insert(key) {
            return Err(format!("duplicate pin name `{}` on symbol", pin.name.trim()));
        }
    }
    Ok(())
}

/// An explicit electrical junction point where intersecting wires connect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Junction {
    #[serde(with = "vec2_array")]
    pub at: Vec2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<Handle>,
}

impl Junction {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !self.at.x.is_finite() || !self.at.y.is_finite() {
            return Err("junction coordinates must be finite numbers".into());
        }
        Ok(())
    }
}

/// A resolved pin on an instance in world coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePin {
    pub name: String,
    #[serde(with = "vec2_array")]
    pub local: Vec2,
    #[serde(with = "vec2_array")]
    pub at: Vec2,
    pub block: String,
    pub handle: Handle,
    pub device: Option<String>,
    pub role: Option<Role>,
    pub tag: Option<String>,
}

/// A physical device, counted once regardless of how many views depict it.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Device {
    pub tag: String,
    pub family: String,
    pub manufacturer: String,
    pub catalog: String,
    pub description: String,
    pub installation: String,
    pub location: String,
}

impl Device {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.tag.trim().is_empty() {
            return Err("tag must not be empty".into());
        }
        for text in [&self.tag, &self.family, &self.manufacturer, &self.catalog, &self.description, &self.installation, &self.location] {
            if text.len() > MAX_TEXT || text.chars().any(char::is_control) {
                return Err(format!("component fields must be at most {MAX_TEXT} UTF-8 bytes without control characters"));
            }
        }
        Ok(())
    }

    /// Tags are ASCII case-insensitive and unique within installation/location.
    pub fn same_tag_scope(&self, other: &Self) -> bool {
        self.tag.eq_ignore_ascii_case(&other.tag)
            && self.installation.eq_ignore_ascii_case(&other.installation)
            && self.location.eq_ignore_ascii_case(&other.location)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    #[default]
    Component,
    Coil,
    Contact,
    Footprint,
}

impl Role {
    pub fn is_primary(self) -> bool {
        matches!(self, Self::Component | Self::Coil)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Representation {
    pub device: String,
    pub role: Role,
}

/// Repeated on each assigned INSERT so native handle remapping in DXF/DWG does not
/// invalidate associations. Conflicting copies are rejected on read rather than merged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Envelope {
    pub schema: u32,
    pub device: String,
    pub properties: Device,
    pub role: Role,
}

impl Envelope {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.schema != 1 && self.schema != SCHEMA {
            return Err(format!("unsupported electrical schema {}", self.schema));
        }
        if self.device.len() != 32 || !self.device.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid electrical device identity".into());
        }
        self.properties.validate()
    }
}

/// Only single top-level INSERTs can be associated in this first implementation.
pub fn assignable(d: &Drawing, handle: Handle) -> bool {
    d.entity(handle).is_some_and(|e| match &e.kind {
        EntityKind::Insert(i) => i.rows == 1 && i.cols == 1 && d.block(&i.block).is_some_and(|b| b.xref_path.is_none() && b.array.is_none()),
        _ => false,
    })
}

/// Compute all resolved pins for a given block reference in world coordinates.
pub fn instance_pins(d: &Drawing, handle: Handle) -> Vec<InstancePin> {
    let Some(e) = d.entity(handle) else { return Vec::new() };
    let EntityKind::Insert(ref ins) = e.kind else { return Vec::new() };
    let Some(blk) = d.block(&ins.block) else { return Vec::new() };
    let Some(pin_defs) = d.electrical.symbols.get(&ins.block.to_ascii_uppercase()) else {
        return Vec::new();
    };
    let transform = ins.transform(blk.base.xy());
    let rep = d.electrical.representations.get(&handle);
    let dev = rep.and_then(|r| d.electrical.devices.get(&r.device));

    pin_defs
        .iter()
        .map(|p| {
            let world_at = transform.apply(p.at);
            InstancePin {
                name: p.name.clone(),
                local: p.at,
                at: world_at,
                block: ins.block.clone(),
                handle,
                device: rep.map(|r| r.device.clone()),
                role: rep.map(|r| r.role),
                tag: dev.map(|d| d.tag.clone()),
            }
        })
        .collect()
}

/// Compute all resolved pins across model and layout spaces.
pub fn all_instance_pins(d: &Drawing) -> Vec<InstancePin> {
    let mut out = Vec::new();
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if matches!(e.kind, EntityKind::Insert(_)) {
            out.extend(instance_pins(d, e.handle));
        }
    }
    out
}

/// An electrically connected net of pins, wires and junctions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Net {
    pub id: usize,
    pub pins: Vec<InstancePin>,
    pub wires: Vec<Handle>,
    pub junctions: Vec<Vec2>,
    pub devices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct WireSeg {
    handle: Handle,
    p1: Vec2,
    p2: Vec2,
}

struct Dsu {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect(), rank: vec![0; n] }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            let next = self.parent[x];
            self.parent[x] = self.parent[next];
            x = next;
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a != root_b {
            match self.rank[root_a].cmp(&self.rank[root_b]) {
                std::cmp::Ordering::Less => self.parent[root_a] = root_b,
                std::cmp::Ordering::Greater => self.parent[root_b] = root_a,
                std::cmp::Ordering::Equal => {
                    self.parent[root_b] = root_a;
                    self.rank[root_a] = self.rank[root_a].saturating_add(1);
                }
            }
        }
    }
}

/// Computes all connected nets from pins, wires and explicit junctions.
/// Distinguishes crossing wires (which remain separate) from connected junctions.
pub fn compute_nets(d: &Drawing) -> (Vec<Net>, Vec<InstancePin>) {
    let mut wire_segs: Vec<WireSeg> = Vec::new();
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if !d.electrical.is_wire(e) {
            continue;
        }
        match &e.kind {
            EntityKind::Line(l) => {
                let p1 = l.a.xy();
                let p2 = l.b.xy();
                if p1.dist(p2) > WIRE_TOLERANCE {
                    wire_segs.push(WireSeg { handle: e.handle, p1, p2 });
                }
            }
            EntityKind::LwPolyline(poly) => {
                let n = poly.vertices.len();
                for i in 0..n.saturating_sub(1) {
                    let Some(v1) = poly.vertices.get(i) else { break };
                    let Some(v2) = poly.vertices.get(i + 1) else { break };
                    let p1 = v1.p;
                    let p2 = v2.p;
                    if p1.dist(p2) > WIRE_TOLERANCE {
                        wire_segs.push(WireSeg { handle: e.handle, p1, p2 });
                    }
                }
                if poly.closed
                    && n > 2
                    && let (Some(v1), Some(v2)) = (poly.vertices.last(), poly.vertices.first())
                {
                    let p1 = v1.p;
                    let p2 = v2.p;
                    if p1.dist(p2) > WIRE_TOLERANCE {
                        wire_segs.push(WireSeg { handle: e.handle, p1, p2 });
                    }
                }
            }
            _ => {}
        }
    }

    let pins = all_instance_pins(d);
    let mut junctions: Vec<Vec2> = d.electrical.junctions.iter().map(|j| j.at).collect();
    for e in d.model.iter().chain(d.layouts.iter().flat_map(|l| l.entities.iter())) {
        if e.common.layer.eq_ignore_ascii_case("JUNCTIONS") {
            let pt = match &e.kind {
                EntityKind::Point(p) => Some(p.p.xy()),
                EntityKind::Circle(c) => Some(c.center.xy()),
                _ => None,
            };
            if let Some(pos) = pt
                && !junctions.iter().any(|j| j.dist(pos) <= WIRE_TOLERANCE)
            {
                junctions.push(pos);
            }
        }
    }

    let num_wires = wire_segs.len();
    let num_pins = pins.len();
    let num_junctions = junctions.len();
    let total = num_wires.saturating_add(num_pins).saturating_add(num_junctions);
    let mut dsu = Dsu::new(total);

    // 1. Wire to Wire: endpoint-to-endpoint or same entity.
    for i in 0..num_wires {
        for j in (i + 1)..num_wires {
            let Some(w1) = wire_segs.get(i) else { break };
            let Some(w2) = wire_segs.get(j) else { break };
            if w1.handle == w2.handle
                || w1.p1.dist(w2.p1) <= WIRE_TOLERANCE
                || w1.p1.dist(w2.p2) <= WIRE_TOLERANCE
                || w1.p2.dist(w2.p1) <= WIRE_TOLERANCE
                || w1.p2.dist(w2.p2) <= WIRE_TOLERANCE
            {
                dsu.union(i, j);
            }
        }
    }

    // 2. Explicit Junctions: connect any wire passing through or touching the junction.
    // Crucial: crossing wires without a junction do NOT connect; with a junction, both connect to J!
    for (k, jpos) in junctions.iter().enumerate() {
        let junc_idx = num_wires + num_pins + k;
        for (i, w) in wire_segs.iter().enumerate() {
            if Line::new(w.p1, w.p2).dist(*jpos) <= WIRE_TOLERANCE {
                dsu.union(junc_idx, i);
            }
        }
    }

    // 3. Pins: connect any pin touching a wire or explicit junction.
    for (m, pin) in pins.iter().enumerate() {
        let pin_idx = num_wires + m;
        for (i, w) in wire_segs.iter().enumerate() {
            if Line::new(w.p1, w.p2).dist(pin.at) <= WIRE_TOLERANCE {
                dsu.union(pin_idx, i);
            }
        }
        for (k, jpos) in junctions.iter().enumerate() {
            if pin.at.dist(*jpos) <= WIRE_TOLERANCE {
                let junc_idx = num_wires + num_pins + k;
                dsu.union(pin_idx, junc_idx);
            }
        }
    }

    // Group into connected components.
    let mut groups: BTreeMap<usize, (BTreeSet<Handle>, Vec<InstancePin>, Vec<Vec2>, BTreeSet<String>)> = BTreeMap::new();
    for i in 0..num_wires {
        let root = dsu.find(i);
        if let Some(w) = wire_segs.get(i) {
            groups.entry(root).or_default().0.insert(w.handle);
        }
    }
    for (m, pin) in pins.iter().enumerate() {
        let pin_idx = num_wires + m;
        let root = dsu.find(pin_idx);
        let entry = groups.entry(root).or_default();
        entry.1.push(pin.clone());
        if let Some(dev) = &pin.device {
            entry.3.insert(dev.clone());
        }
    }
    for (k, jpos) in junctions.iter().enumerate() {
        let junc_idx = num_wires + num_pins + k;
        let root = dsu.find(junc_idx);
        groups.entry(root).or_default().2.push(*jpos);
    }

    let mut nets = Vec::new();
    let mut unconnected = Vec::new();

    for (_root, (wires_set, mut comp_pins, comp_juncs, devs_set)) in groups {
        let has_conductor = !wires_set.is_empty() || !comp_juncs.is_empty();
        if has_conductor || comp_pins.len() > 1 {
            comp_pins.sort_by(|a, b| a.handle.cmp(&b.handle).then_with(|| a.name.cmp(&b.name)));
            let wires: Vec<Handle> = wires_set.into_iter().collect();
            let devices: Vec<String> = devs_set.into_iter().collect();
            nets.push((wires, comp_pins, comp_juncs, devices));
        } else if comp_pins.len() == 1
            && let Some(p) = comp_pins.into_iter().next()
        {
            unconnected.push(p);
        }
    }

    nets.sort_by(|a, b| {
        let a_wire = a.0.first().map(|h| h.0).unwrap_or(u64::MAX);
        let b_wire = b.0.first().map(|h| h.0).unwrap_or(u64::MAX);
        a_wire.cmp(&b_wire).then_with(|| {
            let a_pin = a.1.first().map(|p| p.handle.0).unwrap_or(u64::MAX);
            let b_pin = b.1.first().map(|p| p.handle.0).unwrap_or(u64::MAX);
            a_pin.cmp(&b_pin)
        })
    });

    let assigned_nets = nets
        .into_iter()
        .enumerate()
        .map(|(idx, (wires, comp_pins, comp_juncs, devices))| Net { id: idx + 1, pins: comp_pins, wires, junctions: comp_juncs, devices })
        .collect();

    unconnected.sort_by(|a, b| a.handle.cmp(&b.handle).then_with(|| a.name.cmp(&b.name)));

    (assigned_nets, unconnected)
}
