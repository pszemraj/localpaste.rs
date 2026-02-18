//! Theme constants and one-time style application for the egui app.

use super::LocalPasteApp;
use eframe::egui::{
    self, style::WidgetVisuals, Color32, CornerRadius, FontData, FontDefinitions, FontFamily,
    FontId, Margin, Stroke, TextStyle, Visuals,
};
use tracing::warn;

/// Primary app background color.
pub(super) const COLOR_BG_PRIMARY: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);
/// Secondary panel background color.
pub(super) const COLOR_BG_SECONDARY: Color32 = Color32::from_rgb(0x16, 0x1b, 0x22);
/// Elevated widget background color.
pub(super) const COLOR_BG_TERTIARY: Color32 = Color32::from_rgb(0x21, 0x26, 0x29);
/// Primary foreground text color.
pub(super) const COLOR_TEXT_PRIMARY: Color32 = Color32::from_rgb(0xc9, 0xd1, 0xd9);
/// Secondary text color for supporting UI copy.
pub(super) const COLOR_TEXT_SECONDARY: Color32 = Color32::from_rgb(0x8b, 0x94, 0x9e);
/// Muted text color for low-priority labels.
pub(super) const COLOR_TEXT_MUTED: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);
/// Primary accent color for active controls.
pub(super) const COLOR_ACCENT: Color32 = Color32::from_rgb(0xE5, 0x70, 0x00);
/// Hover accent color for interactive controls.
pub(super) const COLOR_ACCENT_HOVER: Color32 = Color32::from_rgb(0xCE, 0x42, 0x2B);
/// Selection outline color.
pub(super) const COLOR_SELECTION_STROKE: Color32 = Color32::from_rgb(0x3B, 0x82, 0xF6);
/// Selection fill color as RGBA bytes.
pub(super) const COLOR_SELECTION_FILL_RGBA: [u8; 4] = [0x3B, 0x82, 0xF6, 0x55];
/// Border/stroke color for panels and widgets.
pub(super) const COLOR_BORDER: Color32 = Color32::from_rgb(0x30, 0x36, 0x3d);
/// Font family key for bundled 0xProto typeface.
pub(super) const FONT_0XPROTO: &str = "0xProto";
/// Custom font family key used by the editor text style.
pub(super) const EDITOR_FONT_FAMILY: &str = "Editor";
/// Text style name used by editor rendering paths.
pub(super) const EDITOR_TEXT_STYLE: &str = "Editor";
const FONT_0XPROTO_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/0xProto/0xProto-Regular-NL.ttf"
));

fn selection_fill_color() -> Color32 {
    Color32::from_rgba_unmultiplied(
        COLOR_SELECTION_FILL_RGBA[0],
        COLOR_SELECTION_FILL_RGBA[1],
        COLOR_SELECTION_FILL_RGBA[2],
        COLOR_SELECTION_FILL_RGBA[3],
    )
}

impl LocalPasteApp {
    /// Applies LocalPaste UI theme/fonts once per app lifetime.
    pub(super) fn ensure_style(&mut self, ctx: &egui::Context) {
        if self.style_applied {
            return;
        }

        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            FONT_0XPROTO.to_string(),
            FontData::from_static(FONT_0XPROTO_BYTES).into(),
        );
        let editor_family = FontFamily::Name(EDITOR_FONT_FAMILY.into());
        // Virtual editor wrap/cursor math assumes fixed-width glyphs. Keep fallback chain
        // aligned with the monospace family to avoid proportional-font drift.
        let mut editor_fallback_fonts = vec![FONT_0XPROTO.to_string()];
        if let Some(monospace_fonts) = fonts.families.get(&FontFamily::Monospace).cloned() {
            for font_name in monospace_fonts {
                if !editor_fallback_fonts
                    .iter()
                    .any(|existing| existing == &font_name)
                {
                    editor_fallback_fonts.push(font_name);
                }
            }
        }
        fonts
            .families
            .insert(editor_family.clone(), editor_fallback_fonts);
        let editor_font_ready = fonts.font_data.contains_key(FONT_0XPROTO);
        if !editor_font_ready {
            warn!("0xProto font missing; falling back to monospace in editor");
        }
        ctx.set_fonts(fonts);

