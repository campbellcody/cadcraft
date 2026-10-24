# Drawing packages (ETRANSMIT)

Use **File ▸ eTransmit...** or type `ETRANSMIT`, enter a new ZIP file name,
then enter a source file or folder you approve for including dependencies. Enter
at the source prompt approves none; it never grants access to the drawing folder
automatically. Extract the entire ZIP before opening its root drawing. Keep its
`drawings/` and `assets/` folders beside that drawing.

The package contains a snapshot of the current drawing, including unsaved drawing
edits, and these dependencies:

- External reference drawings, including nested, overlay and unloaded references.
- Valid PNG/JPEG image files referenced by model space, paper space or ordinary blocks.
- UTF-8 CSV files linked to tables.

Dependencies are included only when the caller explicitly authorizes their
resolved source file or folder. A drawing's own paths are not export authorization.
By default only the current drawing snapshot is included; unapproved references
are reported in `missing`. Folder approval covers its canonical descendants,
including nested xrefs' dependencies; a symlink escaping that folder needs separate
approval. Exact-file approval does not approve sibling files.

Copies of drawings use relative paths into the package. Original files, the open
drawing, its saved/dirty state and undo history are unchanged. References to the
same source file are packaged once; different files with the same name get unique
names. Circular xrefs reuse their existing package entry. Xrefs use their saved
on-disk content, including when another open drawing has unsaved edits.

The root drawing uses its source DXF/DWG name by default; an untitled drawing uses
`Drawing.dxf`. A template or other imported format defaults to a DXF snapshot.
Names are sanitized for portability; long stems are shortened while preserving
valid file extensions. During BEDIT or REFEDIT, the snapshot uses the committed
host drawing, as ordinary saves do; save the block edit first to include it.

## Scripts, CLI, control channel and MCP

All entry points use the engine command; JSON calls never open a dialog:

```json
{"command":"etransmit","params":{"path":"/tmp/panel-package.zip","allow":["/project/panel"],"strict":true}}
```

```sh
cadcraft-cli run panel.dxf --cmd 'etransmit {"path":"panel-package.zip","allow":["."],"strict":true}'
```

`allow` is an optional array of up to 128 existing files or directories chosen by
the caller, resolved relative to the process working directory. An omitted or
empty array grants no dependency exports. Approve only intended sources: approving
a broad folder authorizes eligible referenced files throughout that folder.

The optional `drawing` parameter chooses the root drawing name and format, for
example `"drawing":"panel.dxf"`. Only DXF and DWG root names are accepted. DXF
can preserve objects the DWG writer does not support. Drawings pass through the
normal format hooks; packaging does not improve the existing DWG writer's fidelity.

The destination must be a new `.zip` file in an existing folder. Completed packages
are published without replacing existing files. A failed operation removes its
staging file. On Unix the completed archive gets ordinary-save permissions
(`0666` filtered through the current umask), while staging ZIP data stays private.
A restrictive umask is respected. ZIP entries are stored without compression.

The result and the ZIP's `manifest.json` contain:

| Field | Meaning |
|---|---|
| `schema` | Manifest version, currently 1 |
| `drawing` | Root drawing name inside the ZIP |
| `files` | Payload entries: package path, kind and byte count (excludes the manifest itself) |
| `missing` | Dependencies not included: owning package drawing, kind, stored reference and reason |
| `warnings` | Drawing read warnings and known unsupported DWG entity losses |
| `complete` | All recognized dependencies collected and no drawing warnings |
| `limitations` | Types of support files and references this command does not collect |

The command result also contains the destination `path`. With `strict:false`
(default), missing files produce a partial package and a command-line summary.
Missing paths remain unchanged in the drawing copy and are listed in the manifest
(extremely long reference names are truncated in diagnostics).
Up to five diagnostic details are also shown at the command line.
With `strict:true`, missing, unapproved or invalid dependencies and drawing warnings
fail the command and no package is published. `complete:true` describes the supported dependency types;
it does not guarantee compatibility or completeness for unmodeled references.

Stored dependency paths follow the existing document-path policy before export
authorization is checked. Source authorization applies to xrefs, images and CSV
links, including files named by nested xrefs. The current drawing snapshot is the
explicitly selected root and needs no allowlist entry. Relative paths
resolve beside their owning source drawing; a missing path also gets a basename
lookup beside that drawing, as xref loading does. Refused remote paths are reported
without following them. Relative references in untitled drawings have no source
folder and are reported missing; save the drawing first or use absolute references.

Image sources must have PNG/JPEG content matching their extension and decode
within 16,384 pixels per dimension and 64 MiB of decoder allocation. CSV sources
must have a `.csv` extension and contain UTF-8 text without binary control
characters. Format checks provide additional validation; they never replace
source authorization. Other image formats are reported as unsupported.

## Limits and remaining work

Packages currently require the native app or CLI. The browser command reports
that local dependency packaging is unavailable. Fonts, plot configuration files,
application support files, and references hidden in unmodeled foreign objects are
not collected. No font or other support asset is supplied by CADCraft.

Work is bounded to 1,000 unique dependency identities, 10,000 reference occurrences,
1,000,000 scanned objects/collections and `MAX_BLOCK_DEPTH` xref levels. Input and
output each have a 64 MiB per-file and 256 MiB total byte budget. Oversized or unreadable
dependencies are reported missing; scan or output limits abort the operation.
