# LocalPaste Desktop Palette

The native egui interface mirrors the tones used by the previous web UI.
These canonical values are applied inside `legacy/gui/mod.rs` and should be reused for future widgets or theming tweaks.

| Token                  | Hex       | Notes                      |
| ---------------------- | --------- | -------------------------- |
| `COLOR_BG_PRIMARY`     | `#0D1117` | Window background          |
| `COLOR_BG_SECONDARY`   | `#161B22` | Panels, status bar         |
| `COLOR_BG_TERTIARY`    | `#212629` | Editor frames, inputs      |
| `COLOR_TEXT_PRIMARY`   | `#C9D1D9` | Body text                  |
| `COLOR_TEXT_SECONDARY` | `#8B949E` | Secondary text             |
| `COLOR_TEXT_MUTED`     | `#6E7681` | Labels / metadata          |
| `COLOR_ACCENT`         | `#E57000` | Primary actions, selection |
| `COLOR_ACCENT_HOVER`   | `#CE422B` | Accent hover state         |
| `COLOR_DANGER`         | `#F85149` | Destructive actions        |
| `COLOR_BORDER`         | `#30363D` | Divider strokes            |

These values are applied in `crates/localpaste_gui/src/app.rs` and preserved in `legacy/gui/mod.rs`
for reference while the legacy GUI remains in the repo.

## Editor Font

The native editor uses the 0xProto font (Regular NL) under the SIL Open Font License.
Font files and license live at `assets/fonts/0xProto`.
