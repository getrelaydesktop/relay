#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{cell::RefCell, rc::Rc, time::Duration};

use slint::winit_030::{WinitWindowAccessor, winit::window::ResizeDirection};
use slint::{Color, ComponentHandle, Model, ModelRc, Timer, VecModel};

slint::include_modules!();
mod attachments;
mod composer;
mod details;
mod preferences;
mod presence;
mod profiles;
mod qr;
mod render_assets;
mod rich_text;
mod sending;
mod telegram;
#[cfg(windows)]
mod tray;
#[cfg(test)]
mod visual_tests;

const AVATAR_PALETTES: [((u8, u8, u8), (u8, u8, u8)); 7] = [
    ((255, 136, 94), (228, 82, 70)),
    ((255, 205, 106), (255, 168, 92)),
    ((174, 148, 250), (102, 95, 255)),
    ((160, 222, 126), (84, 203, 104)),
    ((83, 237, 214), (40, 201, 183)),
    ((114, 213, 253), (42, 158, 241)),
    ((224, 162, 243), (214, 105, 237)),
];

struct Chat {
    row: ChatRow,
    messages: Rc<VecModel<MessageRow>>,
    draft: slint::SharedString,
    formatted_draft: composer::Draft,
    editor_history: composer::History,
}

struct Demo {
    detail_requests: std::collections::HashSet<(i32, i32)>,
    preview_lru: std::collections::VecDeque<(i32, i32, usize)>,
    preview_bytes: usize,
    chats: Vec<Chat>,
    chat_model: Rc<VecModel<ChatRow>>,
    folder_model: Rc<VecModel<FolderRow>>,
    active: usize,
    folder: i32,
    folders: Vec<telegram::TgFolder>,
    viewed_photo: Option<(i32, i32)>,
    live_mode: bool,
    next_local_id: i32,
    pending_send: Option<sending::PendingSend>,
    network_poll: Timer,
    bridge: telegram::TelegramBridge,
}

impl Demo {
    fn show_photo(&mut self, ui: &Desktop, peer_id: i32, mid: i32) {
        let Some(chat) = self.chats.iter().find(|chat| chat.row.id == peer_id) else {
            return;
        };
        let photos: Vec<_> = chat
            .messages
            .iter()
            .filter(|message| message.is_photo)
            .collect();
        let Some(index) = photos.iter().position(|message| message.id == mid) else {
            return;
        };
        self.viewed_photo = Some((peer_id, mid));
        ui.set_photo_title(chat.row.title.clone());
        ui.set_photo_index(index as i32 + 1);
        ui.set_photo_count(photos.len() as i32);
        ui.set_photo_image(Default::default());
        ui.set_photo_zoom(1.0);
        ui.set_photo_status("Загрузка фото…".into());
        ui.set_photo_open(true);
        ui.invoke_focus_photo();
        self.bridge.view_photo(peer_id, mid);
    }
    fn new() -> Self {
        Self::with_bridge(telegram::TelegramBridge::spawn())
    }

    fn with_bridge(bridge: telegram::TelegramBridge) -> Self {
        Self {
            detail_requests: Default::default(),
            preview_lru: Default::default(),
            preview_bytes: 0,
            chats: Self::demo_chats(),
            chat_model: Rc::new(VecModel::default()),
            folder_model: Rc::new(VecModel::default()),
            active: 0,
            folder: 0,
            folders: vec![],
            viewed_photo: None,
            live_mode: false,
            next_local_id: -1,
            pending_send: None,
            network_poll: Timer::default(),
            bridge,
        }
    }

    fn demo_chats() -> Vec<Chat> {
        vec![
            Chat::new(
                0,
                "Анна",
                "А",
                (174, 148, 250),
                (102, 95, 255),
                0,
                2,
                &[
                    ("Привет! Завтра к десяти удобно?", "10:18", false),
                    ("Да, давай. Возьму ноутбук.", "10:20", true),
                    ("Договорились, встречаемся у входа.", "10:21", false),
                ],
            ),
            Chat::new(
                1,
                "Команда",
                "К",
                (83, 237, 214),
                (40, 201, 183),
                2,
                1,
                &[
                    ("Маша: Залила свежий макет в общую папку.", "09:43", false),
                    ("Илья: Посмотрю до обеда.", "09:45", false),
                ],
            ),
            Chat::new(
                2,
                "Алексей",
                "А",
                (255, 136, 94),
                (228, 82, 70),
                1,
                2,
                &[("Ты нашёл переходник?", "09:17", false)],
            ),
            Chat::new(
                3,
                "Избранное",
                "★",
                (114, 213, 253),
                (42, 158, 241),
                0,
                2,
                &[("Забрать заказ в четверг", "08:34", true)],
            ),
        ]
    }

    fn matching(&self, query: &str) -> impl Iterator<Item = &Chat> {
        let query = query.trim().to_lowercase();
        let folder = self.folder;
        let members = self.folders.iter().find(|f| f.id == folder);
        self.chats.iter().filter(move |chat| {
            (folder == 0
                || if self.live_mode {
                    members.is_some_and(|f| f.chat_ids.contains(&chat.row.id))
                } else {
                    chat.row.folder == folder
                })
                && chat.row.title.to_lowercase().contains(&query)
        })
    }

    fn refresh_chats(&self, ui: &Desktop) {
        let mut rows: Vec<_> = self
            .matching(&ui.get_query())
            .map(|chat| chat.row.clone())
            .collect();
        if self.live_mode && self.folder != 0 {
            if let Some(folder) = self.folders.iter().find(|f| f.id == self.folder) {
                rows.sort_by_key(|chat| {
                    folder
                        .chat_ids
                        .iter()
                        .position(|id| *id == chat.id)
                        .unwrap_or(usize::MAX)
                });
            }
        }
        update_rows(&self.chat_model, rows);
        ui.set_chats(ModelRc::from(Rc::clone(&self.chat_model)));
        let folders: Vec<_> = if self.live_mode {
            self.folders
                .iter()
                .map(|folder| FolderRow {
                    id: folder.id,
                    title: folder.title.clone().into(),
                    unread: self
                        .chats
                        .iter()
                        .filter(|chat| {
                            chat.row.unread > 0
                                && (folder.id == 0 || folder.chat_ids.contains(&chat.row.id))
                        })
                        .count() as i32,
                })
                .collect()
        } else {
            [(0, "All chats"), (1, "Work"), (2, "Personal")]
                .into_iter()
                .map(|(id, title)| FolderRow {
                    id,
                    title: title.into(),
                    unread: self
                        .chats
                        .iter()
                        .filter(|c| c.row.unread > 0 && (id == 0 || c.row.folder == id))
                        .count() as i32,
                })
                .collect()
        };
        update_rows(&self.folder_model, folders);
        ui.set_folders(ModelRc::from(Rc::clone(&self.folder_model)));
    }

