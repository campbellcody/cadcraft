# CADCraft Electrical integration branch

`cadcraft-electrical` is an integration branch for evaluating electrical workflows on
top of the normal CADCraft application. It retains the upstream drafting application
and combines these feature heads:

| Input | Revision | Role |
|---|---|---|
| Official `storytold/cadcraft` `main` | `41d9b5e` (fetched 2026-10-24) | Base drafting, file I/O, layouts, plotting, automation and platforms |
| `feat/electrical-components` | `09e4271` | Device identity, shared metadata, representation associations and physical-device BOM |
| `feat/etransmit-packages` | `bddbcae` | Native drawing ZIP packages with explicitly authorized dependencies |

The feature heads are merged, preserving their histories. The electrical feature also
includes a separate mechanical Clippy cleanup commit. No upstream changes are replaced
by an old feature snapshot. Both the component and packaging commands remain available
through the normal engine registry.

The Electrical branch uses a green circuit icon and matching interface palettes: forest
green in dark mode and light sage in light mode. Tool accents, selection controls,
command prompts and navigation overlays share the theme tokens. User-selected canvas
and selection colours still take precedence; drawing entity and layer colours retain
their document values.

## What to evaluate

| Workflow | Implemented scope | Remaining limits |
|---|---|---|
| Ordinary CAD drafting | Existing upstream functionality | Upstream's documented bugs and parity gaps still apply |
| Electrical component metadata | Stable physical-device identity, scoped tags, manufacturer/catalog/description fields, coil/contact/footprint roles | Metadata does not automatically rewrite visible block attributes; no electrical palette |
| Purchasing BOM | Count distinct physical devices; return JSON or CSV | No catalog database, assemblies, accessories or purchasing approval |
| Electrical persistence | CADCraft metadata through ASCII/binary DXF and DWG, tested synthetically | No claim of AutoCAD Electrical recognition or lossless general DWG exchange |
| Portable handoff | Root snapshot, saved xrefs, supported images and linked CSV files in native ZIP packages | Explicit source authorization required; fonts, plot configs and application support files excluded |
| Combined handoff | Preserve unsaved root metadata, child drawing identities and linked BOM CSV when packaging and relocating | An unlinked BOM export is not automatically collected; xrefs use saved on-disk data |
| Intelligent electrical drafting | Not implemented | Pins, wire nets, wire numbers, source/destination signals and contact cross-references |
| Electrical projects | Not implemented | Cross-drawing device indexing, project-wide retagging, title-block updates and recovery of project-wide edits |
| Production EE documentation | Not implemented as an end-to-end workflow | Terminal strips, cables, PLC generation, part catalogs, linked panel placement and electrical audit |

This branch is a base-CAD application with early electrical metadata/reporting and
handoff capabilities. It is not yet a replacement for AutoCAD Electrical. Existing
base-AutoCAD percentages are not Electrical readiness measurements.

## Reproduce the integration check

```sh
cargo test -p cadcraft-engine --test electrical --test etransmit
cargo test -p cadcraft-engine --test etransmit electrical_devices_and_linked_bom_survive_package_relocation -- --exact
cargo xtask ci
```

Validation on 2026-10-24: all 14 electrical and 18 packaging integration tests passed,
and all seven CI steps passed (fmt, Clippy, workspace tests/doctests, translations,
assets, dependency layers and WASM).

The combined regression generates original synthetic drawings. For both DXF and DWG
root formats, it creates a relay coil and two contacts representing one physical device,
adds an electrical child xref, changes root metadata without saving, exports and links a
BOM CSV, then creates a strict package with the source folder explicitly authorized.
It checks that packaging leaves the open drawing, source file, dirty state and undo/redo
unchanged. After extraction into a different folder and removal of all original source
files, it checks the device identities, three representations per relay, edited properties,
child reference, CSV bytes and regenerated BOM.

The child and root drawings keep separate device inventories. Preserving both inventories
does not implement a project-wide BOM or a cross-drawing connectivity graph.

## Try it in the application

Run the usual native application, or use the CLI:

```sh
cargo run --release -p cadcraft -- --sample
cargo run -p cadcraft-cli -- commands electrical
cargo run -p cadcraft-cli -- commands etransmit
```

Follow [electrical component examples](electrical.md) to associate existing block
references, inspect the device list and generate a BOM. Use [drawing packages](etransmit.md)
for package parameters and dependency authorization. To include a generated BOM CSV,
link it to a drawing table or distribute it explicitly alongside the package; approving
a folder does not include every file in it.

For a useful practitioner pilot, create a small machine-control drawing and change one
relay's part assignment, copy another relay as a distinct device, regenerate its BOM,
package it and reopen the relocated result. Record where manual work is needed for
tags, wiring, references, terminals and reports. Those missing workflows determine
the next electrical feature work; a successful ZIP or BOM alone does not establish
engineering-documentation readiness.
