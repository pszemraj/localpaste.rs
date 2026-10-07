# LocalPaste Desktop Palette

The desktop uses a dark palette defined in the theme module:
[`../../crates/localpaste_gui/src/app/style.rs`](../../crates/localpaste_gui/src/app/style.rs).

| Token                       | Hex         | Notes                                   |
| --------------------------- | ----------- | --------------------------------------- |
| `COLOR_BG_PRIMARY`          | `#0D1117`   | Window background                       |
| `COLOR_BG_SECONDARY`        | `#161B22`   | Panels, status bar                      |
| `COLOR_BG_TERTIARY`         | `#212629`   | Editor frames, inputs                   |
| `COLOR_TEXT_PRIMARY`        | `#C9D1D9`   | Body text                               |
| `COLOR_TEXT_SECONDARY`      | `#8B949E`   | Secondary text                          |
| `COLOR_TEXT_MUTED`          | `#6E7681`   | Labels / metadata                       |
| `COLOR_ACCENT_TEXT`         | `#D0843A`   | Accent text and links                   |
| `COLOR_ACCENT_SURFACE`      | `#B86724`   | Filled controls                         |
| `COLOR_ACCENT_SURFACE_HOVER` | `#C37431`  | Hovered controls                        |
| `COLOR_MODAL_CHROME`        | `#8A522A`   | Modal title bars                        |
| `COLOR_SELECTION_STROKE`    | `#3B82F6`   | Selection outline                       |
| `COLOR_SELECTION_FILL_RGBA` | `#3B82F655` | Selection fill RGBA tuple (`[r,g,b,a]`) |
| `COLOR_BORDER`              | `#30363D`   | Divider strokes                         |

## Editor Font

The native editor uses the bundled `0xProto-Regular-NL.ttf`.

Related files:

- Font file: [`../../assets/fonts/0xProto/0xProto-Regular-NL.ttf`](../../assets/fonts/0xProto/0xProto-Regular-NL.ttf)
- Font license: [`../../assets/fonts/0xProto/LICENSE`](../../assets/fonts/0xProto/LICENSE)