    fn refresh_history(&self, ui: &Desktop) {
        if self.chats.is_empty() {
            ui.set_messages(ModelRc::new(VecModel::<MessageRow>::default()));
            ui.set_active_chat(ChatRow::default());
            return;
        }
        let idx = self.active.min(self.chats.len() - 1);
        let chat = &self.chats[idx];
        // Resolve once when opening/refreshing history, not once per visible
        // row in the deferred thumbnail scan.
        details::resolve_local(&chat.messages, &chat.row.title);
        ui.set_active_chat(chat.row.clone());
        ui.set_messages(ModelRc::from(Rc::clone(&chat.messages)));
        let weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_scroll_to_bottom();
            }
        });
    }

    fn select_folder(&mut self, ui: &Desktop, folder: i32) {
        self.folder = folder;
        ui.set_active_folder(folder);
        self.refresh_chats(ui);
    }

    fn open(&mut self, ui: &Desktop, id: i32) {
        self.open_with_history(ui, id, true);
    }
    fn open_with_history(&mut self, ui: &Desktop, id: i32, load_history: bool) {
        let Some(index) = self.chats.iter().position(|chat| chat.row.id == id) else {
            return;
        };
        if !self.chats.is_empty() && self.active < self.chats.len() {
            self.chats[self.active].draft = ui.get_draft();
        }
        self.active = index;
        self.viewed_photo = None;
        ui.set_photo_open(false);
        ui.set_selected_message(0);
        ui.set_chat_menu_open(false);
        ui.set_reply_message(0);
        ui.set_edit_message_id(0);
        ui.set_chat_query(Default::default());
        ui.set_chat_search_open(false);
        self.chats[index].row.unread = 0;
        ui.set_draft(self.chats[index].draft.clone());
        self.chats[index]
            .formatted_draft
            .edit(ui.get_draft().as_str());
        composer::preview(ui, &self.chats[index].formatted_draft);
        ui.set_file_open(false);
        ui.set_editor_menu_open(false);
        ui.set_link_editor_open(false);
        ui.set_large_editor_open(false);
        ui.set_attachments(Default::default());
        ui.set_show_chat(true);
        ui.set_has_chat(true);
        if self.live_mode && load_history {
            self.bridge.load_history(id);
            self.bridge.read(id);
        }
        self.refresh_chats(ui);
        self.refresh_history(ui);
        ui.invoke_focus_composer();
    }

    fn send(&mut self, ui: &Desktop) {
        if ui.get_message_busy() || self.pending_send.is_some() {
            return;
        }
        let draft = ui.get_draft();
        let body = draft.as_str();
        if body.trim().is_empty() || self.chats.is_empty() {
            return;
        }
        let idx = self.active.min(self.chats.len() - 1);
        let peer_id = self.chats[idx].row.id;
        self.chats[idx].formatted_draft.edit(body);
        let entities = self.chats[idx].formatted_draft.entities.clone();
        if matches!(self.chats[idx].row.channel_access, 1 | 3) {
            return;
        }
        if self.live_mode && ui.get_edit_message_id() > 0 {
            ui.set_message_busy(true);
            ui.set_network_status("Отправка…".into());
            self.bridge
                .edit_formatted(peer_id, ui.get_edit_message_id(), body, entities);
            return;
        }
        let token = self.next_local_id;
        self.next_local_id -= 1;
        let reply = (ui.get_reply_message() > 0).then(|| ui.get_reply_message());
        let now = chrono::Local::now();
        let mut row = message(body, &now.format("%H:%M").to_string(), true);
        row.has_formatting = !entities.is_empty();
        row.formatted_body = rich_text::format(body, &entities, 0);
        row.id = if self.live_mode { token } else { -token };
        row.date = now.format("%d.%m.%Y").to_string().into();
        row.send_token = token;
        row.reply_to = reply.unwrap_or_default();
        row.sender = "Вы".into();
        row.entities_json = composer::encode(&entities).into();
        row.reply_author = ui.get_reply_author();
        row.reply_body = ui.get_reply_body();
        row.reply_resolved = reply.is_some();
        row.pending = self.live_mode;
        row.channel_post = self.chats[idx].row.channel_access == 2;
        let chat = &mut self.chats[idx];
        chat.row.last_message_id = row.id;
        chat.row.time = row.time.clone();
        chat.messages.push(row);
        chat.row.preview = body.into();
        chat.row.outgoing = true;
        chat.draft = Default::default();
        chat.formatted_draft = Default::default();
        ui.set_draft(Default::default());
        composer::preview(ui, &composer::Draft::default());
        ui.set_reply_message(0);
        ui.invoke_animate_sent_message();
        self.refresh_chats(ui);
        self.refresh_history(ui);
        if self.live_mode {
            self.pending_send = Some(sending::PendingSend {
                peer_id,
                local_id: token,
            });
            ui.set_text_send_pending(true);
            ui.set_message_busy(true);
            ui.set_network_status(Default::default());
            self.bridge.send_formatted(peer_id, body, reply, entities);
        }
    }

    fn retry_send(&mut self, ui: &Desktop, local_id: i32) {
        if !self.live_mode || ui.get_message_busy() || self.pending_send.is_some() {
            return;
        }
        let Some(chat) = self.chats.get(self.active) else {
            return;
        };
        if matches!(chat.row.channel_access, 1 | 3) {
            return;
        }
        let Some(index) = chat
            .messages
            .iter()
            .position(|row| row.id == local_id && row.failed)
        else {
            return;
        };
        let mut row = chat.messages.row_data(index).unwrap();
        row.pending = true;
        row.failed = false;
        let body = row.body.clone();
        let entities = composer::decode(row.entities_json.as_str());
        let reply = (row.reply_to > 0).then_some(row.reply_to);
        chat.messages.set_row_data(index, row);
        let peer_id = chat.row.id;
        self.pending_send = Some(sending::PendingSend { peer_id, local_id });
        ui.set_text_send_pending(true);
        ui.set_message_busy(true);
        ui.set_network_status(Default::default());
        self.bridge.send_formatted(peer_id, &body, reply, entities);
    }

    fn poll_telegram(&mut self, ui: &Desktop) {
        self.network_poll.stop();
        self.bridge.begin_poll();
        let started = std::time::Instant::now();
        let mut handled = 0;
        let mut avatars_changed = false;
        loop {
            // Yield between events so a burst of background results cannot
            // monopolize the UI thread. One expensive event can still exceed
            // this soft budget; the queue is never dropped or reordered.
            if handled >= 32 || (handled > 0 && started.elapsed() >= Duration::from_millis(4)) {
                let weak = ui.as_weak();
                self.network_poll.start(
                    slint::TimerMode::SingleShot,
                    Duration::from_millis(1),
                    move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.invoke_network_events();
                        }
                    },
                );
                break;
            }
            let Some(ev) = self.bridge.try_recv() else {
                break;
            };
            handled += 1;
            match ev {
                telegram::TgEvent::ProfileReady { key, profile } => {
                    if key == "self" {
                        if let Some(photo) = &profile.photo {
                            ui.set_user_avatar(slint_image(photo));
                            ui.set_user_has_avatar(true);
                        }
                    }
                    if ui.get_profile_key() == key {
                        ui.set_profile_is_user(profile.is_user);
                        ui.set_profile_is_self(profile.is_self);
                        ui.set_profile_is_bot(profile.is_bot);
                        ui.set_profile_online(profile.online);
                        ui.set_profile_birthday(profile.birthday.into());
                        if let Some(muted) = profile.notifications_muted {
                            if !ui.get_profile_action_busy() {
                                ui.set_profile_muted(muted);
                            }
                            ui.set_profile_mute_ready(true);
                        }
                        let link = if !profile.username.is_empty() {
                            format!("https://t.me/{}", profile.username.trim_start_matches('@'))
                        } else if profile.is_self && !profile.phone.is_empty() {
                            format!("https://t.me/{}", profile.phone)
                        } else {
                            String::new()
                        };
                        ui.set_profile_qr_matrix(if link.is_empty() {
                            Default::default()
                        } else {
                            qr::encode_qr_matrix(&link).into()
                        });
                        ui.set_profile_link(link.into());
                        ui.set_profile_name(profile.name.into());
                        ui.set_profile_initials(profile.initials.into());
                        ui.set_profile_presence(profile.presence.into());
                        ui.set_profile_username(profile.username.into());
                        ui.set_profile_phone(profile.phone.into());
                        ui.set_profile_about(profile.about.into());
                        if let Some(photo) = profile.photo {
                            ui.set_profile_photo(slint_image(&photo));
                            ui.set_profile_has_photo(true);
                        }
                        ui.set_profile_status(Default::default());
                    }
                }
                telegram::TgEvent::GiftsReady {
                    key,
                    gifts,
                    next,
                    append,
                } => {
                    if ui.get_profile_key() == key {
                        ui.set_profile_busy(false);
                    }
                    if ui.get_profile_key() == key && ui.global::<Theme>().get_show_gifts() {
                        let mut rows: Vec<_> = if append {
                            ui.get_profile_gifts().iter().collect()
                        } else {
                            vec![]
                        };
                        rows.extend(gifts.into_iter().map(|g| GiftRow {
                            key: g.key.into(),
                            title: g.title.into(),
                            detail: g.detail.into(),
                            url: g.url.into(),
                            has_photo: g.photo.is_some(),
                            photo: g.photo.as_ref().map(slint_image).unwrap_or_default(),
                        }));
                        let empty = rows.is_empty();
                        ui.set_profile_gifts(ModelRc::new(VecModel::from(rows)));
                        ui.set_profile_gifts_status(if empty {
                            "Нет публичных подарков".into()
                        } else {
                            Default::default()
                        });
                        ui.set_profile_next(next.into());
                        ui.set_profile_busy(false);
                    }
                }
                telegram::TgEvent::GiftPreview {
                    key,
                    gift_key,
                    pixels,
                } => {
                    if ui.get_profile_key() == key && ui.global::<Theme>().get_show_gifts() {
                        let model = ui.get_profile_gifts();
                        let image = slint_image(&pixels);
                        for (i, mut row) in model.iter().enumerate() {
                            if row.key == gift_key {
                                row.photo = image.clone();
                                row.has_photo = true;
                                model.set_row_data(i, row);
                            }
                        }
                    }
                }
                telegram::TgEvent::GiftsFailed { key, message } => {
                    if ui.get_profile_key() == key {
                        ui.set_profile_busy(false);
                        ui.set_profile_gifts_status(
                            format!("Не удалось загрузить подарки: {message}").into(),
                        );
                    }
                }
                telegram::TgEvent::ProfileMuteResult {
                    key,
                    muted,
                    message,
                } => {
                    if ui.get_profile_key() == key {
                        ui.set_profile_action_busy(false);
                        if let Some(muted) = muted {
                            ui.set_profile_muted(muted);
                        } else {
                            ui.set_profile_status(
                                format!("Не удалось изменить уведомления: {message}").into(),
                            );
                        }
                    }
                }
                telegram::TgEvent::ProfileFailed { key, message } => {
                    if ui.get_profile_key() == key {
                        ui.set_profile_status(
                            format!("Не удалось загрузить данные: {message}").into(),
                        );
                        ui.set_profile_busy(false);
                    }
                }
                telegram::TgEvent::FileSent { path } => {
                    let rows = ui
                        .get_attachments()
                        .iter()
                        .filter(|r| std::path::Path::new(r.path.as_str()) != path)
                        .collect::<Vec<_>>();
                    ui.set_attachments(ModelRc::new(VecModel::from(rows)));
                    ui.set_attachment_caption(Default::default());
                }
                telegram::TgEvent::FilesSent => {
                    ui.set_upload_busy(false);
                    ui.set_message_busy(false);
                    ui.set_attachments(Default::default());
                    ui.set_attachment_caption(Default::default());
                    ui.set_network_status("Файлы отправлены".into());
                }
                telegram::TgEvent::Presence {
                    peer_id,
                    text,
                    online,
                } => {
                    if let Some(chat) = self.chats.iter_mut().find(|chat| chat.row.id == peer_id) {
                        chat.row.presence = text.into();
                        chat.row.online = online;
                        if ui.get_active_chat().id == peer_id {
                            ui.set_active_chat(chat.row.clone());
                        }
                    }
                }
                telegram::TgEvent::Thumbnail {
                    peer_id,
                    message_id,
                    key,
                    pixels,
                } => {
                    self.detail_requests.remove(&(peer_id, message_id));
                    if let Some(chat) = self.chats.iter().find(|chat| chat.row.id == peer_id) {
                        if let Some(index) =
                            chat.messages.iter().position(|row| row.id == message_id)
                        {
                            let mut row = chat.messages.row_data(index).unwrap();
                            if !key.is_empty() && row.photo_key != key {
                                continue;
                            }
                            row.preview_failed = pixels.is_none();
                            if let Some(pixels) = pixels {
                                let bytes = pixels.len();
                                row.preview = slint_image(&pixels);
                                row.has_preview = true;
                                let mut freed = 0;
                                self.preview_lru.retain(|&(peer, mid, bytes)| {
                                    if peer == peer_id && mid == message_id {
                                        freed += bytes;
                                        false
                                    } else {
                                        true
                                    }
                                });
                                self.preview_bytes = self.preview_bytes.saturating_sub(freed);
                                self.preview_lru.push_back((peer_id, message_id, bytes));
                                self.preview_bytes += bytes;
                            }
                            chat.messages.set_row_data(index, row);
                        }
                    }
                    while self.preview_bytes > 24 * 1024 * 1024 {
                        let Some((peer, mid, bytes)) = self.preview_lru.pop_front() else {
                            break;
                        };
                        self.preview_bytes = self.preview_bytes.saturating_sub(bytes);
                        if let Some(chat) = self.chats.iter().find(|chat| chat.row.id == peer) {
                            if let Some(index) = chat.messages.iter().position(|row| row.id == mid)
                            {
                                let mut row = chat.messages.row_data(index).unwrap();
                                row.preview = Default::default();
                                row.has_preview = false;
                                chat.messages.set_row_data(index, row);
                            }
                        }
                    }
                }
                telegram::TgEvent::ReplyResolved {
                    peer_id,
                    message_id,
                    original,
                } => {
                    if let Some(chat) = self.chats.iter().find(|chat| chat.row.id == peer_id) {
                        if let Some(index) =
                            chat.messages.iter().position(|row| row.id == message_id)
                        {
                            let mut row = chat.messages.row_data(index).unwrap();
                            row.reply_resolved = true;
                            if let Some(original) = original {
                                row.reply_author = original.sender.into();
                                if !row.reply_quoted {
                                    row.reply_body = original.body.into();
                                }
                            } else if row.reply_body.is_empty() {
                                row.reply_body = "Сообщение удалено или недоступно".into();
                            }
                            chat.messages.set_row_data(index, row);
                        }
                    }
                }
                telegram::TgEvent::JumpReady {
                    peer_id,
                    message_id,
                    messages,
                } => {
                    if let Some(chat) = self.chats.iter().find(|chat| chat.row.id == peer_id) {
                        sending::merge_history(
                            &chat.messages,
                            messages.iter().map(live_message).collect(),
                        );
                        self.open_with_history(ui, peer_id, false);
                        let weak = ui.as_weak();
                        Timer::single_shot(Duration::ZERO, move || {
                            if let Some(ui) = weak.upgrade() {
                                ui.invoke_reveal_message(message_id);
                            }
                        });
                        ui.set_network_status(Default::default());
                    }
                }
                telegram::TgEvent::Qr { matrix } => {
                    let ready = !matrix.is_empty();
                    ui.set_auth_qr_matrix(matrix.into());
                    if ready && ui.get_auth_step() == 0 {
                        ui.set_auth_busy(false);
                        ui.set_auth_error(Default::default());
                    }
                }
                telegram::TgEvent::CodeSent { phone, delivery } => {
                    ui.set_auth_delivery(delivery.into());
                    ui.set_auth_phone(phone.into());
                    ui.set_auth_step(2);
                    ui.set_auth_busy(false);
                    ui.set_auth_error(Default::default());
                }
                telegram::TgEvent::NeedPassword { hint } => {
                    ui.set_auth_password_hint(hint.into());
                    ui.set_auth_step(3);
                    ui.set_auth_busy(false);
                    ui.set_auth_error(Default::default());
                }
                telegram::TgEvent::Authorized {
                    name,
                    phone,
                    username,
                    initials,
                    dialogs,
                    folders,
                } => {
                    self.live_mode = true;
                    self.detail_requests.clear();
                    self.preview_lru.clear();
                    self.preview_bytes = 0;
                    ui.set_show_chat(false);
                    self.folders = folders;
                    self.folder = 0;
                    ui.set_active_folder(0);
                    ui.set_connected(true);
                    ui.set_user_name(name.into());
                    ui.set_user_phone(phone.into());
                    ui.set_user_username(username.into());
                    ui.set_user_initials(initials.into());
                    ui.set_auth_busy(false);
                    ui.set_auth_error(Default::default());
                    ui.set_auth_required(false);
                    ui.set_auth_code(Default::default());
                    ui.set_auth_password(Default::default());

                    {
                        self.chats = dialogs
                            .into_iter()
                            .enumerate()
                            .map(|(i, d)| {
                                let (top, bot) = if d.saved || d.id == 3 {
                                    ((114, 213, 253), (42, 158, 241))
                                } else {
                                    AVATAR_PALETTES[i % AVATAR_PALETTES.len()]
                                };
                                let preview =
                                    d.messages.last().map(message_preview).unwrap_or_default();
                                let (time, outgoing) = d
                                    .messages
                                    .last()
                                    .map(|m| (m.time.as_str(), m.outgoing))
                                    .unwrap_or(("", false));
                                Chat {
                                    row: ChatRow {
                                        no_photo: d.no_photo,
                                        id: d.id,
                                        title: chat_preview(&d.title),
                                        initials: d.initials.into(),
                                        accent: Color::from_rgb_u8(bot.0, bot.1, bot.2),
                                        accent_top: Color::from_rgb_u8(top.0, top.1, top.2),
                                        preview,
                                        time: time.into(),
                                        unread: d.unread,
                                        outgoing,
                                        last_message_id: d.messages.last().map_or(0, |m| m.id),
                                        read_outbox_max_id: d.read_outbox_max_id,
                                        folder: d.folder,
                                        channel_access: d.channel as i32,
                                        presence: d.presence.into(),
                                        online: d.online,
                                        ..Default::default()
                                    },
                                    messages: Rc::new(VecModel::from(
                                        d.messages.iter().map(live_message).collect::<Vec<_>>(),
                                    )),
                                    draft: Default::default(),
                                    formatted_draft: Default::default(),
                                    editor_history: Default::default(),
                                }
                            })
                            .collect();
                        self.active = 0;
                        self.bridge.load_avatar(0);
                        ui.set_has_chat(!self.chats.is_empty());
                        self.refresh_chats(ui);
                        self.refresh_history(ui);
                        if let Some(chat) = self.chats.first() {
                            self.bridge.load_history(chat.row.id);
                        }
                    }
                }
                telegram::TgEvent::History { peer_id, messages } => {
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
                        let rows: Vec<MessageRow> =
                            messages.iter().map(live_message).collect::<Vec<_>>();
                        if let Some(last) = messages.last() {
                            chat.row.preview = message_preview(last);
                            chat.row.time = last.time.clone().into();
                            chat.row.outgoing = last.outgoing;
                            chat.row.last_message_id = last.id;
                        }
                        sending::merge_history(&chat.messages, rows);
                        self.refresh_chats(ui);
                        if self.chats.get(self.active).map(|c| c.row.id) == Some(peer_id) {
                            self.refresh_history(ui);
                        }
                    }
                }
                telegram::TgEvent::OutboxRead { peer_id, max_id } => {
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id)
                        && max_id > chat.row.read_outbox_max_id
                    {
                        chat.row.read_outbox_max_id = max_id;
                        if ui.get_active_chat().id == peer_id {
                            ui.set_active_chat(chat.row.clone());
                        }
                        self.refresh_chats(ui);
                    }
                }
                telegram::TgEvent::NewMessage { peer_id, message } => {
                    self.upsert_message(ui, peer_id, &message, true);
                }
                telegram::TgEvent::MessageSent { peer_id, message } => {
                    ui.set_text_send_pending(false);
                    if !ui.get_upload_busy() {
                        ui.set_message_busy(false);
                    }
                    ui.set_network_status(Default::default());
                    let pending = self.pending_send.take();
                    if let Some(pending) = &pending {
                        if pending.peer_id == peer_id {
                            if let Some(chat) = self.chats.iter().find(|c| c.row.id == peer_id) {
                                sending::acknowledge(
                                    &chat.messages,
                                    pending.local_id,
                                    live_message(&message),
                                );
                            }
                        }
                    }
                    ui.set_file_path(Default::default());
                    self.upsert_message(ui, peer_id, &message, false);
                }
                telegram::TgEvent::MessageEdited { peer_id, message } => {
                    if self
                        .chats
                        .iter()
                        .find(|c| c.row.id == peer_id)
                        .is_some_and(|c| c.messages.iter().any(|m| m.id == message.id))
                    {
                        self.upsert_message(ui, peer_id, &message, false);
                    }
                }
                telegram::TgEvent::MessagesDeleted { peer_id, ids } => {
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
                        let rows: Vec<_> = chat
                            .messages
                            .iter()
                            .filter(|m| !ids.contains(&m.id))
                            .collect();
                        chat.messages.set_vec(rows);
                    }
                    ui.set_selected_message(0);
                    self.refresh_history(ui);
                    self.refresh_chats(ui);
                }
                telegram::TgEvent::OlderHistory { peer_id, messages } => {
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
                        let existing: Vec<_> = chat.messages.iter().collect();
                        let mut rows: Vec<_> = messages
                            .iter()
                            .filter(|m| !existing.iter().any(|old| old.id == m.id))
                            .map(live_message)
                            .collect();
                        rows.extend(existing);
                        chat.messages.set_vec(rows);
                    }
                    ui.set_network_status(if messages.is_empty() {
                        "Начало истории".into()
                    } else {
                        Default::default()
                    });
                }
                telegram::TgEvent::SearchResults { peer_id, messages } => {
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
                        chat.messages
                            .set_vec(messages.iter().map(live_message).collect::<Vec<_>>());
                    }
                    ui.set_network_status(
                        format!(
                            "Найдено: {}. Refresh — вернуться к истории.",
                            messages.len()
                        )
                        .into(),
                    );
                }
                telegram::TgEvent::OperationDone { message } => {
                    ui.set_network_status(message.into());
                }
                telegram::TgEvent::EditCompleted {
                    peer_id,
                    message_id,
                } => {
                    ui.set_message_busy(false);
                    ui.set_network_status("Сообщение изменено".into());
                    if self.chats.get(self.active).map(|c| c.row.id) == Some(peer_id)
                        && ui.get_edit_message_id() == message_id
                    {
                        ui.set_edit_message_id(0);
                        ui.set_draft(Default::default());
                    }
                }
                telegram::TgEvent::SendFailed { message } => {
                    if ui.get_upload_busy() {
                        ui.set_file_open(true);
                    }
                    ui.set_upload_busy(false);
                    ui.set_text_send_pending(false);
                    ui.set_message_busy(false);
                    ui.set_network_status(message.into());
                    if let Some(pending) = self.pending_send.take() {
                        if let Some(chat) = self.chats.iter().find(|c| c.row.id == pending.peer_id)
                        {
                            sending::fail(&chat.messages, pending.local_id);
                        }
                    }
                }
                telegram::TgEvent::DialogOpened { dialog: d } => {
                    let id = d.id;
                    if !self.chats.iter().any(|c| c.row.id == id) {
                        let mut chat = Chat::new(
                            id,
                            &d.title,
                            &d.initials,
                            (114, 213, 253),
                            (42, 158, 241),
                            d.unread,
                            d.folder,
                            &[],
                        );
                        chat.messages
                            .set_vec(d.messages.iter().map(live_message).collect::<Vec<_>>());
                        chat.row.no_photo = d.no_photo;
                        chat.row.channel_access = d.channel as i32;
                        chat.row.presence = d.presence.into();
                        chat.row.online = d.online;
                        self.chats.push(chat);
                    }
                    self.folder = 0;
                    ui.set_active_folder(0);
                    ui.set_query(Default::default());
                    ui.set_network_status(Default::default());
                    self.open(ui, id);
                }
                telegram::TgEvent::DialogAdded { dialog: d } => {
                    if !self.chats.iter().any(|c| c.row.id == d.id) {
                        let mut chat = Chat::new(
                            d.id,
                            &d.title,
                            &d.initials,
                            (114, 213, 253),
                            (42, 158, 241),
                            d.unread,
                            d.folder,
                            &[],
                        );
                        chat.messages
                            .set_vec(d.messages.iter().map(live_message).collect::<Vec<_>>());
                        chat.row.no_photo = d.no_photo;
                        chat.row.channel_access = d.channel as i32;
                        chat.row.presence = d.presence.into();
                        chat.row.online = d.online;
                        self.chats.push(chat);
                        self.refresh_chats(ui);
                    }
                }
                telegram::TgEvent::LoggedOut => {
                    self.detail_requests.clear();
                    self.preview_lru.clear();
                    self.preview_bytes = 0;
                    ui.set_text_send_pending(false);
                    self.pending_send = None;
                    self.live_mode = false;
                    ui.set_show_chat(false);
                    self.folders.clear();
                    self.folder = 0;
                    ui.set_active_folder(0);
                    self.viewed_photo = None;
                    ui.set_photo_open(false);
                    ui.set_photo_image(Default::default());
                    ui.set_connected(false);
                    self.chats = Self::demo_chats();
                    self.active = 0;
                    ui.set_auth_step(0);
                    ui.set_auth_required(true);
                    ui.set_auth_busy(false);
                    ui.set_message_busy(false);
                    ui.set_draft(Default::default());
                    ui.set_auth_password(Default::default());
                    ui.set_auth_code(Default::default());
                    ui.set_selected_message(0);
                    ui.set_reply_message(0);
                    ui.set_edit_message_id(0);
                    ui.set_network_status(Default::default());
                    self.refresh_chats(ui);
                    self.refresh_history(ui);
                }
                telegram::TgEvent::Folders { folders } => {
                    self.folders = folders;
                    if !self.folders.iter().any(|f| f.id == self.folder) {
                        self.folder = 0;
                        ui.set_active_folder(0);
                    }
                    self.refresh_chats(ui);
                }
                telegram::TgEvent::PhotoReady {
                    peer_id,
                    message_id,
                    pixels,
                } => {
                    if ui.get_photo_open() && self.viewed_photo == Some((peer_id, message_id)) {
                        ui.set_photo_image(slint_image(&pixels));
                        ui.set_photo_status(Default::default());
                    }
                }
                telegram::TgEvent::AvatarReady { peer_id, pixels } => {
                    if peer_id == 0 {
                        ui.set_user_avatar(slint_image(&pixels));
                        ui.set_user_has_avatar(true);
                    }
                    if let Some(chat) = self.chats.iter_mut().find(|chat| chat.row.id == peer_id) {
                        chat.row.avatar = slint_image(&pixels);
                        chat.row.has_avatar = true;
                        if ui.get_active_chat().id == peer_id {
                            ui.set_active_chat(chat.row.clone());
                        }
                        avatars_changed = true;
                    }
                }
                telegram::TgEvent::ChannelJoined { peer_id, access } => {
                    ui.set_message_busy(false);
                    ui.set_network_status(Default::default());
                    if let Some(chat) = self.chats.iter_mut().find(|chat| chat.row.id == peer_id) {
                        chat.row.channel_access = access as i32;
                        if ui.get_active_chat().id == peer_id {
                            ui.set_active_chat(chat.row.clone());
                        }
                        self.refresh_chats(ui);
                    }
                }
                telegram::TgEvent::PhotoFailed {
                    peer_id,
                    message_id,
                    message,
                } => {
                    if ui.get_photo_open() && self.viewed_photo == Some((peer_id, message_id)) {
                        ui.set_photo_status(message.into());
                    }
                }
                telegram::TgEvent::Error { message } => {
                    if self.pending_send.is_none() {
                        ui.set_text_send_pending(false);
                        ui.set_message_busy(false);
                    }
                    ui.set_auth_busy(false);
                    if ui.get_auth_required() {
                        ui.set_auth_error(message.into());
                    } else {
                        ui.set_network_status(message.into());
                    }
                }
            }
        }
        if avatars_changed {
            self.refresh_chats(ui);
        }
        if let Some(chat) = self.chats.get(self.active) {
            details::resolve_local(&chat.messages, &chat.row.title);
        }
    }

    fn upsert_message(
        &mut self,
        ui: &Desktop,
        peer_id: i32,
        msg: &telegram::TgMessage,
        incoming: bool,
    ) {
        let active = self.chats.get(self.active).map(|c| c.row.id) == Some(peer_id);
        let reveal = incoming
            && !msg.outgoing
            && active
            && ui.get_conversation_visible()
            && !ui.get_auth_required()
            && ui.get_window_visible()
            && ui.get_history_at_bottom();
        if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
            let index = chat.messages.iter().position(|m| m.id == msg.id);
            if let Some(i) = index {
                let mut row = live_message(msg);
                if row.photo_key != chat.messages.row_data(i).unwrap().photo_key {
                    self.detail_requests.remove(&(peer_id, msg.id));
                }
                details::retain(&mut row, &chat.messages.row_data(i).unwrap());
                chat.messages.set_row_data(i, row);
            } else {
                let mut row = live_message(msg);
                if reveal {
                    row.receive_token = self.next_local_id;
                    self.next_local_id -= 1;
                    ui.invoke_animate_received_message();
                }
                chat.messages.push(row);
            }
            if chat.messages.iter().last().is_some_and(|m| m.id == msg.id) {
                chat.row.preview = message_preview(msg);
                chat.row.time = msg.time.clone().into();
                chat.row.outgoing = msg.outgoing;
                chat.row.last_message_id = msg.id;
            }
            if incoming && index.is_none() && !msg.outgoing {
                if active && ui.get_show_chat() && ui.get_has_chat() {
                    self.bridge.read(peer_id);
                } else {
                    chat.row.unread += 1;
                }
            }
            self.refresh_chats(ui);
            if active {
                // The persistent history model is already bound. Rebinding it
                // used to force a jump to the bottom even while reading above.
                ui.set_active_chat(self.chats[self.active].row.clone());
            }
        }
    }
}

