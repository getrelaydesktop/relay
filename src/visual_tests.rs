//! Render the real interface without a desktop session, files or Telegram login.
use super::*;
use i_slint_renderer_skia::{SkiaRenderer, SkiaSharedContext};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, PhysicalSize, Rgba8Pixel};
use std::cell::Cell;

mod probes {
    slint::slint! {
        import { VectorIcon } from "../ui/icons/vector-icons.slint";
        import { Motion, MessageSendMotion, MessageReceiveMotion, RippleSurface, PopupReveal } from "../ui/motion.slint";
        export { TelegramMotion } from "../ui/motion.slint";
        export component AnimationProbe inherits Window {
            background: #000000;
            in-out property <bool> pressed;
            in-out property <length> press-x: 8px;
            in-out property <bool> popup-open;
            in-out property <int> send-token;
            in-out property <int> receive-token;
            out property <float> receive-progress: receive.progress;
            receive := MessageReceiveMotion { token: root.receive-token; }
            out property <float> send-progress: send.progress;
            out property <float> send-horizontal: send.horizontal;
            out property <float> send-rise: send.rise;
            out property <float> send-mix: send.mix;
            send := MessageSendMotion { token: root.send-token; }
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

    for (name, width, height, scale, dark, channel) in [
        ("desktop-100", 1100, 720, 1.0, true, false),
        ("desktop-125", 1100, 720, 1.25, true, false),
        ("desktop-150", 1100, 720, 1.5, true, false),
        ("desktop-175", 1100, 720, 1.75, true, false),
        ("desktop-200", 1100, 720, 2.0, true, false),
        ("desktop-300", 1100, 720, 3.0, true, false),
        ("desktop-light", 1100, 720, 1.0, false, false),
        ("maximized-1080p", 1920, 1040, 1.0, true, false),
        ("maximized-1440p-150", 1706, 933, 1.5, true, false),
        ("maximized-4k-200", 1920, 1040, 2.0, true, false),
        ("channel", 1100, 720, 1.0, true, true),
        ("compact", 430, 720, 1.0, true, false),
        ("menu", 1100, 720, 1.0, true, false),
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
        let physical_height = (height as f32 * scale) as u32;
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
    verify_profiles(&window, &ui);
    verify_tray_resource_release(&window, &ui);
    verify_sending(&window, &ui);
    verify_incoming(&window, &ui);
    verify_context_menu(&window, &ui);
    verify_photos_and_replies(&window, &ui);
    verify_rich_text_and_composer(&window, &ui);
    verify_maximized_history_work(&window, &ui);
    verify_network_burst_yields(&window, &ui);
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
        .on_send_rise(sending::rise_progress);
    probe
        .global::<probes::TelegramMotion>()
        .on_claim_send(|_| true);
    probe
        .global::<probes::TelegramMotion>()
        .on_claim_receive(|_| true);
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
    probe.set_send_token(-1001);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(40);
    window.take_snapshot().unwrap();
    assert!((probe.get_send_progress() - 1.0 / 7.0).abs() < 0.002);
    assert!((probe.get_send_horizontal() - 0.7626953).abs() < 0.002);
    assert!((probe.get_send_mix() - 40.0 / 110.0).abs() < 0.002);
    window.advance(100);
    window.take_snapshot().unwrap();
    assert!(
        (probe.get_send_rise() - 0.8024034).abs() < 0.002,
        "sending rise must use Telegram's cubic bezier: {}",
        probe.get_send_rise()
    );
    window.advance(140);
    window.take_snapshot().unwrap();
    assert_eq!(probe.get_send_progress(), 1.0);
    assert_eq!(probe.get_send_rise(), 1.0);
    probe.set_receive_token(-2001);
    window.advance(0);
    window.take_snapshot().unwrap();
    assert_eq!(probe.get_receive_progress(), 0.0);
    window.advance(30);
    window.take_snapshot().unwrap();
    assert!((probe.get_receive_progress() - 0.67232).abs() < 0.002);
    window.advance(45);
    window.take_snapshot().unwrap();
    assert!((probe.get_receive_progress() - 0.96875).abs() < 0.002);
    window.advance(75);
    window.take_snapshot().unwrap();
    assert_eq!(probe.get_receive_progress(), 1.0);
    probe.hide().unwrap();
}

fn verify_incoming(window: &PreviewWindow, ui: &Desktop) {
    let (bridge, _commands, events) = telegram::TelegramBridge::offline_for_rendering();
    let mut state = Demo::with_bridge(bridge);
    state.live_mode = true;
    let today = chrono::Local::now().format("%d.%m.%Y").to_string();
    for chat in &state.chats {
        for index in 0..chat.messages.row_count() {
            let mut row = chat.messages.row_data(index).unwrap();
            row.id = index as i32 + 1;
            row.date = today.clone().into();
            chat.messages.set_row_data(index, row);
        }
    }
    ui.set_draft(Default::default());
    ui.set_network_status(Default::default());
    ui.set_reply_message(0);
    ui.set_message_busy(false);
    state.refresh_chats(ui);
    state.refresh_history(ui);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(500);
    window.take_snapshot().unwrap();
    assert!(ui.get_history_at_bottom());
    let incoming = telegram::TgMessage {
        id: 101,
        date: today.clone(),
        has_media: false,
        is_photo: false,
        body: "Входящее сообщение плавно появляется снизу, сдвигая переписку".into(),
        time: "12:31".into(),
        outgoing: false,
        channel_post: false,
        service: false,
        ..Default::default()
    };
    let count = state.chats[0].messages.row_count();
    events
        .send(telegram::TgEvent::NewMessage {
            peer_id: 0,
            message: incoming.clone(),
        })
        .unwrap();
    state.poll_telegram(ui);
    let token = state.chats[0]
        .messages
        .row_data(count)
        .unwrap()
        .receive_token;
    assert_ne!(token, 0, "new visible incoming messages must animate");
    window.advance(0);
    save_animation_frame("receive-0", &window.take_snapshot().unwrap());
    window.advance(30);
    save_animation_frame("receive-30", &window.take_snapshot().unwrap());
    // Duplicate updates preserve the live delegate and cannot restart it.
    events
        .send(telegram::TgEvent::NewMessage {
            peer_id: 0,
            message: incoming.clone(),
        })
        .unwrap();
    state.poll_telegram(ui);
    assert_eq!(state.chats[0].messages.row_count(), count + 1);
    assert_eq!(
        state.chats[0]
            .messages
            .row_data(count)
            .unwrap()
            .receive_token,
        token
    );
    window.advance(45);
    save_animation_frame("receive-75", &window.take_snapshot().unwrap());
    window.advance(75);
    save_animation_frame("receive-150", &window.take_snapshot().unwrap());
    assert!(!ui.global::<TelegramMotion>().invoke_claim_receive(token));

    // Loading existing history keeps the token, rather than replaying a reveal.
    let rows = state.chats[0]
        .messages
        .iter()
        .map(|mut row| {
            row.receive_token = 0;
            row
        })
        .collect();
    sending::merge_history(&state.chats[0].messages, rows);
    assert_eq!(
        state.chats[0]
            .messages
            .row_data(count)
            .unwrap()
            .receive_token,
        token
    );

    let history: Vec<_> = (0..30)
        .map(|index| {
            let mut row = message(
                "Предыдущая переписка\nОстаётся на месте при получении нового сообщения",
                "12:30",
                index % 2 == 1,
            );
            row.id = index + 1;
            row.date = today.clone().into();
            row
        })
        .collect();
    state.chats[0].messages.set_vec(history);
    window.advance(500);
    window.take_snapshot().unwrap();
    ui.set_history_scroll_y(-160.0);
    window.take_snapshot().unwrap();
    assert!(!ui.get_history_at_bottom());
    let offset = ui.get_history_scroll_y();
    let mut another = incoming.clone();
    another.id = 102;
    events
        .send(telegram::TgEvent::NewMessage {
            peer_id: 0,
            message: another,
        })
        .unwrap();
    state.poll_telegram(ui);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(200);
    window.take_snapshot().unwrap();
    assert!(
        (ui.get_history_scroll_y() - offset).abs() < 0.01,
        "incoming messages must not pull readers away from older history"
    );
    assert_eq!(
        state.chats[0].messages.iter().last().unwrap().receive_token,
        0
    );

    // A long conversation follows the reveal through its final frame, even
    // when there are 75 ms between rendered frames.
    ui.invoke_scroll_to_bottom();
    window.take_snapshot().unwrap();
    let mut tall = incoming.clone();
    tall.id = 103;
    tall.body = "Ещё одна строка входящего сообщения\n".repeat(8);
    events
        .send(telegram::TgEvent::NewMessage {
            peer_id: 0,
            message: tall,
        })
        .unwrap();
    state.poll_telegram(ui);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(75);
    window.take_snapshot().unwrap();
    window.advance(75);
    window.take_snapshot().unwrap();
    assert!(
        ui.get_history_at_bottom(),
        "the final incoming frame must remain pinned to the bottom"
    );

    let unread = state.chats[1].row.unread;
    events
        .send(telegram::TgEvent::NewMessage {
            peer_id: 1,
            message: incoming,
        })
        .unwrap();
    state.poll_telegram(ui);
    assert_eq!(
        state.chats[1].messages.iter().last().unwrap().receive_token,
        0
    );
    assert_eq!(state.chats[1].row.unread, unread + 1);
}

fn verify_context_menu(window: &PreviewWindow, ui: &Desktop) {
    ui.set_network_status(Default::default());
    ui.global::<Theme>().set_dark(true);
    ui.set_emoji_open(false);
    ui.set_chat_menu_open(false);
    let rows: Vec<_> = (0..30)
        .map(|index| {
            let mut row = message(
                "Сообщение для проверки меню\nПосле прокрутки и при разном масштабе",
                "12:32",
                index % 2 == 1,
            );
            row.id = index + 1;
            row.date = "09.11.2026".into();
            row
        })
        .collect();
    ui.set_messages(ModelRc::new(VecModel::from(rows)));
    let actions = Rc::new(RefCell::new(Vec::new()));
    let received = Rc::clone(&actions);
    let weak = ui.as_weak();
    ui.on_message_action(move |action| {
        received.borrow_mut().push((
            action.to_string(),
            weak.upgrade().unwrap().get_selected_message(),
        ));
    });
    for (label, width, scale) in [
        ("desktop", 1100, 1.0),
        ("desktop-150", 1100, 1.5),
        ("compact", 430, 1.0),
    ] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        window.window.set_size(PhysicalSize::new(
            (width as f32 * scale) as u32,
            (720.0 * scale) as u32,
        ));
        window.advance(0);
        window.take_snapshot().unwrap();
        window.advance(500);
        window.take_snapshot().unwrap();
        ui.set_history_scroll_y(-250.0);
        let pixels = window.take_snapshot().unwrap();
        let left = if width < 740 { 0 } else { 398 };
        for (corner, outgoing, bottom) in [
            ("left", false, false),
            ("bottom-left", false, true),
            ("bottom-right", true, true),
        ] {
            let x = if outgoing { width - 35 } else { left + 60 };
            let color = if outgoing {
                [43, 82, 120]
            } else {
                [24, 37, 51]
            };
            let matches = |y: &u32| {
                let index = ((*y as f32 * scale) as u32 * pixels.width()
                    + (x as f32 * scale) as u32) as usize;
                let p = pixels.as_slice()[index];
                [p.r, p.g, p.b] == color
            };
            let y = if bottom {
                (500..664).rev().find(matches)
            } else {
                (130..300).find(matches)
            }
            .expect("visible bubble under the test pointer");
            let position = LogicalPosition::new(x as f32, y as f32);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: slint::platform::PointerEventButton::Right,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: slint::platform::PointerEventButton::Right,
            });
            window.advance(0);
            window.take_snapshot().unwrap();
            assert!(
                ui.get_chat_menu_open(),
                "right-click must open the message menu: {label}/{corner}"
            );
            let selected = ui.get_selected_message();
            assert!(selected > 0);
            let anchor_x = ui.get_chat_menu_x()
                + if outgoing {
                    ui.get_chat_menu_width()
                } else {
                    0.0
                };
            let anchor_y = ui.get_chat_menu_y()
                + if bottom {
                    ui.get_chat_menu_height()
                } else {
                    0.0
                };
            assert!(
                (anchor_x - (x - left) as f32).abs() < 1.0,
                "menu must start at the clicked bubble, including sidebar and scroll offsets: {label}/{corner}: {anchor_x} != {}",
                x - left
            );
            assert!(
                (anchor_y - (y - 26) as f32).abs() < 1.0,
                "menu must account for titlebar and viewport scroll: {label}/{corner}: {anchor_y} != {}",
                y - 26
            );
            window.advance(200);
            save_animation_frame(
                &format!("context-{label}-{corner}"),
                &window.take_snapshot().unwrap(),
            );
            let end_x = ui.get_chat_menu_x()
                + if outgoing {
                    ui.get_chat_menu_width()
                } else {
                    0.0
                };
            let end_y = ui.get_chat_menu_y()
                + if bottom {
                    ui.get_chat_menu_height()
                } else {
                    0.0
                };
            assert!(
                (end_x - anchor_x).abs() < 0.01 && (end_y - anchor_y).abs() < 0.01,
                "the opening corner must stay fixed while the menu reveals"
            );
            assert!(
                ui.get_chat_menu_x() >= 4.0
                    && ui.get_chat_menu_x() + ui.get_chat_menu_width() <= (width - left - 4) as f32
            );
            assert!(
                ui.get_chat_menu_y() >= 4.0
                    && ui.get_chat_menu_y() + ui.get_chat_menu_height() <= 690.0
            );
            // The first action must operate on the clicked message.
            let reply = LogicalPosition::new(
                left as f32 + ui.get_chat_menu_x() + 40.0,
                26.0 + ui.get_chat_menu_y() + 20.0,
            );
            window.dispatch_event(WindowEvent::PointerPressed {
                position: reply,
                button: slint::platform::PointerEventButton::Left,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position: reply,
                button: slint::platform::PointerEventButton::Left,
            });
            assert_eq!(
                actions.borrow().last(),
                Some(&("reply".to_string(), selected))
            );
            assert!(!ui.get_chat_menu_open());
            window.advance(0);
            window.take_snapshot().unwrap();
            window.advance(200);
            window.take_snapshot().unwrap();
        }
        // The header button still anchors to the header, not the last message.
        let position = LogicalPosition::new(width as f32 - 26.0, 53.0);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        window.advance(0);
        window.take_snapshot().unwrap();
        window.advance(200);
        window.take_snapshot().unwrap();
        assert!(ui.get_chat_menu_open());
        assert_eq!(ui.get_selected_message(), 0);
        assert!((ui.get_chat_menu_x() - (width - left - 218 - 8) as f32).abs() < 1.0);
        assert_eq!(ui.get_chat_menu_y(), 50.0);
        ui.set_chat_menu_open(false);
        window.advance(0);
        window.take_snapshot().unwrap();
        window.advance(200);
        window.take_snapshot().unwrap();
    }
}

