# CADCraft Electrical icon

The `cadcraft-electrical` branch uses an original green circuit-board app icon. It
retains the CADCraft icon family's rounded tile, drafting grid, heavy dark outlines,
ivory shapes and orange accent, replacing the drafting tools with an integrated circuit,
traces and terminal pads.

## Provenance

- Created on 2026-10-24 with the built-in `image_gen` tool, using the repository's
  original `assets/app-icon/cadcraft-1024.png` at upstream revision `41d9b5e` as a
  style reference. No external proprietary artwork was used.
- The generated square PNG was normalized from 1,254 pixels to the committed
  1,024-pixel master with ImageMagick's Lanczos resize. Alpha transparency is retained.
- Author: CADCraft Electrical contributors (AI-assisted). Licence: MIT OR Apache-2.0,
  matching the repository's open-source app assets. See [ATTRIBUTION.md](../ATTRIBUTION.md).

## Committed assets

- `assets/app-icon/cadcraft-1024.png`: primary master.
- `assets/app-icon/cadcraft-64.png`, `cadcraft-256.png`: desktop variants.
- `assets/app-icon/hicolor/`: Linux sizes from 16 through 512 pixels.
- `assets/app-icon/cadcraft.ico`: nine PNG-backed Windows sizes from 16 through 256 pixels,
  packed with the existing `cargo xtask ico` command.
- `assets/app-icon/cadcraft-macos-512.png`: macOS variant with the existing 48-pixel
  margin on a 512-pixel canvas. The corresponding ICNS uses master-derived PNG
  representations through 1,024 pixels, including Retina sizes.
- `packaging/macos/dmg/background.svg` and `background.tiff`: the existing installer
  layout with the new linked icon and matching green accent. Text outlines and Finder
  layout positions are retained. The TIFF has 1x/2x pages, a 16-colour palette,
  Deflate compression, 72/144 dpi and the previous sRGB colour profile.

Existing filenames are retained so the desktop window, Windows resources, macOS
bundle, Linux packages and README consume the new icon without changes to application
identifiers or executable names. Other branches retain their previous artwork.

`packaging/icons.sh` regenerates platform variants on macOS from the committed
masters. This change was prepared on Linux using ImageMagick for packaging resizes,
the existing ICO packer and a PNG-backed ICNS container. The DMG SVG was rendered
with `resvg --skip-system-fonts` at widths 660 and 1320, then packaged with ImageMagick;
its Finder layout file did not need regeneration.

## Initial generation prompt

Tool: built-in `image_gen`; transparent background requested. Input image 1 was the
previous repository app icon, provided only as a style reference.

```text
Use case: logo-brand. Asset type: production desktop application icon for CADCraft Electrical, square 1024x1024 PNG with transparent outer corners. Input image 1 is ONLY an original CADCraft style reference, not a subject to retain. Create a new original sibling icon that matches its flat technical drafting aesthetic: a large rounded-square tile filling the image, very bold near-black outlines, warm ivory shapes, a fine near-black drafting grid, and a tiny orange accent. Change the tile background from cyan to a rich strong engineering red (#D63D3D). Replace the drafting triangle and compass completely with a clear, uncluttered circuit-board design: a central warm-ivory integrated-circuit package with thick black outline, four distinct black pins on each side, and a few thick right-angle black PCB traces connecting to large circular ivory terminal pads; one small terminal pad can be orange. Keep the circuit composition centered with generous separation between tracks and nodes and enough padding from the edge to read at 32 pixels. Make the primary circuit silhouette as bold and iconic as the drafting triangle in the reference. Keep the fine drafting grid subtle behind the circuit. Flat solid colors, crisp clean edges, no gradients, no shadows, no texture, no perspective, no 3D. No text, letters, wordmarks, trademarks, border captions, extra symbols, or unrelated artwork. Transparent pixels ONLY outside the rounded red tile; the red tile itself is opaque. A single complete app icon, no mockups or multiple variants.
```

## Final background edit prompt

The user subsequently requested a green background. The built-in image tool edited
the red master using this prompt; all committed variants use the green result:

```text
Use case: precise-object-edit. Edit this existing CADCraft Electrical app icon. Change ONLY the red rounded-square tile background to a rich circuit-board green (#219653). Preserve exactly the existing circuit design, integrated-circuit package, all pin counts, terminal pads, black traces and fine drafting grid, ivory fills, single orange terminal accent, geometry, proportions, rounded tile shape, framing and transparent outer corners. Keep every non-background element unchanged. The final result is the same app icon with a green background instead of red. No lettering, new symbols, added effects or added objects. Preserve transparent pixels outside the tile and preserve the circuit artwork's sharp outlines. Output a single square icon.
```