impl Chat {
    fn new(
        id: i32,
        title: &str,
        initials: &str,
        (top_r, top_g, top_b): (u8, u8, u8),
        (bot_r, bot_g, bot_b): (u8, u8, u8),
        unread: i32,
        folder: i32,
        history: &[(&str, &str, bool)],
    ) -> Self {
        let (preview, time, outgoing) = history.last().copied().unwrap_or_default();
        Self {
            row: ChatRow {
                id,
                title: chat_preview(title),
                initials: initials.into(),
                accent: Color::from_rgb_u8(bot_r, bot_g, bot_b),
                accent_top: Color::from_rgb_u8(top_r, top_g, top_b),
                preview: chat_preview(preview),
                time: time.into(),
                unread,
                outgoing,
                last_message_id: 0,
                read_outbox_max_id: 0,
                folder,
                ..Default::default()
            },
            messages: Rc::new(VecModel::from(
                history
                    .iter()
                    .map(|&(body, time, outgoing)| message(body, time, outgoing))
                    .collect::<Vec<_>>(),
            )),
            draft: Default::default(),
            formatted_draft: Default::default(),
            editor_history: Default::default(),
        }
    }
}

fn update_rows<Row: Clone + PartialEq + 'static>(model: &VecModel<Row>, rows: Vec<Row>) {
    if model.row_count() != rows.len() {
        model.set_vec(rows);
        return;
    }
    for (index, row) in rows.into_iter().enumerate() {
        if model.row_data(index).as_ref() != Some(&row) {
            model.set_row_data(index, row);
        }
    }
}