fn verify_photos_and_replies(window: &PreviewWindow, ui: &Desktop) {
    let (bridge, _commands, events) = telegram::TelegramBridge::offline_for_rendering();
    let mut state = Demo::with_bridge(bridge);
    state.live_mode = true;
    let mut parent = telegram::TgMessage {
        id: 1,
        sender: "Анна".into(),
        body: "Исходное сообщение для ответа".into(),
        time: "12:00".into(),
        ..Default::default()
    };
    let reply = telegram::TgMessage {
        id: 2,
        reply_to: 1,
        sender: "Вы".into(),
        body: "Ответ с цитатой и переходом".into(),
        time: "12:01".into(),
        outgoing: true,
        ..Default::default()
    };
    let photo = telegram::TgMessage {
        id: 3,
        is_photo: true,
        has_media: true,
        photo_width: 640,
        photo_height: 480,
        body: "Подпись к фотографии".into(),
        time: "12:02".into(),
        ..Default::default()
    };
    state.chats[0].messages.set_vec(vec![
        live_message(&parent),
        live_message(&reply),
        live_message(&photo),
    ]);
    state.refresh_history(ui);
    events
        .send(telegram::TgEvent::Thumbnail {
            peer_id: 0,
            message_id: 3,
            key: String::new(),
            pixels: Some(image::RgbaImage::from_pixel(
                640,
                480,
                image::Rgba([20, 160, 80, 255]),
            )),
        })
        .unwrap();
    events
        .send(telegram::TgEvent::Presence {
            peer_id: 0,
            text: "был(а) сегодня в 12:34".into(),
            online: false,
        })
        .unwrap();
    state.poll_telegram(ui);
    assert_eq!(
        state.chats[0].messages.row_data(1).unwrap().reply_author,
        "Анна"
    );
    assert_eq!(
        state.chats[0].messages.row_data(1).unwrap().reply_body,
        parent.body
    );
    assert!(state.chats[0].messages.row_data(2).unwrap().has_preview);
    assert_eq!(ui.get_active_chat().presence, "был(а) сегодня в 12:34");
    let clicked = Rc::new(Cell::new(0));
    let received = clicked.clone();
    ui.on_view_photo(move |id| received.set(id));
    ui.set_reply_message(0);
    ui.set_edit_message_id(0);
    ui.set_network_status(Default::default());
    for (label, width, scale) in [
        ("photo-replies", 1100, 1.0),
        ("photo-replies-150", 1100, 1.5),
        ("photo-replies-compact", 430, 1.0),
    ] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        window.window.set_size(PhysicalSize::new(
            (width as f32 * scale) as u32,
            (720.0 * scale) as u32,
        ));
        window.advance(0);
        window.take_snapshot().unwrap();
        window.advance(500);
        window.take_snapshot().unwrap();
        ui.invoke_scroll_to_bottom();
        let pixels = window.take_snapshot().unwrap();
        let green: Vec<_> = pixels
            .as_slice()
            .iter()
            .enumerate()
            .filter_map(|(i, p)| (p.r == 20 && p.g == 160 && p.b == 80).then_some(i))
            .collect();
        assert!(
            green.len() > 1000,
            "photo must render inside its bubble: {label}"
        );
        save_animation_frame(label, &pixels);
        let index = green[green.len() / 2];
        let position = LogicalPosition::new(
            (index as u32 % pixels.width()) as f32 / scale,
            (index as u32 / pixels.width()) as f32 / scale,
        );
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        assert_eq!(
            clicked.get(),
            3,
            "photo preview must open the existing viewer"
        );
    }
    parent.body = "Отредактированный оригинал".into();
    events
        .send(telegram::TgEvent::MessageEdited {
            peer_id: 0,
            message: parent.clone(),
        })
        .unwrap();
    state.poll_telegram(ui);
    assert_eq!(
        state.chats[0].messages.row_data(1).unwrap().reply_body,
        parent.body
    );
    events
        .send(telegram::TgEvent::History {
            peer_id: 0,
            messages: vec![parent.clone(), reply.clone(), photo.clone()],
        })
        .unwrap();
    state.poll_telegram(ui);
    assert!(state.chats[0].messages.row_data(2).unwrap().has_preview);
    assert_eq!(state.chats[0].messages.row_data(1).unwrap().reply_to, 1);
    let mut replacement = photo.clone();
    replacement.media_key = "new-photo".into();
    events
        .send(telegram::TgEvent::MessageEdited {
            peer_id: 0,
            message: replacement,
        })
        .unwrap();
    state.poll_telegram(ui);
    assert!(!state.chats[0].messages.row_data(2).unwrap().has_preview);
    events
        .send(telegram::TgEvent::Thumbnail {
            peer_id: 0,
            message_id: 3,
            key: "old-photo".into(),
            pixels: Some(image::RgbaImage::from_pixel(
                640,
                480,
                image::Rgba([220, 20, 20, 255]),
            )),
        })
        .unwrap();
    state.poll_telegram(ui);
    assert!(
        !state.chats[0].messages.row_data(2).unwrap().has_preview,
        "a late download must not replace an edited photo"
    );
    events
        .send(telegram::TgEvent::Thumbnail {
            peer_id: 0,
            message_id: 3,
            key: "new-photo".into(),
            pixels: Some(image::RgbaImage::from_pixel(
                640,
                480,
                image::Rgba([20, 160, 80, 255]),
            )),
        })
        .unwrap();
    state.poll_telegram(ui);
    assert!(state.chats[0].messages.row_data(2).unwrap().has_preview);
    ui.set_reply_message(1);
    ui.set_reply_author("Анна".into());
    ui.set_reply_body(parent.body.clone().into());
    window.advance(0);
    window.take_snapshot().unwrap();
    save_animation_frame("reply-composer", &window.take_snapshot().unwrap());
    ui.set_reply_message(0);
    ui.set_file_preview(state.chats[0].messages.row_data(2).unwrap().preview);
    ui.set_has_file_preview(true);
    ui.set_file_open(true);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(200);
    let preview = window.take_snapshot().unwrap();
    assert!(
        preview
            .as_slice()
            .iter()
            .filter(|p| p.r == 20 && p.g == 160 && p.b == 80)
            .count()
            > 1000
    );
    save_animation_frame("attachment-preview", &preview);
    ui.set_file_open(false);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(200);
    window.take_snapshot().unwrap();
    let rows: Vec<_> = (1..30)
        .map(|id| {
            let mut row = message("Более раннее сообщение\nПереход из цитаты", "12:00", false);
            row.id = id;
            row
        })
        .collect();
    ui.set_messages(ModelRc::new(VecModel::from(rows)));
    window.advance(0);
    window.take_snapshot().unwrap();
    ui.invoke_scroll_to_bottom();
    window.take_snapshot().unwrap();
    let before = ui.get_history_scroll_y();
    assert!(before < -100.0);
    ui.invoke_reveal_message(1);
    window.advance(0);
    window.take_snapshot().unwrap();
    assert!(
        ui.get_history_scroll_y() > before + 100.0,
        "jumping to a reply must scroll to the original"
    );
    save_animation_frame("reply-jump", &window.take_snapshot().unwrap());
}

