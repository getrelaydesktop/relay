//! Render the real interface without a desktop session, files or Telegram login.
use super::*;
use i_slint_renderer_skia::{SkiaRenderer, SkiaSharedContext};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, PhysicalSize, Rgba8Pixel};
use std::cell::Cell;

mod probes {
    slint::slint! {
        import { VectorIcon } from "../ui/icons/vector-icons.slint";
        import { Motion, RippleSurface, PopupReveal } from "../ui/motion.slint";
        export { TelegramMotion } from "../ui/motion.slint";
        export component AnimationProbe inherits Window {
            background: #000000;
            in-out property <bool> pressed;
            in-out property <length> press-x: 8px;
            in-out property <bool> popup-open;
            out property <float> toggle-progress: toggle.value;
            out property <int> active-waves: ripple.active-waves;
            out property <float> popup-progress: popup.show-progress;
            out property <float> popup-width: popup.width / 180px;
            out property <float> popup-height: popup.height / 100px;
            out property <float> popup-opacity: popup.opacity;
            toggle := Motion { target: root.pressed ? 1 : 0; duration: 120ms; }
            ripple := RippleSurface {
                x: 20px; y: 20px; width: 180px; height: 48px;
                border-radius: 24px;
                pressed: root.pressed; press-x: root.press-x; press-y: 24px;
                ink: #ffffff;
            }
            popup := PopupReveal {
                x: 20px; y: 0px;
                shown: root.popup-open; content-width: 180px; content-height: 100px;
                fill: #17212b;
                Text { text: "Telegram popup"; color: #ffffff; font-size: 14px; }
            }
        }
        export component IconAtlas inherits Window {
            background: #17212b;
            out property <[string]> icons: [
                "advanced", "back", "call", "channel", "chat-settings", "chats",
                "clip", "close", "edit-folders", "folder", "groups_create", "language",
                "maximize", "menu", "mic", "minimize", "moon", "more",
                "notifications", "panel", "personal", "phone", "power", "privacy",
                "profile", "read", "saved", "search", "send", "sent",
                "settings", "smile", "window-close"
            ];
            for icon[index] in root.icons: Rectangle {
                x: mod(index, 6) * 100px;
                y: floor(index / 6) * 72px;
                width: 100px;
                height: 72px;
                VectorIcon {
                    x: (parent.width - self.width) / 2;
                    y: 6px;
                    name: icon;
                    tint: #9cb6ca;
                }
                Text {
                    x: 0px; y: 50px; width: parent.width; height: 20px;
                    text: icon; color: #9cb6ca; font-size: 10px;
                    horizontal-alignment: center;
                }
            }
        }
    }
}
use probes::{AnimationProbe, IconAtlas};

struct PreviewPlatform(Rc<PreviewWindow>);

struct PreviewWindow {
    window: slint::Window,
    renderer: SkiaRenderer,
    size: Cell<PhysicalSize>,
    elapsed: Cell<Duration>,
}

impl PreviewWindow {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|weak: &std::rc::Weak<Self>| Self {
            window: slint::Window::new(weak.clone()),
            renderer: SkiaRenderer::default(&SkiaSharedContext::default()),
            size: Cell::new(PhysicalSize::new(1100, 720)),
            elapsed: Cell::new(Duration::ZERO),
        })
    }

    fn advance(&self, milliseconds: u64) {
        self.elapsed
            .set(self.elapsed.get() + Duration::from_millis(milliseconds));
        slint::platform::update_timers_and_animations();
    }
}

impl std::ops::Deref for PreviewWindow {
    type Target = slint::Window;
    fn deref(&self) -> &Self::Target {
        &self.window
    }
}