fn slint_image(pixels: &image::RgbaImage) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
    );
    slint::Image::from_rgba8(buffer)
}

fn message(body: &str, time: &str, outgoing: bool) -> MessageRow {
    MessageRow {
        send_token: 0,
        receive_token: 0,
        pending: false,
        failed: false,
        reply_to: 0,
        id: 0,
        date: Default::default(),
        has_media: false,
        is_photo: false,
        body: body.into(),
        time: time.into(),
        outgoing,
        channel_post: false,
        service: false,
        ..Default::default()
    }
}

fn live_message(m: &telegram::TgMessage) -> MessageRow {
    MessageRow {
        sender_key: m.sender_key.clone().into(),
        entities_json: composer::encode(&m.entities).into(),
        send_token: 0,
        receive_token: 0,
        pending: false,
        failed: false,
        reply_to: m.reply_to,
        sender: m.sender.clone().into(),
        reply_body: m.reply_quote.clone().into(),
        reply_quoted: !m.reply_quote.is_empty(),
        reply_external: m.reply_external,
        photo_width: m.photo_width,
        photo_height: m.photo_height,
        photo_key: m.media_key.clone().into(),
        id: m.id,
        date: m.date.clone().into(),
        has_media: m.has_media,
        is_photo: m.is_photo,
        body: m.body.clone().into(),
        has_formatting: !m.entities.is_empty(),
        formatted_body: rich_text::format(&m.body, &m.entities, m.entity_offset),
        time: m.time.clone().into(),
        outgoing: m.outgoing,
        channel_post: m.channel_post,
        service: m.service,
        ..Default::default()
    }
}