fn verify_sending(window: &PreviewWindow, ui: &Desktop) {
    let (bridge, mut commands, events) = telegram::TelegramBridge::offline_for_rendering();
    let mut state = Demo::with_bridge(bridge);
    state.live_mode = true;
    let today: slint::SharedString = chrono::Local::now().format("%d.%m.%Y").to_string().into();
    for chat in &state.chats {
        for index in 0..chat.messages.row_count() {
            let mut row = chat.messages.row_data(index).unwrap();
            row.id = index as i32 + 1;
            row.date = today.clone();
            chat.messages.set_row_data(index, row);
        }
    }
    ui.set_auth_required(false);
    ui.set_settings_open(false);
    window.window.set_size(PhysicalSize::new(1100, 720));
    state.refresh_chats(ui);
    state.refresh_history(ui);
    window.advance(0);
    window.take_snapshot().unwrap();
    window.advance(500);
    window.take_snapshot().unwrap();
    let before = state.chats[0].messages.row_count();
    ui.set_draft("Текст плавно перелетает из поля ввода в сообщение".into());
    ui.set_reply_message(42);
    state.send(ui);
    assert!(
        ui.get_draft().is_empty(),
        "the composer must clear before any response"
    );
    assert_eq!(state.chats[0].messages.row_count(), before + 1);
    assert!(state.chats[0].messages.row_data(before).unwrap().pending);
    assert!(matches!(
        commands.try_recv().unwrap(),
        telegram::TgCommand::SendFormatted(0, text, Some(42), entities)
            if text == "Текст плавно перелетает из поля ввода в сообщение" && entities.is_empty()
    ));
    window.advance(0);
    let start = window.take_snapshot().unwrap();
    let composer_ink = |pixels: &slint::SharedPixelBuffer<Rgba8Pixel>| {
        (688..706)
            .flat_map(|y| (450..650).map(move |x| (y * 1100 + x) as usize))
            .filter(|index| {
                let p = pixels.as_slice()[*index];
                p.r > 200 && p.g > 200 && p.b > 200
            })
            .count()
    };
    assert!(
        composer_ink(&start) > 20,
        "the flying text must start over the cleared composer, before entering history"
    );
    save_animation_frame("send-0", &start);
    window.advance(40);
    save_animation_frame("send-40", &window.take_snapshot().unwrap());
    window.advance(70);
    save_animation_frame("send-110", &window.take_snapshot().unwrap());
    window.advance(170);
    let finish = window.take_snapshot().unwrap();
    assert_eq!(
        composer_ink(&finish),
        0,
        "flying text must leave the composer at the end"
    );
    save_animation_frame("send-280", &finish);

    // A server response updates the existing bubble and preserves a new draft.
    ui.set_draft("Следующее сообщение".into());
    let body = state.chats[0]
        .messages
        .row_data(before)
        .unwrap()
        .body
        .to_string();
    let server = telegram::TgMessage {
        id: 91,
        date: today.to_string(),
        has_media: false,
        is_photo: false,
        body,
        time: "12:30".into(),
        outgoing: true,
        channel_post: false,
        service: false,
        ..Default::default()
    };
    events
        .send(telegram::TgEvent::MessageSent {
            peer_id: 0,
            message: server.clone(),
        })
        .unwrap();
    events
        .send(telegram::TgEvent::NewMessage {
            peer_id: 0,
            message: server,
        })
        .unwrap();
    state.poll_telegram(ui);
    assert_eq!(ui.get_draft(), "Следующее сообщение");
    assert_eq!(state.chats[0].messages.row_count(), before + 1);
    let sent = state.chats[0].messages.row_data(before).unwrap();
    assert_eq!(sent.id, 91);
    assert!(!sent.pending);
    assert!(
        !ui.global::<TelegramMotion>()
            .invoke_claim_send(sent.send_token),
        "acknowledgments and history refresh must not replay a completed send"
    );

    // Keep failed text in its bubble; retry the same row and reply target.
    ui.set_reply_message(53);
    state.send(ui);
    commands.try_recv().unwrap();
    events
        .send(telegram::TgEvent::SendFailed {
            message: "Offline test".into(),
        })
        .unwrap();
    state.poll_telegram(ui);
    let failed = state.chats[0].messages.row_data(before + 1).unwrap();
    assert!(failed.failed && !failed.pending);
    assert_eq!(failed.body, "Следующее сообщение");
    state.retry_send(ui, failed.id);
    assert_eq!(state.chats[0].messages.row_count(), before + 2);
    assert!(matches!(
        commands.try_recv().unwrap(),
        telegram::TgCommand::SendFormatted(0, text, Some(53), entities)
            if text == "Следующее сообщение" && entities.is_empty()
    ));
    assert!(
        state.chats[0]
            .messages
            .row_data(before + 1)
            .unwrap()
            .pending
    );
    events
        .send(telegram::TgEvent::Error {
            message: "Unrelated request failed".into(),
        })
        .unwrap();
    state.poll_telegram(ui);
    assert!(
        ui.get_message_busy(),
        "an unrelated error must not unlock an active text send"
    );
    events
        .send(telegram::TgEvent::SendFailed {
            message: "Offline test".into(),
        })
        .unwrap();
    state.poll_telegram(ui);
    drop(commands);
    state.retry_send(ui, failed.id);
    state.poll_telegram(ui);
    let row = state.chats[0].messages.row_data(before + 1).unwrap();
    assert!(
        row.failed && !row.pending,
        "a disconnected bridge must leave retryable text instead of a stuck clock"
    );
}

