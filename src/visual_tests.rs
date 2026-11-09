//! Render the real interface without a desktop session, files or Telegram login.
use super::*;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, PhysicalSize, Rgb8Pixel, SharedPixelBuffer};

struct PreviewPlatform(Rc<MinimalSoftwareWindow>);

impl Platform for PreviewPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.0.clone())
    }
}

#[test]
fn portable_interface_renders_icons_at_multiple_scales() {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(PreviewPlatform(window.clone()))).unwrap();
    let ui = Desktop::new().unwrap();
    install_rendering(&ui);
    let chats = Demo::demo_chats();
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

    for (name, width, scale, dark, channel) in [
        ("desktop-100", 1100, 1.0, true, false),
        ("desktop-150", 1100, 1.5, true, false),
        ("desktop-200", 1100, 2.0, true, false),
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
        window.set_size(PhysicalSize::new(physical_width, physical_height));
        slint::platform::update_timers_and_animations();
        let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(physical_width, physical_height);
        // The first pass resolves the list's item sizes; the second positions
        // the short conversation against the bottom of the viewport.
        for _ in 0..2 {
            window.request_redraw();
            assert!(window.draw_if_needed(|renderer| {
                renderer.render(pixels.make_mut_slice(), physical_width as usize);
            }));
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
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.render(pixels.make_mut_slice(), physical_width as usize);
            });
        }
        if channel {
            let mut incoming = 0;
            let mut outgoing = 0;
            for y in 80..674 {
                for x in 398..1100 {
                    let pixel = pixels.as_slice()[y * physical_width as usize + x];
                    incoming += usize::from(
                        pixel
                            == Rgb8Pixel {
                                r: 24,
                                g: 37,
                                b: 51,
                            },
                    );
                    outgoing += usize::from(
                        pixel
                            == Rgb8Pixel {
                                r: 43,
                                g: 82,
                                b: 120,
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
                image::ColorType::Rgb8,
            )
            .unwrap();
        }
    }
    ui.hide().unwrap();
}