fn message_preview(message: &telegram::TgMessage) -> slint::SharedString {
    let body = chat_preview(&message.body);
    if message.service || message.channel_post {
        return body;
    }
    let sender = if message.outgoing {
        "Вы"
    } else {
        message.sender.as_str()
    };
    if sender.is_empty() {
        body
    } else {
        format!("{}: {body}", chat_preview(sender)).into()
    }
}

fn chat_preview(text: &str) -> slint::SharedString {
    text.split_whitespace().collect::<Vec<_>>().join(" ").into()
}

fn install_rendering(ui: &Desktop) {
    ui.global::<Theme>()
        .on_open_link(|url| rich_text::open_link(url.as_str()));
    let weak = ui.as_weak();
    let mut cursor = (0.0, 0.0);
    ui.window().on_winit_window_event(move |_, event| {
        use slint::winit_030::winit::event::{ElementState, MouseButton, WindowEvent};
        if let Some(ui) = weak.upgrade() {
            match event {
                WindowEvent::CursorMoved { position, .. } => {
                    let p = position.to_logical::<f64>(ui.window().scale_factor() as f64);
                    cursor = (p.x, p.y);
                }
                WindowEvent::HoveredFile(_) => ui.set_file_drag_hover(true),
                WindowEvent::HoveredFileCancelled => ui.set_file_drag_hover(false),
                WindowEvent::DroppedFile(path) => {
                    ui.set_file_drag_hover(false);
                    attachments::stage(&ui, [path.clone()]);
                }
                WindowEvent::MouseInput {
                    button: MouseButton::Right,
                    state: ElementState::Pressed,
                    ..
                } if ui.get_has_chat()
                    && (ui.get_large_editor_open()
                        || cursor.1
                            >= ui.window().size().height as f64
                                / ui.window().scale_factor() as f64
                                - ui.get_composer_height() as f64)
                    && !ui.get_file_open()
                    && !ui.get_link_editor_open()
                    && !ui.get_editor_menu_open()
                    && !ui.get_profile_open() =>
                {
                    ui.invoke_composer_menu();
                    return slint::winit_030::EventResult::PreventDefault;
                }
                _ => {}
            }
        }
        if let WindowEvent::MouseInput { button, .. } = event {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Theme>()
                    .set_context_pointer(*button == MouseButton::Right);
            }
        }
        slint::winit_030::EventResult::Propagate
    });
    ui.global::<TelegramMotion>()
        .on_send_rise(sending::rise_progress);
    let played_sends = Rc::new(RefCell::new(std::collections::HashSet::new()));
    ui.global::<TelegramMotion>()
        .on_claim_send(move |token| played_sends.borrow_mut().insert(token));
    let played_receives = Rc::new(RefCell::new(std::collections::HashSet::new()));
    ui.global::<TelegramMotion>()
        .on_claim_receive(move |token| played_receives.borrow_mut().insert(token));
    ui.global::<TelegramMotion>()
        .on_append_ripple(append_ripple);
    ui.global::<TelegramMotion>()
        .on_remove_ripple(remove_ripple);
    let assets = Rc::new(RefCell::new(render_assets::Assets::new()));

    {
        let theme = ui.global::<Theme>();
        let assets_qr = Rc::clone(&assets);
        theme.on_render_qr(move |matrix, size, accent| {
            assets_qr
                .borrow_mut()
                .qr_card(matrix.as_str(), size.max(64) as u32, accent)
        });
    }
}

