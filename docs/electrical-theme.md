# CADCraft Electrical palette

The `cadcraft-electrical` branch uses green branding throughout the application:
forest-green chrome in Dark mode, light sage chrome in Light mode, and a green
circuit-board app icon. System mode follows the operating system's Light or Dark
choice. This is the default appearance of the Electrical branch.

## Palette and source of truth

[`crates/ui-egui/src/theme.rs`](../crates/ui-egui/src/theme.rs) owns the interface
colours in `theme::Tokens`. These are representative values; use the token for a
control's role rather than copying a hex value into widget code.

| Role | Dark | Light |
|---|---|---|
| Chrome (`chrome`) | `#394B42` | `#E4EBE6` |
| Panels (`panel`) | `#3B4B42` | `#EDF3EF` |
| Active accents (`accent`) | `#219653` | `#197C43` |
| Icon accents (`icon_accent`) | `#54C98B` | `#187842` |
| Selected text/list background (`select_bg`) | `#287A4B` | `#B8DFC7` |
| Model background (`canvas`) | `#222C26` | `#222C26` |

Green accents cover toolbar geometry, active toggles, hyperlinks, command keywords,
selection highlights, grips and constraint markers. The ViewCube uses the dark
palette in both interface modes because it sits over the drawing area. The model
background and grid also keep their drawing-area palette when the interface changes.
The app icon and macOS installer use `#219653`; asset provenance and regeneration
instructions are in [electrical-icon.md](electrical-icon.md).

## Guidance for contributors and agents

- Read colours from `theme::Tokens`; extend the shared tokens when a new semantic
  role needs a colour. Apply egui widget styling through the shared theme setup.
- Keep both Light and Dark modes usable, including hovered, selected, focused and
  disabled controls. Inspect rendered UI in both modes when changing the palette.
- Preserve user-selected model background, paper background and selection colours.
  Theme changes must not rewrite document entity colours, layer colours, colour
  index values, imported drawing data or exported drawing colours.
- Preserve functional colour meanings: warning colours, red hot grips, orange picked
  points, comparison colours, and distinct X/Y axes. Selection windows retain their
  directional solid/dashed behaviour; their area-fill colour preferences still apply.
- Keep the icon, packaging and interface documentation consistent with green
  Electrical branding. Keep historical generation prompts intact for provenance,
  and identify older upstream screenshots when their chrome differs.

Validation on 2026-10-24: native screenshots of both themes were inspected, and
`cargo xtask ci` passed all seven checks for the palette implementation.