        let mut style = (*ctx.style()).clone();
        style.visuals = Visuals::dark();
        style.visuals.override_text_color = Some(COLOR_TEXT_PRIMARY);
        style.visuals.window_fill = COLOR_BG_PRIMARY;
        style.visuals.panel_fill = COLOR_BG_SECONDARY;
        style.visuals.extreme_bg_color = COLOR_BG_PRIMARY;
        style.visuals.faint_bg_color = COLOR_BG_TERTIARY;
        style.visuals.window_stroke = Stroke::new(1.0, COLOR_BORDER);
        style.visuals.hyperlink_color = COLOR_ACCENT;
        style.visuals.selection.bg_fill = selection_fill_color();
        style.visuals.selection.stroke = Stroke::new(1.0, COLOR_SELECTION_STROKE);
        style.visuals.text_edit_bg_color = Some(COLOR_BG_TERTIARY);

        style.visuals.widgets.noninteractive = WidgetVisuals {
            bg_fill: COLOR_BG_SECONDARY,
            weak_bg_fill: COLOR_BG_SECONDARY,
            bg_stroke: Stroke::new(1.0, COLOR_BORDER),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, COLOR_TEXT_SECONDARY),
            expansion: 0.0,
        };
        style.visuals.widgets.inactive = WidgetVisuals {
            bg_fill: COLOR_BG_TERTIARY,
            weak_bg_fill: COLOR_BG_TERTIARY,
            bg_stroke: Stroke::new(1.0, COLOR_BORDER),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, COLOR_TEXT_PRIMARY),
            expansion: 0.0,
        };
        style.visuals.widgets.hovered = WidgetVisuals {
            bg_fill: COLOR_ACCENT_HOVER,
            weak_bg_fill: COLOR_ACCENT_HOVER,
            bg_stroke: Stroke::new(1.0, COLOR_ACCENT_HOVER),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, Color32::WHITE),
            expansion: 0.5,
        };
        style.visuals.widgets.active = WidgetVisuals {
            bg_fill: COLOR_ACCENT,
            weak_bg_fill: COLOR_ACCENT,
            bg_stroke: Stroke::new(1.0, COLOR_ACCENT),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, Color32::WHITE),
            expansion: 0.5,
        };
        style.visuals.widgets.open = WidgetVisuals {
            bg_fill: COLOR_ACCENT,
            weak_bg_fill: COLOR_ACCENT,
            bg_stroke: Stroke::new(1.0, COLOR_ACCENT),
            corner_radius: CornerRadius::same(6),
            fg_stroke: Stroke::new(1.0, Color32::WHITE),
            expansion: 0.0,
        };

        style.spacing.window_margin = Margin::same(12);
        style.spacing.button_padding = egui::vec2(14.0, 8.0);
        style.spacing.item_spacing = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 34.0;
        style.spacing.text_edit_width = 280.0;
        style.spacing.indent = 18.0;
        style.spacing.menu_margin = Margin::same(8);
        style.spacing.combo_width = 220.0;

        style.text_styles.insert(
            TextStyle::Heading,
            FontId::new(24.0, FontFamily::Proportional),
        );
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(16.0, FontFamily::Proportional));
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(15.0, FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Name(EDITOR_TEXT_STYLE.into()),
            FontId::new(
                15.0,
                if editor_font_ready {
                    FontFamily::Name(EDITOR_FONT_FAMILY.into())
                } else {
                    FontFamily::Monospace
                },
            ),
        );
        style.text_styles.insert(
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        );

        ctx.set_style(style);
        self.style_applied = true;
    }
}