fn append_ripple(waves: ModelRc<i32>, generation: i32) -> ModelRc<i32> {
    if let Some(rows) = waves.as_any().downcast_ref::<VecModel<i32>>() {
        rows.push(generation);
        waves
    } else {
        let mut rows: Vec<_> = waves.iter().collect();
        rows.push(generation);
        ModelRc::new(VecModel::from(rows))
    }
}

fn remove_ripple(waves: ModelRc<i32>, generation: i32) {
    if let Some(rows) = waves.as_any().downcast_ref::<VecModel<i32>>() {
        let index = rows.iter().position(|wave| wave == generation);
        if let Some(index) = index {
            rows.remove(index);
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let renderer = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia".into());
    // Skia's default Windows surface is software. Request the native GPU API
    // explicitly so the cost of rasterizing every pixel does not grow on CPU.
    #[cfg(windows)]
    let renderer = renderer.require_d3d();
    #[cfg(not(windows))]
    let renderer = renderer.require_opengl();
    renderer.select()?;
    let ui = Desktop::new()?;
    install_rendering(&ui);
    attachments::install(&ui);
    #[cfg(windows)]
    let tray = {
        let restore = ui.as_weak();
        let exit = ui.as_weak();
        match tray::Tray::new(
            move || {
                let _ = restore.clone().upgrade_in_event_loop(|ui| {
                    ui.set_window_visible(true);
                    ui.window().set_minimized(false);
                    let _ = ui.show();
                    ui.window()
                        .with_winit_window(|window| window.focus_window());
                });
            },
            move || {
                let _ = exit.clone().upgrade_in_event_loop(|_| {
                    let _ = slint::quit_event_loop();
                });
            },
        ) {
            Ok(tray) => Some(tray),
            Err(error) => {
                eprintln!("Notification icon unavailable: {error}");
                None
            }
        }
    };
    #[cfg(windows)]
    let has_tray = tray.is_some();
    #[cfg(not(windows))]
    let has_tray = false;
    let weak = ui.as_weak();
    ui.on_minimize_window(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().set_minimized(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_maximize_window(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().set_maximized(!ui.window().is_maximized());
        }
    });
    let weak = ui.as_weak();
    ui.on_close_window(move || {
        if has_tray {
            if let Some(ui) = weak.upgrade() {
                ui.set_window_visible(false);
                let _ = ui.hide();
            }
        } else if let Err(error) = slint::quit_event_loop() {
            eprintln!("Could not close the application: {error}");
        }
    });
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        if has_tray {
            if let Some(ui) = weak.upgrade() {
                ui.set_window_visible(false);
            }
        }
        slint::CloseRequestResponse::HideWindow
    });
    let weak = ui.as_weak();
    ui.on_drag_window(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().with_winit_window(|window| {
                if let Err(error) = window.drag_window() {
                    eprintln!("Could not move the window: {error}");
                }
            });
        }
    });
    let weak = ui.as_weak();
    ui.on_resize_window(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().with_winit_window(|window| {
                if let Err(error) = window.drag_resize_window(ResizeDirection::SouthEast) {
                    eprintln!("Could not resize the window: {error}");
                }
            });
        }
    });
    match preferences::Preferences::load() {
        Ok(preferences) => preferences.apply(&ui),
        Err(error) => {
            eprintln!("Could not load appearance preferences: {error}");
            ui.set_settings_error(
                "Не удалось загрузить оформление. Применены настройки по умолчанию.".into(),
            );
        }
    }
    let weak = ui.as_weak();
    ui.on_settings_changed(move || {
        if let Some(ui) = weak.upgrade() {
            match preferences::Preferences::save(&ui) {
                Ok(()) => ui.set_settings_error(Default::default()),
                Err(error) => {
                    eprintln!("Could not save appearance preferences: {error}");
                    ui.set_settings_error(
                        "Не удалось сохранить оформление. Проверьте доступ к папке настроек."
                            .into(),
                    );
                }
            }
        }
    });
    let demo = Rc::new(RefCell::new(Demo::new()));
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.global::<Theme>().on_open_profile(move |key| {
        let Some(ui) = weak.upgrade() else { return };
        if key.is_empty() || ui.get_auth_required() {
            return;
        }
        ui.set_settings_open(false);
        ui.set_menu_open(false);
        ui.set_profile_key(key.clone());
        ui.set_profile_open(true);
        ui.set_profile_is_user(false);
        ui.set_profile_is_self(key == "self");
        ui.set_profile_is_bot(false);
        ui.set_profile_online(false);
        ui.set_profile_birthday(Default::default());
        ui.set_profile_action_busy(false);
        ui.set_profile_mute_ready(false);
        ui.set_profile_muted(false);
        ui.set_profile_qr_matrix(Default::default());
        ui.set_profile_link(Default::default());
        ui.set_profile_gifts_status("Загрузка подарков…".into());
        ui.set_profile_name("Загрузка…".into());
        ui.set_profile_initials(Default::default());
        ui.set_profile_presence(Default::default());
        ui.set_profile_username(Default::default());
        ui.set_profile_phone(Default::default());
        ui.set_profile_about(Default::default());
        ui.set_profile_has_photo(false);
        ui.set_profile_photo(Default::default());
        ui.set_profile_gifts(Default::default());
        ui.set_profile_next(Default::default());
        ui.set_profile_status("Загрузка профиля…".into());
        ui.set_profile_busy(true);
        state.borrow_mut().bridge.profile(
            key.as_str(),
            ui.global::<Theme>().get_show_gifts(),
            None,
        );
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_more_gifts(move || {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.get_profile_open()
            || ui.get_profile_busy()
            || ui.get_profile_next().is_empty()
            || !ui.global::<Theme>().get_show_gifts()
        {
            return;
        }
        ui.set_profile_busy(true);
        state.borrow_mut().bridge.profile(
            ui.get_profile_key().as_str(),
            true,
            Some(ui.get_profile_next().to_string()),
        );
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_profile_action(move |action| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_auth_required() || !ui.get_profile_open() {
            return;
        }
        let key = ui.get_profile_key();
        let mut state = state.borrow_mut();
        if action == "message" {
            ui.set_profile_open(false);
            if let Some(id) = key
                .strip_prefix("chat:")
                .and_then(|s| s.parse::<i32>().ok())
            {
                state.open(&ui, id);
            } else {
                state.bridge.profile_message(key.as_str());
            }
        } else if action == "mute" && ui.get_profile_mute_ready() && !ui.get_profile_action_busy() {
            ui.set_profile_action_busy(true);
            state
                .bridge
                .profile_mute(key.as_str(), !ui.get_profile_muted());
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_profile_photo_requested(move || {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.get_profile_has_photo() {
            return;
        }
        state.borrow_mut().viewed_photo = None;
        ui.set_photo_from_profile(true);
        ui.set_profile_open(false);
        ui.set_photo_title(ui.get_profile_name());
        ui.set_photo_image(ui.get_profile_photo());
        ui.set_photo_zoom(1.0);
        ui.set_photo_index(1);
        ui.set_photo_count(1);
        ui.set_photo_status(Default::default());
        ui.set_photo_open(true);
        ui.invoke_focus_photo();
    });

    demo.borrow().refresh_chats(&ui);
    demo.borrow().refresh_history(&ui);

    // Auth callbacks
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_request_qr(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_auth_error(Default::default());
            state.borrow_mut().bridge.start_qr();
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_switch_to_phone(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().bridge.cancel_qr();
            ui.set_auth_busy(false);
            ui.set_auth_error(Default::default());
            ui.set_auth_step(1);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_switch_to_qr(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_auth_error(Default::default());
            ui.set_auth_step(0);
            state.borrow_mut().bridge.start_qr();
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_submit_phone(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_auth_busy() {
                return;
            }
            let phone = ui.get_auth_phone().trim().to_owned();
            if phone.is_empty() {
                return;
            }
            ui.set_auth_busy(true);
            ui.set_auth_error(Default::default());
            state.borrow_mut().bridge.send_code(&phone);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_submit_code(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_auth_busy() {
                return;
            }
            let code = ui.get_auth_code().trim().to_owned();
            if code.is_empty() {
                return;
            }
            ui.set_auth_busy(true);
            ui.set_auth_error(Default::default());
            state.borrow_mut().bridge.sign_in_code(&code);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_submit_password(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_auth_busy() {
                return;
            }
            let password = ui.get_auth_password().to_string();
            if password.is_empty() {
                return;
            }
            ui.set_auth_busy(true);
            ui.set_auth_error(Default::default());
            state.borrow_mut().bridge.sign_in_password(&password);
        }
    });
    let weak = ui.as_weak();
    ui.on_auth_open_demo(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_auth_error(Default::default());
            ui.set_auth_required(false);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_auth_logout(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_auth_error(Default::default());
            ui.set_auth_step(0);
            ui.set_auth_required(true);
            state.borrow_mut().bridge.logout();
        }
    });

    // Wake only when the bridge has events, including while the window is hidden.
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&demo);
        ui.on_network_events(move || {
            if let Some(ui) = weak.upgrade() {
                state.borrow_mut().poll_telegram(&ui);
            }
        });
        let weak = std::sync::Mutex::new(ui.as_weak());
        demo.borrow().bridge.notify_with(move || {
            let _ = weak
                .lock()
                .unwrap()
                .clone()
                .upgrade_in_event_loop(|ui| ui.invoke_network_events());
        });
    }

    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_request_details(move |mid| {
        let Some(_ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if !state.live_mode || mid <= 0 {
            return;
        }
        let Some(chat) = state.chats.get(state.active) else {
            return;
        };
        let peer = chat.row.id;
        let Some(row) = chat.messages.iter().find(|row| row.id == mid) else {
            return;
        };
        if !(row.is_photo && !row.has_preview && !row.preview_failed)
            && !(row.reply_to > 0 && !row.reply_resolved)
        {
            return;
        }
        if state.detail_requests.insert((peer, mid)) {
            if row.is_photo && !row.has_preview && !row.preview_failed {
                state.bridge.thumbnail(peer, mid);
            }
            if row.reply_to > 0 && !row.reply_resolved {
                state.bridge.resolve_reply(peer, mid);
            }
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_jump_reply(move |mid| {
        let Some(ui) = weak.upgrade() else { return };
        let state = state.borrow();
        let Some(chat) = state.chats.get(state.active) else {
            return;
        };
        let Some(row) = chat.messages.iter().find(|row| row.id == mid) else {
            return;
        };
        if !row.reply_external && chat.messages.iter().any(|parent| parent.id == row.reply_to) {
            ui.invoke_reveal_message(row.reply_to);
        } else if state.live_mode {
            ui.set_network_status("Загрузка исходного сообщения…".into());
            state.bridge.jump_reply(chat.row.id, mid);
        }
    });
    let weak = ui.as_weak();
    ui.on_cancel_reply(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_reply_message(0);
            ui.set_edit_message_id(0);
            ui.set_reply_author(Default::default());
            ui.set_reply_body(Default::default());
            ui.invoke_focus_composer();
        }
    });
    let weak = ui.as_weak();
    ui.on_preview_file(move || {
        let Some(ui) = weak.upgrade() else { return };
        let path = ui.get_file_path().to_string();
        ui.set_file_preview_error(Default::default());
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let preview = details::file_preview(&path);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if ui.get_file_path() != path {
                    return;
                }
                match preview {
                    Ok(Some(pixels)) => {
                        ui.set_file_preview(slint_image(&pixels));
                        ui.set_has_file_preview(true);
                    }
                    Ok(None) => {
                        ui.set_has_file_preview(false);
                    }
                    Err(error) => {
                        ui.set_has_file_preview(false);
                        ui.set_file_preview_error(error.into());
                    }
                }
            });
        });
    });

    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_open_chat(move |id| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().open(&ui, id);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_request_avatar(move |id| {
        let Some(_ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if state.live_mode && state.chats.iter().any(|chat| chat.row.id == id) {
            state.bridge.load_avatar(id);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_select_folder(move |folder| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().select_folder(&ui, folder);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_query_edited(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow().refresh_chats(&ui);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_open_first_result(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = state
            .borrow()
            .matching(&ui.get_query())
            .next()
            .map(|chat| chat.row.id);
        if let Some(id) = id {
            state.borrow_mut().open(&ui, id);
        } else if state.borrow().live_mode {
            let query = ui.get_query();
            if query.starts_with('@') || query.starts_with("https://t.me/") {
                ui.set_network_status("Поиск чата…".into());
                state.borrow_mut().bridge.open_username(query.as_str());
            }
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_send_message(move || {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().send(&ui);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_retry_message(move |local_id| {
        if let Some(ui) = weak.upgrade() {
            state.borrow_mut().retry_send(&ui, local_id);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_send_file(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if !state.live_mode
            || ui.get_message_busy()
            || ui.get_attachment_pending() > 0
            || !ui.get_chat_can_write()
            || ui.get_attachments().row_count() == 0
        {
            return;
        }
        let Some(id) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        ui.set_message_busy(true);
        ui.set_upload_busy(true);
        ui.set_file_open(false);
        ui.set_network_status("Загрузка файла…".into());
        state.bridge.send_files(
            id,
            ui.get_attachments()
                .iter()
                .map(|r| std::path::PathBuf::from(r.path.as_str()))
                .collect(),
            ui.get_attachment_caption().as_str(),
            (ui.get_reply_message() > 0).then(|| ui.get_reply_message()),
        );
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_editor_history(move |action| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let active = state.active;
        if let Some(chat) = state.chats.get_mut(active) {
            chat.editor_history
                .action(action.as_str(), &mut chat.formatted_draft);
            chat.draft = chat.formatted_draft.text.clone().into();
            if action != "begin" {
                ui.set_draft(chat.draft.clone());
            }
            composer::preview(&ui, &chat.formatted_draft);
            chat.editor_history.sync(&ui);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_draft_edited(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let active = state.active;
        if let Some(chat) = state.chats.get_mut(active) {
            chat.editor_history.record(&chat.formatted_draft);
            chat.formatted_draft.edit(ui.get_draft().as_str());
            chat.draft = ui.get_draft();
            composer::preview(&ui, &chat.formatted_draft);
            chat.editor_history.sync(&ui);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_format_editor(move |kind, start, end| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if ui.get_auth_required()
            || !ui.get_chat_can_write()
            || ui.get_message_busy() && !ui.get_text_send_pending()
        {
            return;
        }
        let active = state.active;
        if let Some(chat) = state.chats.get_mut(active) {
            chat.editor_history.record(&chat.formatted_draft);
            chat.formatted_draft.edit(ui.get_draft().as_str());
            if chat.formatted_draft.toggle(
                kind.as_str(),
                start.max(0) as usize,
                end.max(0) as usize,
                ui.get_editor_link().as_str(),
            ) {
                composer::preview(&ui, &chat.formatted_draft);
                chat.editor_history.sync(&ui);
            } else {
                ui.set_network_status(
                    "Выделите текст для форматирования; для ссылки укажите полный URL.".into(),
                );
            }
        }
        ui.invoke_focus_composer();
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_load_older(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(chat) = state.chats.get(state.active) else {
            return;
        };
        let id = chat.row.id;
        let before = chat
            .messages
            .iter()
            .find(|m| m.id > 0)
            .map(|m| m.id)
            .unwrap_or(0);
        if state.live_mode {
            ui.set_network_status("Загрузка истории…".into());
            state.bridge.older(id, before);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_search_chat(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(id) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        if state.live_mode {
            ui.set_network_status("Поиск…".into());
            state.bridge.search(id, ui.get_chat_query().as_str());
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_refresh_telegram(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(id) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        if state.live_mode {
            ui.set_chat_search_open(false);
            ui.set_network_status(Default::default());
            state.bridge.load_history(id);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_message_action(move |action| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(id) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        let mid = ui.get_selected_message();
        match action.as_str() {
            "cancel" => {
                ui.set_reply_message(0);
                ui.set_edit_message_id(0);
                ui.set_selected_message(0);
                ui.set_reply_author(Default::default());
                ui.set_reply_body(Default::default());
            }
            "reply" if mid > 0 => {
                ui.set_reply_message(mid);
                ui.set_edit_message_id(0);
                let sender = state.chats[state.active]
                    .messages
                    .iter()
                    .find(|row| row.id == mid)
                    .map(|row| {
                        if row.outgoing {
                            "Вы".into()
                        } else if row.sender.is_empty() {
                            state.chats[state.active].row.title.clone()
                        } else {
                            row.sender
                        }
                    })
                    .unwrap_or_default();
                ui.set_reply_author(sender);
                ui.set_reply_body(ui.get_selected_body());
                ui.set_network_status(Default::default());
                ui.invoke_focus_composer();
            }
            "edit" if mid > 0 && ui.get_selected_outgoing() => {
                ui.set_edit_message_id(mid);
                ui.set_reply_message(0);
                ui.set_draft(ui.get_selected_body());
                let active = state.active;
                let entities = state.chats[active]
                    .messages
                    .iter()
                    .find(|m| m.id == mid)
                    .map(|m| composer::decode(m.entities_json.as_str()))
                    .unwrap_or_default();
                state.chats[active].formatted_draft = composer::Draft {
                    text: ui.get_draft().to_string(),
                    entities,
                };
                composer::preview(&ui, &state.chats[active].formatted_draft);
                ui.invoke_focus_composer();
            }
            "delete" if mid > 0 => {
                ui.set_confirm_delete(true);
            }
            "confirm-delete" if state.live_mode && mid > 0 => {
                state.bridge.delete(id, mid);
            }
            "download" if state.live_mode && mid > 0 => {
                ui.set_network_status("Скачивание…".into());
                state.bridge.download(id, mid);
            }
            "forward" if mid > 0 => {
                ui.set_forward_open(true);
            }
            _ => (),
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_forward_message(move |target| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(from) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        if state.live_mode && ui.get_selected_message() > 0 {
            state
                .bridge
                .forward(from, ui.get_selected_message(), target);
            ui.set_forward_open(false);
            ui.set_network_status("Пересылка…".into());
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_view_photo(move |mid| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(peer_id) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        if !state.live_mode || mid <= 0 {
            return;
        }
        state.show_photo(&ui, peer_id, mid);
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_step_photo(move |direction| {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some((peer_id, _)) = state.viewed_photo else {
            return;
        };
        let next = ui.get_photo_index() - 1 + direction;
        if next < 0 || next >= ui.get_photo_count() {
            return;
        }
        let id = state
            .chats
            .iter()
            .find(|chat| chat.row.id == peer_id)
            .and_then(|chat| {
                chat.messages
                    .iter()
                    .filter(|message| message.is_photo)
                    .nth(next as usize)
            })
            .map(|message| message.id);
        if let Some(id) = id {
            state.show_photo(&ui, peer_id, id);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_save_photo(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if ui.get_photo_open()
            && let Some((peer_id, id)) = state.viewed_photo
        {
            state.bridge.download(peer_id, id);
        }
    });
    let (weak, state) = (ui.as_weak(), Rc::clone(&demo));
    ui.on_join_channel(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let chat = ui.get_active_chat();
        if state.live_mode && chat.channel_access == 3 && !ui.get_message_busy() {
            ui.set_message_busy(true);
            state.bridge.join_channel(chat.id);
        }
    });
    if has_tray {
        ui.show()?;
        slint::run_event_loop_until_quit()
    } else {
        ui.run()
    }
}