fn verify_rich_text_and_composer(window: &PreviewWindow, ui: &Desktop) {
    use grammers_tl_types::{enums::MessageEntity, types};
    ui.set_auth_required(false);
    ui.set_has_chat(true);
    ui.set_show_chat(true);
    ui.set_file_open(false);
    ui.set_photo_open(false);
    ui.set_menu_open(false);
    ui.set_settings_open(false);
    ui.set_chat_menu_open(false);
    ui.set_reply_message(0);
    ui.set_edit_message_id(0);
    ui.set_draft(Default::default());
    ui.set_message_busy(false);
    ui.set_network_status(Default::default());
    ui.set_active_chat(Demo::demo_chats()[0].row.clone());
    let text = "Жирный · курсив · подчёркнутый · зачёркнутый\nМоноширинный код: cargo run --locked\nСсылка на Telegram\n\nПробелы и **буквальные звёздочки** сохраняются.";
    let mut entities = Vec::new();
    for (part, kind) in [
        ("Жирный", 0),
        ("курсив", 1),
        ("подчёркнутый", 2),
        ("зачёркнутый", 3),
        ("cargo run --locked", 4),
        ("Ссылка на Telegram", 5),
    ] {
        let start = text.find(part).unwrap();
        let offset = text[..start].encode_utf16().count() as i32;
        let length = part.encode_utf16().count() as i32;
        entities.push(match kind {
            0 => MessageEntity::Bold(types::MessageEntityBold { offset, length }),
            1 => MessageEntity::Italic(types::MessageEntityItalic { offset, length }),
            2 => MessageEntity::Underline(types::MessageEntityUnderline { offset, length }),
            3 => MessageEntity::Strike(types::MessageEntityStrike { offset, length }),
            4 => MessageEntity::Code(types::MessageEntityCode { offset, length }),
            _ => MessageEntity::TextUrl(types::MessageEntityTextUrl {
                offset,
                length,
                url: "https://telegram.org".into(),
            }),
        });
    }
    let mut row = message(text, "10:22", false);
    row.id = 71;
    row.date = "10 ноября".into();
    row.has_formatting = true;
    row.formatted_body = rich_text::format(text, &entities, 0);
    ui.set_messages(ModelRc::new(VecModel::from(vec![row])));
    let opened = Rc::new(RefCell::new(String::new()));
    let links = Rc::clone(&opened);
    ui.global::<Theme>()
        .on_open_link(move |url| *links.borrow_mut() = url.to_string());
    for (name, width, scale, dark) in [
        ("formatted-dark", 1100, 1.0, true),
        ("formatted-light", 1100, 1.0, false),
        ("formatted-150", 1100, 1.5, true),
        ("formatted-compact", 430, 1.0, true),
    ] {
        ui.global::<Theme>().set_dark(dark);
        ui.global::<Theme>().set_context_pointer(false);
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        window.window.set_size(PhysicalSize::new(
            (width as f32 * scale) as u32,
            (720.0 * scale) as u32,
        ));
        for _ in 0..3 {
            window.advance(500);
            window.take_snapshot().unwrap();
        }
        assert!(
            (ui.get_composer_height() - 46.0).abs() <= 1.0,
            "empty composer must retain Telegram's 46px height"
        );
        let pixels = window.take_snapshot().unwrap();
        save_animation_frame(name, &pixels);
        if name == "formatted-dark" {
            let link_pixel = (90..650)
                .find_map(|y| {
                    (420..1050).find_map(|x| {
                        let p = pixels.as_slice()[y * 1100 + x];
                        ([p.r, p.g, p.b] == [82, 136, 193]).then_some((x, y))
                    })
                })
                .expect("formatted link must use Telegram's link color");
            let position = LogicalPosition::new(link_pixel.0 as f32, link_pixel.1 as f32);
            for event in [
                WindowEvent::PointerPressed {
                    position,
                    button: slint::platform::PointerEventButton::Left,
                },
                WindowEvent::PointerReleased {
                    position,
                    button: slint::platform::PointerEventButton::Left,
                },
            ] {
                window.dispatch_event(event);
            }
            assert_eq!(
                &*opened.borrow(),
                "https://telegram.org",
                "native link must receive clicks through the message surface"
            );
            ui.global::<Theme>().set_context_pointer(true);
            window.dispatch_event(WindowEvent::PointerPressed {
                position,
                button: slint::platform::PointerEventButton::Right,
            });
            window.dispatch_event(WindowEvent::PointerReleased {
                position,
                button: slint::platform::PointerEventButton::Right,
            });
            assert_eq!(
                ui.get_selected_message(),
                71,
                "formatted messages must retain their context menu"
            );
            ui.set_chat_menu_open(false);
            ui.global::<Theme>().set_context_pointer(false);
        }
    }
    ui.set_draft("Первая строка\nВторая строка\nТретья строка".into());
    window.advance(500);
    window.take_snapshot().unwrap();
    assert!(
        ui.get_composer_height() > 70.0,
        "composer must grow for line breaks"
    );
    let multiline = window.take_snapshot().unwrap();
    assert!(
        (686..713).any(|y| (396..416).any(|x| {
            let p = multiline.as_slice()[y * 430 + x];
            [p.r, p.g, p.b] == [82, 136, 193]
        })),
        "send button must stay at the bottom of a multiline composer"
    );
    save_animation_frame("multiline-composer", &multiline);
    ui.invoke_focus_composer();
    window.dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Shift.into(),
    });
    window.dispatch_event(WindowEvent::KeyPressed {
        text: slint::platform::Key::Return.into(),
    });
    window.dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Return.into(),
    });
    window.dispatch_event(WindowEvent::KeyReleased {
        text: slint::platform::Key::Shift.into(),
    });
    assert_eq!(
        ui.get_draft().matches('\n').count(),
        3,
        "Shift+Enter inserts a line break"
    );
    ui.set_draft("Очень длинный черновик\n".repeat(30).into());
    window.take_snapshot().unwrap();
    assert!(
        ui.get_composer_height() <= 166.0,
        "long drafts must scroll inside a bounded composer"
    );
    ui.set_draft(Default::default());
    window.take_snapshot().unwrap();
    assert!((ui.get_composer_height() - 46.0).abs() <= 1.0);
}

