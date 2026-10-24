//! Physical devices are separate from their graphical representations. Ordinary geometry
//! copies have new handles and are deliberately unassigned until explicitly associated.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Drawing, EntityKind, Handle};

/// Version of the per-INSERT persistence envelope.
pub const SCHEMA: u32 = 1;
/// Registered application reserved for CADCraft's electrical metadata.
pub const APP: &str = "CADCRAFT_ELECTRICAL";
/// Bounds shared by commands and file readers.
pub const MAX_DEVICES: usize = 10_000;
pub const MAX_TEXT: usize = 1024;
pub const MAX_PAYLOAD: usize = 16_384;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ElectricalData {
    pub devices: BTreeMap<String, Device>,
    pub representations: BTreeMap<Handle, Representation>,
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
        if self.schema != SCHEMA {
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
