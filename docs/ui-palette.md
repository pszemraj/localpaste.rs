# LocalPaste Desktop Palette

The native egui interface mirrors the tones used by the previous web UI.
These canonical values are applied inside `src/gui/mod.rs` and should be reused for future widgets or theming tweaks.

| Token                  | Hex       | Notes                      |
| ---------------------- | --------- | -------------------------- |
| `COLOR_BG_PRIMARY`     | `#0D1117` | Window background          |
| `COLOR_BG_SECONDARY`   | `#161B22` | Panels, status bar         |
| `COLOR_BG_TERTIARY`    | `#21262D` | Editor frames, inputs      |
| `COLOR_TEXT_PRIMARY`   | `#C9D1D9` | Body text                  |
| `COLOR_TEXT_SECONDARY` | `#8B949E` | Secondary text             |
| `COLOR_TEXT_MUTED`     | `#6E7681` | Labels / metadata          |
| `COLOR_ACCENT`         | `#E57000` | Primary actions, selection |
| `COLOR_ACCENT_HOVER`   | `#CE422B` | Accent hover state         |
| `COLOR_DANGER`         | `#F85149` | Destructive actions        |
| `COLOR_BORDER`         | `#30363D` | Divider strokes            |

The screenshot at `assets/ui.png` shows the original layout these colors came from.