fn verify_network_burst_yields(window: &PreviewWindow, ui: &Desktop) {
    let (bridge, _commands, events) = telegram::TelegramBridge::offline_for_rendering();
    let state = Rc::new(RefCell::new(Demo::with_bridge(bridge)));
    let (weak, callback_state) = (ui.as_weak(), state.clone());
    ui.on_network_events(move || {
        if let Some(ui) = weak.upgrade() {
            callback_state.borrow_mut().poll_telegram(&ui);
        }
    });
    for index in 0..130 {
        events
            .send(telegram::TgEvent::Error {
                message: format!("burst-{index}"),
            })
            .unwrap();
    }
    state.borrow_mut().poll_telegram(ui);
    assert_ne!(
        ui.get_network_status(),
        "burst-129",
        "one UI turn must not drain a large event burst"
    );
    for _ in 0..130 {
        window.advance(2);
        if ui.get_network_status() == "burst-129" {
            break;
        }
    }
    assert_eq!(
        ui.get_network_status(),
        "burst-129",
        "deferred batches must preserve the tail of the queue"
    );
    window.advance(2);
    ui.on_network_events(|| {});
}

// Work-count assertions are deterministic on shared CI runners. Snapshot
// timings are not a GPU/DWM frame-rate measurement and must not gate CI.
fn verify_maximized_history_work(window: &PreviewWindow, ui: &Desktop) {
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.window.set_size(PhysicalSize::new(1920, 1040));
    ui.set_auth_required(false);
    ui.set_window_visible(true);
    ui.set_has_chat(true);
    ui.set_show_chat(true);
    ui.set_chat_menu_open(false);
    ui.set_emoji_open(false);
    ui.set_draft(Default::default());
    let requested = Rc::new(RefCell::new(Vec::new()));
    let calls = requested.clone();
    ui.on_request_details(move |id| calls.borrow_mut().push(id));
    let rows = Rc::new(VecModel::from(
        (1..=400)
            .map(|id| {
                let mut row = message(
                    &format!("Сообщение {id}: проверка длинной истории"),
                    "12:00",
                    id % 2 == 0,
                );
                row.id = id;
                row.date = "10 ноября".into();
                row
            })
            .collect::<Vec<_>>(),
    ));
    ui.set_messages(ModelRc::from(rows.clone()));
    window.take_snapshot().unwrap();
    ui.invoke_scroll_to_bottom();
    let settle = || {
        for _ in 0..4 {
            window.advance(100);
            window.take_snapshot().unwrap();
        }
    };
    settle();
    assert!(
        requested.borrow().is_empty(),
        "plain text must not trigger detail scans"
    );

    // One unresolved quote above the viewport and one at its bottom. Keep
    // requests unresolved so a repeating idle timer cannot hide behind caching.
    for index in [0, 399] {
        let mut row = rows.row_data(index).unwrap();
        row.reply_to = 9000;
        rows.set_row_data(index, row);
    }
    window.take_snapshot().unwrap();
    ui.invoke_scroll_to_bottom();
    settle();
    assert!(requested.borrow().contains(&400));
    assert!(
        !requested.borrow().contains(&1),
        "offscreen details must stay lazy"
    );
    let at_rest = requested.borrow().len();
    for _ in 0..12 {
        window.advance(80);
        window.take_snapshot().unwrap();
    }
    assert_eq!(
        requested.borrow().len(),
        at_rest,
        "idle history must not poll every 80ms"
    );

    requested.borrow_mut().clear();
    ui.set_history_scroll_y(0.0);
    settle();
    assert!(
        requested.borrow().contains(&1),
        "scrolling must request newly visible details"
    );
    assert!(!requested.borrow().contains(&400));
    save_animation_frame("maximized-history-top", &window.take_snapshot().unwrap());

    requested.borrow_mut().clear();
    ui.set_window_visible(false);
    ui.invoke_scroll_to_bottom();
    settle();
    assert!(
        requested.borrow().is_empty(),
        "hidden windows must not request details"
    );
    ui.set_window_visible(true);
    settle();
    assert!(
        requested.borrow().contains(&400),
        "restoring the window must rescan"
    );
    requested.borrow_mut().clear();
    window.window.set_size(PhysicalSize::new(1920, 1000));
    settle();
    assert!(
        !requested.borrow().is_empty(),
        "resizing a long history must rescan even when content height is unchanged"
    );

    let mut last = rows.row_data(399).unwrap();
    last.reply_resolved = true;
    rows.set_row_data(399, last);
    settle();
    requested.borrow_mut().clear();
    ui.set_history_scroll_y(ui.get_history_scroll_y() + 1.0);
    settle();
    assert!(
        requested.borrow().is_empty(),
        "resolved details must not be requested again"
    );
    ui.on_request_details(|_| {});
}

