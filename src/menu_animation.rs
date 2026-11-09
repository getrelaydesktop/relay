use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use slint::{ComponentHandle, Timer, TimerMode};

use crate::Desktop;

// Telegram 7.4.1: LayerStackWidget::setMainMenuShown uses boxDuration
// (200 ms) and anim::easeOutCirc: sqrt(1 - (t - 1) * (t - 1)).
pub fn install(ui: &Desktop) {
    let timer = Rc::new(Timer::default());
    let weak_ui = ui.as_weak();
    ui.on_animate_menu(move |opened| {
        let Some(ui) = weak_ui.upgrade() else { return };
        let from = ui.get_menu_progress();
        let target = if opened { 1.0 } else { 0.0 };
        let started = Instant::now();
        let weak_ui = ui.as_weak();
        let weak_timer = Rc::downgrade(&timer);

        // Restarting the timer replaces the previous animation, starting
        // from its current position when the user reverses direction.
        timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
            let Some(ui) = weak_ui.upgrade() else {
                if let Some(timer) = weak_timer.upgrade() {
                    timer.stop();
                }
                return;
            };
            let t = (started.elapsed().as_secs_f64() / 0.2).min(1.0);
            let progress = (1.0 - (t - 1.0) * (t - 1.0)).max(0.0).sqrt();
            ui.set_menu_progress(from + (target - from) * progress as f32);
            if t >= 1.0 {
                ui.set_menu_progress(target);
                if let Some(timer) = weak_timer.upgrade() {
                    timer.stop();
                }
            }
        });
    });
}
