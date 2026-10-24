# Electrical components and bills of materials

CADCraft can associate ordinary block references with physical electrical devices.
A relay's coil, contacts and panel footprint share one device identity and contribute
one device to its BOM. Two relays with the same manufacturer and part number contribute
quantity two. This is a single-drawing foundation for electrical work; wire connectivity,
pin assignments, cross-sheet references and electrical project management are not implemented.

## Commands

All commands use the same engine registry through the command line, scripts, CLI,
control channel and MCP. They currently accept JSON parameters; there is no electrical
palette or guided insertion dialog. Use `electrical.components {}` to inspect associations.

| Command | Behavior |
|---|---|
| `electrical.component.assign` | Create a device on an existing block reference, or link a reference to an existing device |
| `electrical.component.update` | Update shared properties by device identity |
| `electrical.component.detach` | Remove a reference's association while retaining geometry and visible attributes |
| `electrical.components` | List live devices, their representations and unassigned block references |
| `electrical.bom` | Return a physical-device BOM, optionally writing a CSV |
| `electrical.pin.define` | Define named connection pins on a symbol in block-local coordinates |
| `electrical.pin.remove` | Remove pin(s) from a symbol definition |
| `electrical.pins` | Query pins on a symbol or resolved world-space pins on an instance |
| `electrical.wire` | Draw wire segments on a wire layer and register them |
| `electrical.wire.assign` | Register existing lines/polylines as wires |
| `electrical.wire.detach` | Remove wire registration |
| `electrical.junction` | Add an explicit electrical junction dot at a coordinate or on an entity |
| `electrical.junction.remove` | Remove an explicit junction |
| `electrical.junctions` | List active junctions |
| `electrical.net.inspect` | Inspect the net containing a wire, pin, device or point |
| `electrical.net.highlight` | Inspect and highlight a net's entities in the active selection |
| `electrical.nets` | Compute and list all nets and unconnected pins |

These are CADCraft extension commands, not claims of compatible AutoCAD Electrical commands.
Programmatic calls never open dialogs. Successful edits participate in drawing undo/redo;
list/BOM queries and CSV export do not dirty the drawing or create undo steps.

## Associate a relay

Create or insert your own block symbols, then use their hexadecimal handles. For
example, with existing references `100`, `101` and `102`, enter this at the command line:

```text
electrical.component.assign {"handle":"100","role":"coil","tag":"K1","family":"relay","manufacturer":"Example","catalog":"R-2","description":"Control relay","installation":"Machine 1","location":"Panel A"}
```

The result contains a random 32-character hexadecimal `device` identity. Substitute
that actual result for `<device-id>` below; the handle, editable tag and device identity
are different things.

```text
electrical.component.assign {"handle":"101","device":"<device-id>","role":"contact"}
electrical.component.assign {"handle":"102","device":"<device-id>","role":"contact"}
electrical.components {}
electrical.bom {}
```

The three views contribute one relay. Assign another block as a new device with tag
`K2` and the same other fields to obtain a BOM row with quantity two and tags `K1; K2`.

Assigning a new device requires a non-empty `tag`. Its role defaults to `component`;
`coil` is the other primary role. Contacts and footprints must link to an existing
device using `device`, with role `contact` or `footprint`. A device can have at most
one primary representation. Linking an existing device accepts no property fields;
edit shared properties explicitly instead:

```text
electrical.component.update {"device":"<device-id>","tag":"K9","catalog":"R-4"}
electrical.component.detach {"handle":"102"}
```

Device identity survives retagging and metadata changes. Tags are ASCII
case-insensitive and unique within an installation/location pair. Properties are
`tag`, `family`, `manufacturer`, `catalog`, `description`, `installation` and `location`.
Missing optional fields default to empty strings. Edits trim surrounding whitespace;
each field permits up to 1,024 UTF-8 bytes without control characters. Unknown parameters,
invalid roles and wrongly typed fields are rejected before changing the drawing.

Shared device properties are separate from displayed block attributes. These commands
do not rewrite a symbol's visible tag/description text or supply symbols/catalog content.
They refuse changes on locked layers and while BEDIT or REFEDIT is active.

## BOM results and CSV

```text
electrical.bom {"file":"/tmp/panel-bom.csv"}
```

The result contains:

| Field | Meaning |
|---|---|
| `columns`, `rows` | Quantity, manufacturer, catalog, description, family, installation, location and tags |
| `devices` | Device identity lists corresponding to each BOM row |
| `unassigned` | Top-level INSERT handles without associations; includes ordinary title blocks and other non-electrical blocks |
| `missingCatalog` | Live device identities without a manufacturer or catalog number |
| `missingPrimary` | Live device identities represented only by contacts/footprints |
| `file` | Destination, when requested |

BOM rows combine devices whose non-tag properties match exactly. Devices without a
manufacturer or catalog number remain separate rows so unknown parts are not treated
as one known SKU. Rows and tags have deterministic ordering. The BOM covers assigned
devices only; inspect its diagnostics before using it as a purchasing list.