fn verify_profiles(window: &PreviewWindow, ui: &Desktop) {
    ui.set_auth_required(false);
    ui.set_settings_open(false);
    ui.set_menu_open(false);
    ui.set_profile_key("test:profile".into());
    ui.set_profile_name("Анна Смирнова".into());
    ui.set_profile_initials("АС".into());
    ui.set_profile_presence("в сети".into());
    ui.set_profile_online(true);
    ui.set_profile_is_user(true);
    ui.set_profile_phone("+7 000 000 00 00".into());
    ui.set_profile_username("@profile_test".into());
    ui.set_profile_about(
        "Тестовый профиль: длинное описание с переносом строки.\nТекст остаётся внутри панели."
            .into(),
    );
    ui.set_profile_birthday("15 мая".into());
    ui.set_profile_status(Default::default());
    ui.set_profile_gifts_status(Default::default());
    ui.set_profile_mute_ready(true);
    ui.set_profile_has_photo(true);
    let photo = slint_image(&image::RgbaImage::from_pixel(
        80,
        80,
        image::Rgba([220, 90, 40, 255]),
    ));
    ui.set_profile_photo(photo.clone());
    ui.set_profile_gifts(ModelRc::new(VecModel::from(
        (0..12)
            .map(|i| GiftRow {
                key: format!("fixture:{i}").into(),
                title: format!("Подарок {}", i + 1).into(),
                detail: "100 Stars".into(),
                photo: photo.clone(),
                has_photo: i % 2 == 0,
                collectible: i % 3 == 1,
                center: Color::from_rgb(151, 194, 158),
                edge: Color::from_rgb(61, 122, 88),
                pattern_color: Color::from_rgb(30, 76, 49),
                badge: if i % 3 == 1 {
                    "1 из 5K".into()
                } else {
                    Default::default()
                },
                ..Default::default()
            })
            .collect::<Vec<_>>(),
    )));
    ui.global::<Theme>().set_show_gifts(true);
    ui.set_profile_open(true);
    for (name, scale, dark, width, height) in [
        ("profile-dark-100", 1.0, true, 1100, 720),
        ("profile-light-150", 1.5, false, 1100, 720),
        ("profile-dark-200", 2.0, true, 1100, 720),
        ("profile-maximized", 1.0, true, 1920, 1040),
        ("profile-short", 1.0, true, 1100, 480),
        ("profile-narrow", 1.0, true, 400, 720),
    ] {
        window.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale,
        });
        window.window.set_size(PhysicalSize::new(
            (width as f32 * scale) as u32,
            (height as f32 * scale) as u32,
        ));
        ui.global::<Theme>().set_dark(dark);
        ui.set_profile_scroll_y(0.0);
        window.take_snapshot().unwrap();
        window.advance(250);
        window.take_snapshot().unwrap();
        assert_eq!(
            ui.get_profile_header_y(),
            0.0,
            "{name}: header must be pinned to the panel's top, not centered"
        );
        assert_eq!(
            ui.get_profile_header_height(),
            236.0,
            "upstream expanded header"
        );
        let panel_width = 392.0_f32.min((width as f32 - 96.0).max(324.0));
        let columns = ((panel_width - 22.0 + 10.0) / 117.0).floor() as i32;
        assert_eq!(
            ui.get_profile_gift_columns(),
            columns,
            "Telegram gift column count"
        );
        assert_eq!(
            ui.get_profile_gift_width(),
            ((panel_width - 12.0) / columns as f32).floor() - 10.0,
            "Telegram gift width rounding"
        );
        // Caption dragging must work even with a modal profile above the chat.
        let drag_count = Rc::new(Cell::new(0));
        let calls = drag_count.clone();
        ui.on_drag_window(move || calls.set(calls.get() + 1));
        let caption = LogicalPosition::new(120.0, 13.0);
        window.dispatch_event(WindowEvent::PointerPressed {
            position: caption,
            button: slint::platform::PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position: caption,
            button: slint::platform::PointerEventButton::Left,
        });
        assert_eq!(
            drag_count.get(),
            1,
            "profile must not consume the native caption drag"
        );
        assert!(
            ui.get_profile_open(),
            "caption dragging must not dismiss the profile"
        );
        ui.on_drag_window(|| {});
        let pixels = window.take_snapshot().unwrap();
        save_animation_frame(name, &pixels);
        let center = ((width as f32 / 2.0) * scale) as usize;
        let y = ((40.0 + 24.0 + 40.0) * scale) as usize;
        let p = pixels.as_slice()[y * pixels.width() as usize + center];
        assert!(
            p.r > 180 && p.g < 130,
            "profile photo must render in upstream centered position"
        );
        for offset in [60.0, 112.0, 150.0, 180.0] {
            ui.set_profile_scroll_y(-offset);
            window.take_snapshot().unwrap();
            window.advance(1);
            assert_eq!(
                ui.get_profile_header_y(),
                0.0,
                "{name}: collapse must not move the header down"
            );
            assert_eq!(
                ui.get_profile_header_height(),
                (236.0_f32 - offset).max(56.0)
            );
        }
        save_animation_frame(
            &format!("{name}-collapsed"),
            &window.take_snapshot().unwrap(),
        );
        // The close button must remain clickable at the top after collapse.
        let panel_width = 392.0_f32.min((width as f32 - 96.0).max(324.0));
        let position = LogicalPosition::new((width as f32 + panel_width) / 2.0 - 28.0, 65.0);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: slint::platform::PointerEventButton::Left,
        });
        assert!(
            !ui.get_profile_open(),
            "{name}: top-right close button must dismiss the collapsed profile"
        );
        ui.set_profile_open(true);
        window.take_snapshot().unwrap();
        window.advance(250);
        assert_eq!(
            ui.get_profile_header_height(),
            236.0,
            "reopening resets the header"
        );
    }
    ui.global::<Theme>().set_show_gifts(false);
    ui.set_profile_scroll_y(0.0);
    window.take_snapshot().unwrap();
    window.advance(10);
    save_animation_frame("profile-gifts-hidden", &window.take_snapshot().unwrap());
    ui.set_profile_is_self(true);
    save_animation_frame("profile-self", &window.take_snapshot().unwrap());
    ui.set_profile_open(false);
    ui.set_profile_has_photo(false);
    ui.set_profile_gifts(Default::default());
    ui.set_profile_is_self(false);
    ui.global::<Theme>().set_show_gifts(true);
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor: 1.0 });
    window.window.set_size(PhysicalSize::new(1100, 720));
    window.advance(250);
}

