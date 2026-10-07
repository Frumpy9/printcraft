//! Platform-independent middle-button scrolling for the document viewport.

use egui::{Context, CursorIcon, Event, Key, PointerButton, Pos2, Stroke, Vec2, vec2};

const DEAD_ZONE: f32 = 12.0;
const SPEED_PER_POINT: f32 = 16.0;
const MAX_SPEED: f32 = 3200.0;

#[derive(Default)]
pub(crate) struct AutoScroll {
    anchor: Option<Pos2>,
    organize: bool,
    /// Own a cancelling click through its release, so it cannot also edit page content.
    cancel_button: Option<PointerButton>,
    block_input: bool,
}

impl AutoScroll {
    pub(crate) fn active(&self) -> bool {
        self.anchor.is_some()
    }

    pub(crate) fn cancel(&mut self) {
        self.anchor = None;
        self.cancel_button = None;
        self.block_input = false;
    }

    pub(crate) fn blocks_input(&self) -> bool {
        self.block_input
    }

    /// Run before the document's widgets. Starting is restricted to the unobstructed viewport;
    /// once started, moving outside that viewport still controls the speed.
    pub(crate) fn update(&mut self, ui: &egui::Ui, viewport: egui::Rect, organize: bool) -> Vec2 {
        let ctx = ui.ctx();
        let (pointer, middle_press, middle_down, middle_released, cancel, cancel_button, dt) = ctx.input(|i| {
            let cancel_button = [PointerButton::Primary, PointerButton::Secondary, PointerButton::Extra1, PointerButton::Extra2]
                .into_iter()
                .find(|button| i.pointer.button_pressed(*button));
            (
                i.pointer.hover_pos(),
                // Read the press event itself: later movement in this frame must not move the anchor.
                i.events.iter().find_map(|event| match event {
                    Event::PointerButton { pos, button: PointerButton::Middle, pressed: true, .. } if pos.is_finite() => Some(*pos),
                    _ => None,
                }),
                i.pointer.button_down(PointerButton::Middle),
                i.pointer.button_released(PointerButton::Middle),
                !i.focused
                    || i.key_pressed(Key::Escape)
                    || cancel_button.is_some()
                    || i.events.iter().any(|e| matches!(e, Event::MouseWheel { .. } | Event::Zoom(_))),
                cancel_button,
                i.stable_dt,
            )
        });
        self.block_input = self.active() || self.cancel_button.is_some() || middle_press.is_some() || middle_down || middle_released;
        if let Some(button) = self.cancel_button {
            if !ctx.input(|i| i.pointer.button_down(button)) {
                self.cancel_button = None;
            }
            return Vec2::ZERO;
        }
        if self.active() && (cancel || pointer.is_none() || self.organize != organize || ctx.egui_wants_keyboard_input()) {
            self.cancel();
            self.block_input = true;
            self.cancel_button = cancel_button;
            return Vec2::ZERO;
        }
        if let Some(pressed_at) = middle_press {
            if self.active() {
                self.cancel();
                self.block_input = true;
                self.cancel_button = Some(PointerButton::Middle);
                return Vec2::ZERO;
            } else if !cancel
                && !ctx.egui_wants_keyboard_input()
                && viewport.intersect(ui.clip_rect()).contains(pressed_at)
                && ctx.layer_id_at(pressed_at) == Some(ui.layer_id())
            {
                self.anchor = Some(pressed_at);
                self.organize = organize;
            }
        }
        let (Some(anchor), Some(pointer)) = (self.anchor, pointer) else { return Vec2::ZERO };
        let displacement = pointer.y - anchor.y;
        // Cap the elapsed time too: returning from an idle/hidden window must never jump pages.
        let delta = scroll_delta(displacement, dt);
        if delta != 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        vec2(0.0, delta)
    }

    /// Draw an original geometric marker at the activation point, above page content.
    pub(crate) fn paint(&self, ui: &egui::Ui, viewport: egui::Rect) {
        let Some(anchor) = self.anchor else { return };
        let painter = ui.painter().with_clip_rect(viewport);
        let ink = ui.visuals().text_color();
        painter.circle(anchor, 13.0, ui.visuals().window_fill(), Stroke::new(1.0, ink));
        painter.circle_filled(anchor, 2.0, ink);
        for direction in [-1.0, 1.0] {
            painter.add(egui::Shape::convex_polygon(
                vec![anchor + vec2(-4.0, direction * 6.0), anchor + vec2(4.0, direction * 6.0), anchor + vec2(0.0, direction * 10.0)],
                ink,
                Stroke::NONE,
            ));
        }
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }

    /// Escape belongs to autoscroll first, leaving selection/find/full-screen intact.
    pub(crate) fn escape(&mut self, ctx: &Context) -> bool {
        if self.active() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.cancel();
            true
        } else {
            false
        }
    }
}

fn scroll_delta(displacement: f32, dt: f32) -> f32 {
    if !displacement.is_finite() || !dt.is_finite() {
        return 0.0;
    }
    let speed = ((displacement.abs() - DEAD_ZONE).max(0.0) * SPEED_PER_POINT).min(MAX_SPEED);
    // egui's delta moves content, the opposite of the scroll offset.
    -displacement.signum() * speed * dt.clamp(0.0, 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_has_a_dead_zone_is_symmetric_and_is_bounded() {
        for y in [-12.0, -1.0, 0.0, 1.0, 12.0] {
            assert_eq!(scroll_delta(y, 0.016), 0.0);
        }
        assert!(scroll_delta(30.0, 0.016) < 0.0);
        assert!(scroll_delta(100.0, 1.0 / 60.0).abs() > 20.0, "a moderate displacement scrolls at least 1200 points per second");
        for (near, far) in [(13.0, 30.0), (30.0, 100.0), (100.0, 200.0)] {
            assert!(scroll_delta(far, 0.016).abs() > scroll_delta(near, 0.016).abs());
        }
        assert_eq!(scroll_delta(30.0, 0.016), -scroll_delta(-30.0, 0.016));
        assert_eq!(scroll_delta(1000.0, 0.016), -MAX_SPEED * 0.016);
        assert_eq!(scroll_delta(1000.0, 10.0), -MAX_SPEED * 0.05);
        assert_eq!(scroll_delta(f32::NAN, 0.016), 0.0);
        assert_eq!(scroll_delta(1000.0, f32::INFINITY), 0.0);
    }
}