Deleting or detaching a coil does not erase a physical device still represented by
contacts; it remains counted and appears in `missingPrimary`. Once no representations
remain in model or paper space, that device is omitted. Undo restores associations and
counts with the drawing. Detaching the last stored association removes its device record.

CSV output uses UTF-8, quoted fields and CRLF line endings. Text beginning with
`=`, `+`, `-` or `@` receives a leading apostrophe to prevent spreadsheet formula
evaluation; JSON results retain the original text. Native export writes the requested
`.csv` path, replacing an existing file. Browser export uses the host's file-download
hook and reports an error if that host cannot deliver files.

## Persistence and copy behavior

Electrical metadata travels as versioned `CADCRAFT_ELECTRICAL` xdata on each assigned
INSERT. Each representation carries the stable device identity, role and shared
properties, allowing DXF/DWG handle renumbering without breaking links. The reader
reconstructs a copy-on-write drawing registry. Supported metadata is consumed from
raw xdata so generic geometry copies cannot accidentally inherit a physical identity.

Synthetic tests cover ASCII DXF, binary DXF and DWG read/save/reopen, Unicode fields
and unrelated foreign xdata. This does not establish AutoCAD Electrical recognition
of exported components as intelligent electrical objects or lossless general DWG
exchange. Existing format limitations still apply.

Unknown versions or malformed electrical payloads stay uninterpreted in raw xdata
with a read note. Assignment refuses to overwrite that metadata. Conflicting property
copies, duplicate scoped tags and duplicate primary representations in supported
payloads cause a clear import error instead of merging inconsistent devices.

Ordinary COPY, MIRROR with Copy, arrays and clipboard operations do not establish
electrical associations on new references. Copies are unassigned and appear in report
diagnostics. Explicitly assign a copy as a new device or link it as another view.
Circuit-aware copying and cross-drawing identity remapping are future work. WBLOCK,
block-library exports and clipboard transfer currently export geometry/attributes
without transferring the electrical registry; use full drawing saves for persistence.

Only single, top-level block references in model or paper space can be assigned.
Nested block instances, MINSERT grids, associative array containers and xrefs cannot
be assigned. Commands currently cap processing at 100,000 top-level drawing entities
and 10,000 stored devices/representations. Metadata is not an electrical connectivity
graph, a parts-catalog database or a safety/compliance check.

## Symbol pins and wire connectivity

Symbols (block definitions) can define named connection pins in block-local coordinates:

```text
electrical.pin.define {"block":"RELAY_COIL","pins":[{"name":"A1","at":[-10,0]},{"name":"A2","at":[10,0]}]}
electrical.pin.define {"block":"CONTACT_NO","pins":[{"name":"13","at":[0,5]},{"name":"14","at":[0,-5]}]}
```

When an instance of the block is inserted, rotated or mirrored (e.g. through negative scaling),
the instance's pins are automatically transformed to world space:
- Translation shifts pin positions to the insertion point.
- Rotation rotates pin directions around the insertion base.
- Mirroring (e.g. `scale.x = -1.0` or `scale.y = -1.0`) flips pin positions across the mirror axis.

Use `electrical.pins {"handle":"<insert-handle>"}` to inspect the resolved world coordinates of an
instance's pins.

### Wires and explicit junctions

Wires connect pins and explicit junctions to form electrical nets:
- `electrical.wire {"points":[[10,0],[40,0]]}` creates LINE segments on layer `WIRES` and marks them as wires.
- `electrical.wire.assign {"handles":["101"]}` registers existing lines or polylines as wires.
- Connected endpoints (within tolerance `1e-4`) form a continuous conductor.
- **Crossing wires versus connected junctions**: When two wires cross without an explicit junction,
  they are treated as crossing conductors and remain in separate electrical nets. Placing an explicit
  junction at the intersection connects both wires into the same net:
  ```text
  electrical.junction {"at":[10,10]}
  ```
- Removing or undoing the junction splits the wires back into independent nets.

### Net inspection and highlighting

Use `electrical.net.inspect` to trace the net connected to any wire, pin, device or coordinate:

```text
electrical.net.inspect {"handle":"<wire-or-insert-handle>"}
electrical.net.inspect {"pin":{"handle":"<insert-handle>","name":"A1"}}
electrical.net.inspect {"point":[10,10]}
electrical.net.highlight {"handle":"<wire-handle>"}
electrical.nets {}
```

Passing `"highlight": true` or using `electrical.net.highlight` selects the wire and symbol representation
handles of the net in the session selection, highlighting them in the interface.

All wire creation, junction placement and pin definition participate in the drawing undo/redo history.
Pins, wires and explicit junctions persist through ASCII DXF, binary DXF and DWG save and reopen cycles.

## Next electrical work

Project-wide tagging, contact pin inventories, visual attribute synchronization, terminal strips,
panel placement and circuit-aware copy behavior need their own implementations and
end-to-end tests.