fn verify_tray_resource_release(window: &PreviewWindow, ui: &Desktop) {
    let (bridge, _commands, events) = telegram::TelegramBridge::offline_for_rendering();
    let mut state = Demo::with_bridge(bridge);
    ui.set_window_visible(true);
    ui.set_draft("Черновик остаётся при сворачивании".into());
    state.refresh_chats(ui);
    state.refresh_history(ui);
    let before = state.chats[0].messages.row_count();
    let mut row = state.chats[0].messages.row_data(0).unwrap();
    row.preview = slint_image(&image::RgbaImage::from_pixel(
        64,
        64,
        image::Rgba([100, 120, 140, 255]),
    ));
    row.has_preview = true;
    let mid = row.id;
    state.chats[0].messages.set_row_data(0, row);
    state.preview_bytes = 64 * 64 * 4;
    state.suspend_ui(ui);
    window.take_snapshot().unwrap();
    assert_eq!(
        ui.get_messages().row_count(),
        0,
        "tray must unload history delegates"
    );
    assert_eq!(
        ui.get_chats().row_count(),
        0,
        "tray must unload chat delegates"
    );
    assert_eq!(state.preview_bytes, 0);
    assert!(!state.chats[0].messages.row_data(0).unwrap().has_preview);
    assert_eq!(
        state.chats[0].messages.row_count(),
        before,
        "messages are application state, not disposable UI"
    );
    events
        .send(telegram::TgEvent::Thumbnail {
            peer_id: state.chats[0].row.id,
            message_id: mid,
            key: String::new(),
            pixels: Some(image::RgbaImage::from_pixel(
                64,
                64,
                image::Rgba([100, 120, 140, 255]),
            )),
        })
        .unwrap();
    state.poll_telegram(ui);
    state.refresh_chats(ui);
    state.refresh_history(ui);
    assert_eq!(
        ui.get_messages().row_count(),
        0,
        "background work must not rebuild hidden delegates"
    );
    assert_eq!(
        state.preview_bytes, 0,
        "late media must not refill the cache in tray"
    );
    state.resume_ui(ui);
    window.take_snapshot().unwrap();
    window.advance(1);
    assert_eq!(ui.get_messages().row_count(), before);
    assert_eq!(ui.get_draft(), "Черновик остаётся при сворачивании");
    ui.set_draft(Default::default());
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