impl WindowAdapter for PreviewWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }
    fn renderer(&self) -> &dyn slint::platform::Renderer {
        &self.renderer
    }
    fn size(&self) -> PhysicalSize {
        self.size.get()
    }
    fn set_size(&self, size: slint::WindowSize) {
        let scale = self.window.scale_factor();
        self.size.set(size.to_physical(scale));
        self.window.dispatch_event(WindowEvent::Resized {
            size: size.to_logical(scale),
        });
    }
}

impl Platform for PreviewPlatform {
    fn duration_since_start(&self) -> Duration {
        self.0.elapsed.get()
    }
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

#[test]
fn portable_interface_renders_icons_at_multiple_scales() {
    let window = PreviewWindow::new();
    slint::platform::set_platform(Box::new(PreviewPlatform(window.clone()))).unwrap();
    let ui = Desktop::new().unwrap();
    install_rendering(&ui);
    let mut chats = Demo::demo_chats();
    // Exercise the downloaded-photo path, rather than only pre-rounded
    // placeholder images. The old software renderer left these square.
    let photo = image::RgbaImage::from_fn(138, 138, |_, y| {
        image::Rgba([48, 160 - (y / 3) as u8, 230, 255])
    });
    chats[0].row.avatar = slint_image(&photo);
    chats[0].row.has_avatar = true;
    ui.set_chats(ModelRc::new(VecModel::from(
        chats
            .iter()
            .map(|chat| chat.row.clone())
            .collect::<Vec<_>>(),
    )));
    ui.set_folders(ModelRc::new(VecModel::from(vec![
        FolderRow {
            id: 0,
            title: "Все чаты".into(),
            unread: 5,
        },
        FolderRow {
            id: 1,
            title: "Работа".into(),
            unread: 2,
        },
        FolderRow {
            id: 2,
            title: "Личное".into(),
            unread: 0,
        },
    ])));
    ui.set_active_chat(chats[0].row.clone());
    let mut conversation: Vec<_> = chats[0].messages.iter().collect();
    conversation.push(message("Длинное сообщение должно переноситься по словам, сохраняя отступы пузыря.\nВремя и отметки прочтения остаются внизу и не перекрывают текст.", "10:22", true));
    for (index, row) in conversation.iter_mut().enumerate() {
        row.id = index as i32 + 1;
        row.date = "9 ноября".into();
    }
    ui.set_messages(ModelRc::new(VecModel::from(conversation.clone())));
    ui.set_auth_required(false);
    ui.set_has_chat(true);
    ui.set_show_chat(true);
    ui.set_connected(true);
    ui.show().unwrap();
    window.advance(500);

    for (name, width, scale, dark, channel) in [
        ("desktop-100", 1100, 1.0, true, false),
        ("desktop-125", 1100, 1.25, true, false),
        ("desktop-150", 1100, 1.5, true, false),
        ("desktop-175", 1100, 1.75, true, false),
        ("desktop-200", 1100, 2.0, true, false),
        ("desktop-300", 1100, 3.0, true, false),
        ("desktop-light", 1100, 1.0, false, false),
        ("channel", 1100, 1.0, true, true),
        ("compact", 430, 1.0, true, false),
        ("menu", 1100, 1.0, true, false),
    ] {
        let mut active = chats[0].row.clone();
        active.read_outbox_max_id = 10;
        if channel {
            active.title = "Новости Relay".into();
            active.channel_access = 2;
            let mut service = message("Канал создан", "10:00", true);
            service.service = true;
            let mut post = message(
                "Новости канала отображаются слева, без отметок прочтения.",
                "10:22",
                true,
            );
            post.channel_post = true;
            service.date = "9 ноября".into();
            post.date = "9 ноября".into();
            ui.set_messages(ModelRc::new(VecModel::from(vec![service, post])));
        } else {
            ui.set_messages(ModelRc::new(VecModel::from(conversation.clone())));
        }
        ui.set_active_chat(active);
        ui.global::<Theme>().set_dark(dark);
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        let physical_width = (width as f32 * scale) as u32;
        let physical_height = (720.0 * scale) as u32;
        window
            .window
            .set_size(PhysicalSize::new(physical_width, physical_height));
        slint::platform::update_timers_and_animations();
        let mut pixels = window.take_snapshot().unwrap();
        // The first pass resolves the list's item sizes; the second positions
        // the short conversation against the bottom of the viewport.
        for _ in 0..2 {
            window.advance(500);
            pixels = window.take_snapshot().unwrap();
        }
        if name == "menu" {
            let position = LogicalPosition::new(1074.0, 53.0);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: slint::platform::PointerEventButton::Left,
            });
            pixels = window.take_snapshot().unwrap();
            window.advance(500);
            pixels = window.take_snapshot().unwrap();
        }
        if channel {
            let mut incoming = 0;
            let mut outgoing = 0;
            for y in 80..674 {
                for x in 398..1100 {
                    let pixel = pixels.as_slice()[y * physical_width as usize + x];
                    incoming += usize::from(
                        pixel
                            == Rgba8Pixel {
                                r: 24,
                                g: 37,
                                b: 51,
                                a: 255,
                            },
                    );
                    outgoing += usize::from(
                        pixel
                            == Rgba8Pixel {
                                r: 43,
                                g: 82,
                                b: 120,
                                a: 255,
                            },
                    );
                }
            }
            assert!(incoming > 100, "missing channel post bubble");
            assert_eq!(outgoing, 0, "channel post rendered as an outgoing message");
        }
        if width > 740 {
            // Regression: the hamburger was completely absent in a standalone
            // executable, because its mask was loaded relative to the CWD.
            let pixel = |x: u32, y: u32| {
                pixels.as_slice()[((y as f32 * scale) as u32 * physical_width
                    + (x as f32 * scale) as u32) as usize]
            };
            let background = pixel(12, 45);
            assert_eq!(
                pixel(82, 88),
                pixel(76, 88),
                "photo avatar must have clipped corners at {scale}x"
            );
            assert_ne!(
                pixel(105, 111),
                pixel(76, 88),
                "photo avatar center must remain visible at {scale}x"
            );
            assert!(
                (22..50).any(|x| (40..65).any(|y| pixel(x, y) != background)),
                "missing menu icon at {scale}x"
            );
            let caption_background = pixel(width - 107, 4);
            assert!(
                (width - 30..width - 6).any(|x| (4..22).any(|y| pixel(x, y) != caption_background)),
                "missing close icon at {scale}x"
            );
        }
        // Optional visual artifacts for manual review. No user data is used.
        if let Some(directory) = std::env::var_os("RELAY_UI_SNAPSHOTS") {
            std::fs::create_dir_all(&directory).unwrap();
            image::save_buffer(
                std::path::Path::new(&directory).join(format!("{name}.png")),
                pixels.as_bytes(),
                physical_width,
                physical_height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
    verify_telegram_motion(&window, &ui);
    ui.hide().unwrap();
    drop(ui);

    // Check every glyph, including settings and caption icons absent from the
    // main conversation snapshot. The curved glyphs must retain AA coverage.
    let atlas = IconAtlas::new().unwrap();
    atlas.show().unwrap();
    for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        let width = (600.0 * scale) as u32;
        let height = (432.0 * scale) as u32;
        window.window.set_size(PhysicalSize::new(width, height));
        let pixels = window.take_snapshot().unwrap();
        let background = Rgba8Pixel {
            r: 23,
            g: 33,
            b: 43,
            a: 255,
        };
        let foreground = Rgba8Pixel {
            r: 156,
            g: 182,
            b: 202,
            a: 255,
        };
        for (index, name) in atlas.get_icons().iter().enumerate() {
            let left = ((index % 6) as f32 * 100.0 * scale) as u32;
            let top = ((index / 6) as f32 * 72.0 * scale) as u32;
            let mut ink = 0;
            let mut antialiased = 0;
            for y in top..top + (48.0 * scale) as u32 {
                for x in left..left + (100.0 * scale) as u32 {
                    let p = pixels.as_slice()[(y * width + x) as usize];
                    ink += usize::from(p != background);
                    antialiased += usize::from(p != background && p != foreground);
                }
            }
            assert!(ink > 0, "missing vector icon {name} at {scale}x");
            if ["clip", "mic", "search", "smile", "more", "notifications"].contains(&name.as_str())
            {
                assert!(antialiased > 0, "aliased curve in {name} at {scale}x");
            }
        }
        if let Some(directory) = std::env::var_os("RELAY_UI_SNAPSHOTS") {
            image::save_buffer(
                std::path::Path::new(&directory)
                    .join(format!("icons-{}.png", (scale * 100.0) as u32)),
                pixels.as_bytes(),
                width,
                height,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }
    atlas.hide().unwrap();
    drop(atlas);

    let probe = AnimationProbe::new().unwrap();
    probe
        .global::<probes::TelegramMotion>()
        .on_append_ripple(append_ripple);
    probe
        .global::<probes::TelegramMotion>()
        .on_remove_ripple(remove_ripple);
    probe.show().unwrap();
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.window.set_size(PhysicalSize::new(240, 100));
    window.take_snapshot().unwrap();
    probe.set_pressed(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(20);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        (probe.get_toggle_progress() - 1.0 / 6.0).abs() < 0.002,
        "toggle must take 120ms"
    );
    assert_eq!(
        pixels.as_slice()[44 * 240 + 19].r,
        0,
        "ripple must stay inside its mask"
    );
    assert!(
        pixels.as_slice()[44 * 240 + 28].r > 0,
        "ripple must expand from the pointer"
    );
    save_animation_frame("ripple-20", &pixels);
    probe.set_pressed(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(20);
    probe.set_press_x(120.0);
    probe.set_pressed(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(30);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        pixels.as_slice()[44 * 240 + 28].r > 0,
        "the first wave must keep fading after a second press"
    );
    assert!(
        pixels.as_slice()[44 * 240 + 140].r > 0,
        "the second wave must expand from the new pointer location"
    );
    save_animation_frame("ripple-overlap", &pixels);
    assert_eq!(
        pixels.as_slice()[21 * 240 + 21].r,
        0,
        "ripples must respect the rounded mask at the top-left corner"
    );
    assert!(
        pixels.as_slice()[28 * 240 + 28].r > 0,
        "the rounded mask must preserve coverage inside its arc"
    );
    window.advance(200);
    window.take_snapshot().unwrap();
    assert_eq!(
        probe.get_active_waves(),
        1,
        "finished waves must be removed while the new press is still held"
    );
    probe.set_pressed(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(250);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        pixels
            .as_slice()
            .iter()
            .all(|p| p.r == 0 && p.g == 0 && p.b == 0),
        "released ripples must disappear"
    );
    assert_eq!(probe.get_active_waves(), 0);
    probe.set_popup_open(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(50);
    let pixels = window.take_snapshot().unwrap();
    assert!((probe.get_popup_width() - 0.9061164).abs() < 0.002);
    assert!((probe.get_popup_height() - 0.7841628).abs() < 0.002);
    assert!((probe.get_popup_opacity() - 0.9860133).abs() < 0.002);
    save_animation_frame("popup-open-50", &pixels);
    probe.set_popup_open(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(50);
    window.take_snapshot().unwrap();
    assert!(
        (probe.get_popup_progress() - 0.5).abs() < 0.002,
        "dismissing must keep the unfinished popup reveal running"
    );
    assert!((probe.get_popup_opacity() - 2.0 / 3.0).abs() < 0.002);
    probe.set_popup_open(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    assert!(
        (probe.get_popup_opacity() - 2.0 / 3.0).abs() < 0.002,
        "reopening a popup must preserve its current opacity"
    );
    window.advance(150);
    window.take_snapshot().unwrap();
    assert_eq!(probe.get_popup_progress(), 1.0);
    assert_eq!(probe.get_popup_opacity(), 1.0);
    probe.hide().unwrap();
}

fn save_animation_frame(name: &str, pixels: &slint::SharedPixelBuffer<Rgba8Pixel>) {
    if let Some(directory) = std::env::var_os("RELAY_UI_SNAPSHOTS") {
        image::save_buffer(
            std::path::Path::new(&directory).join(format!("animation-{name}.png")),
            pixels.as_bytes(),
            pixels.width(),
            pixels.height(),
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

fn verify_telegram_motion(window: &PreviewWindow, ui: &Desktop) {
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.window.set_size(PhysicalSize::new(1100, 720));
    ui.global::<Theme>().set_dark(true);
    ui.set_chat_menu_open(false);
    ui.set_menu_open(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(500);
    window.take_snapshot().unwrap();

    ui.set_menu_open(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(100);
    let pixels = window.take_snapshot().unwrap();
    // Upstream easeOutCirc(0.5), independent golden sample (not a bezier fit).
    assert!(
        (ui.get_menu_progress() - 0.8660254).abs() < 0.002,
        "menu must use 200ms easeOutCirc: {}",
        ui.get_menu_progress()
    );
    save_animation_frame("menu-open-100", &pixels);
    ui.set_menu_open(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    assert!(
        (ui.get_menu_progress() - 0.8660254).abs() < 0.002,
        "reversing must preserve the current position"
    );
    window.advance(100);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        (ui.get_menu_progress() - 0.1160254).abs() < 0.002,
        "closing menu must retain the circular curve"
    );
    assert!(
        (ui.get_layer_dim_progress() - 0.4330127).abs() < 0.002,
        "closing dim must fade linearly"
    );
    save_animation_frame("menu-close-100", &pixels);
    ui.set_menu_open(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    assert!(
        (ui.get_menu_progress() - 0.1160254).abs() < 0.002,
        "rapid reopen must not jump"
    );
    window.advance(200);
    window.take_snapshot().unwrap();
    assert_eq!(ui.get_menu_progress(), 1.0);
    ui.set_menu_open(false);
    ui.set_settings_open(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    assert_eq!(
        ui.get_layer_dim_progress(),
        1.0,
        "menu-to-settings must preserve the shared dim"
    );
    window.advance(100);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        (ui.get_settings_progress() - 0.5).abs() < 0.002,
        "box content must fade linearly over 200ms"
    );
    save_animation_frame("settings-open-100", &pixels);
    window.advance(100);
    window.take_snapshot().unwrap();
    ui.set_settings_page(1);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(120);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        (ui.get_settings_page_progress() - 0.8660254).abs() < 0.002,
        "page slide must take 240ms"
    );
    save_animation_frame("settings-page-120", &pixels);
    window.advance(120);
    window.take_snapshot().unwrap();
    ui.set_settings_open(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(200);
    window.take_snapshot().unwrap();

    window.window.set_size(PhysicalSize::new(430, 720));
    ui.set_show_chat(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(240);
    window.take_snapshot().unwrap();
    ui.set_show_chat(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(120);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        (ui.get_compact_progress() - 0.8660254).abs() < 0.002,
        "compact navigation must use the section transition"
    );
    save_animation_frame("compact-chat-120", &pixels);
    window.advance(120);
    window.take_snapshot().unwrap();
    ui.set_auth_required(true);
    ui.set_auth_step(1);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(100);
    let pixels = window.take_snapshot().unwrap();
    assert!(
        (ui.get_auth_transition_progress() - 0.5).abs() < 0.002,
        "intro uses a 200ms linear timeline with separate circular curves"
    );
    save_animation_frame("intro-phone-100", &pixels);
    window.advance(100);
    window.take_snapshot().unwrap();
    assert_eq!(ui.get_auth_transition_progress(), 1.0);
}
