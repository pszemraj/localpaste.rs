//! Window min/max bounds enforcement helpers for native egui viewport sizing.

use eframe::egui;

const MAX_TEXTURE_DIMENSION_FALLBACK_PX: f32 = 8192.0;

fn viewport_inner_size(ctx: &egui::Context, fallback: egui::Vec2) -> egui::Vec2 {
    ctx.input(|input| {
        input
            .viewport()
            .inner_rect
            .map(|rect| rect.size())
            .unwrap_or(fallback)
    })
}

fn max_texture_dimension_px(frame: &eframe::Frame) -> f32 {
    frame
        .wgpu_render_state()
        .map(|state| state.device.limits().max_texture_dimension_2d as f32)
        .unwrap_or(MAX_TEXTURE_DIMENSION_FALLBACK_PX)
}

fn clamped_size_for_texture_limit(
    current_points: egui::Vec2,
    pixels_per_point: f32,
    min_points: egui::Vec2,
    max_dimension_px: f32,
) -> Option<egui::Vec2> {
    if !pixels_per_point.is_finite()
        || pixels_per_point <= 0.0
        || !max_dimension_px.is_finite()
        || max_dimension_px <= 0.0
    {
        return None;
    }

    let current_px = current_points * pixels_per_point;
    if current_px.x <= max_dimension_px && current_px.y <= max_dimension_px {
        return None;
    }

    let max_points = egui::vec2(
        (max_dimension_px / pixels_per_point).max(1.0),
        (max_dimension_px / pixels_per_point).max(1.0),
    );
    let min_clamp = egui::vec2(
        min_points.x.min(max_points.x),
        min_points.y.min(max_points.y),
    );
    let clamped = egui::vec2(
        current_points.x.clamp(min_clamp.x, max_points.x),
        current_points.y.clamp(min_clamp.y, max_points.y),
    );
    Some(clamped)
}

fn clamped_size_for_window_limit(
    current_points: egui::Vec2,
    min_points: egui::Vec2,
    max_points: egui::Vec2,
) -> Option<egui::Vec2> {
    if !max_points.x.is_finite()
        || !max_points.y.is_finite()
        || max_points.x <= 0.0
        || max_points.y <= 0.0
    {
        return None;
    }
    if current_points.x <= max_points.x && current_points.y <= max_points.y {
        return None;
    }

    let max_points = egui::vec2(max_points.x.max(1.0), max_points.y.max(1.0));
    let min_clamp = egui::vec2(
        min_points.x.min(max_points.x),
        min_points.y.min(max_points.y),
    );
    Some(egui::vec2(
        current_points.x.clamp(min_clamp.x, max_points.x),
        current_points.y.clamp(min_clamp.y, max_points.y),
    ))
}

fn clamped_size_for_window_and_texture_limits(
    current_points: egui::Vec2,
    pixels_per_point: f32,
    min_points: egui::Vec2,
    max_points: egui::Vec2,
    max_dimension_px: f32,
) -> Option<egui::Vec2> {
    let window_clamped_points =
        clamped_size_for_window_limit(current_points, min_points, max_points);
    let texture_check_points = window_clamped_points.unwrap_or(current_points);

    clamped_size_for_texture_limit(
        texture_check_points,
        pixels_per_point,
        min_points,
        max_dimension_px,
    )
    .or(window_clamped_points)
}

/// Enforces viewport minimum size, monitor-sane maximum size, and GPU texture-size bounds.
///
/// # Arguments
/// - `ctx`: Active egui context.
/// - `frame`: Current eframe frame handle (for wgpu limits).
/// - `window_checked`: One-shot initialization guard for min-size clamp.
/// - `min_points`: Minimum window size in logical points.
/// - `max_points`: Maximum window size in logical points.
pub(super) fn enforce_window_bounds(
    ctx: &egui::Context,
    frame: &eframe::Frame,
    window_checked: &mut bool,
    min_points: egui::Vec2,
    max_points: egui::Vec2,
) {
    let current_points = viewport_inner_size(ctx, min_points);
    if !*window_checked {
        if current_points.x < min_points.x || current_points.y < min_points.y {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(min_points));
        }
        *window_checked = true;
    }

    let pixels_per_point = ctx.pixels_per_point().max(1.0);
    let max_dimension_px = max_texture_dimension_px(frame);
    if let Some(clamped_points) = clamped_size_for_window_and_texture_limits(
        current_points,
        pixels_per_point,
        min_points,
        max_points,
        max_dimension_px,
    ) {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(clamped_points));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clamped_size_for_texture_limit, clamped_size_for_window_and_texture_limits,
        clamped_size_for_window_limit,
    };
    use eframe::egui;

    fn assert_clamp_case(
        current: egui::Vec2,
        pixels_per_point: f32,
        min: egui::Vec2,
        max_dimension_px: f32,
        expected: Option<egui::Vec2>,
    ) {
        assert_eq!(
            clamped_size_for_texture_limit(current, pixels_per_point, min, max_dimension_px),
            expected
        );
    }

    #[test]
    fn no_clamp_when_within_texture_bounds() {
        let min = egui::vec2(900.0, 600.0);
        let cases = [
            (egui::vec2(1000.0, 700.0), 1.0, None),
            (
                egui::vec2(9360.0, 6166.0),
                1.0,
                Some(egui::vec2(8192.0, 6166.0)),
            ),
            (
                egui::vec2(5000.0, 5000.0),
                2.0,
                Some(egui::vec2(4096.0, 4096.0)),
            ),
        ];

        for (current, pixels_per_point, expected) in cases {
            assert_clamp_case(current, pixels_per_point, min, 8192.0, expected);
        }
    }

    #[test]
    fn clamps_against_window_bounds_before_texture_bounds() {
        let min = egui::vec2(900.0, 600.0);
        let max = egui::vec2(7680.0, 4320.0);
        assert_eq!(
            clamped_size_for_window_limit(egui::vec2(8000.0, 5120.0), min, max),
            Some(egui::vec2(7680.0, 4320.0))
        );
        assert_eq!(
            clamped_size_for_window_limit(egui::vec2(5120.0, 2880.0), min, max),
            None
        );
    }

    #[test]
    fn combined_clamp_does_not_relax_window_limit_to_texture_limit() {
        let min = egui::vec2(900.0, 600.0);
        let max = egui::vec2(7680.0, 4320.0);

        assert_eq!(
            clamped_size_for_window_and_texture_limits(
                egui::vec2(9000.0, 3000.0),
                1.0,
                min,
                max,
                8192.0,
            ),
            Some(egui::vec2(7680.0, 3000.0))
        );
    }

    #[test]
    fn combined_clamp_uses_texture_limit_when_it_is_stricter() {
        let min = egui::vec2(900.0, 600.0);
        let max = egui::vec2(7680.0, 4320.0);

        assert_eq!(
            clamped_size_for_window_and_texture_limits(
                egui::vec2(9000.0, 5000.0),
                2.0,
                min,
                max,
                8192.0,
            ),
            Some(egui::vec2(4096.0, 4096.0))
        );
    }
}
