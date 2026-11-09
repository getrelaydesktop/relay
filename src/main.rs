#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{cell::RefCell, rc::Rc, time::Duration};

use slint::winit_030::{WinitWindowAccessor, winit::window::ResizeDirection};
use slint::{Color, ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};

slint::include_modules!();
mod menu_animation;
mod preferences;
mod qr;
mod render_assets;
mod telegram;
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
}

struct Demo {
    chats: Vec<Chat>,
    chat_model: Rc<VecModel<ChatRow>>,
    folder_model: Rc<VecModel<FolderRow>>,
    active: usize,
    folder: i32,
    folders: Vec<telegram::TgFolder>,
    viewed_photo: Option<(i32, i32)>,
    live_mode: bool,
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
        Self {
            chats: Self::demo_chats(),
            chat_model: Rc::new(VecModel::default()),
            folder_model: Rc::new(VecModel::default()),
            active: 0,
            folder: 0,
            folders: vec![],
            viewed_photo: None,
            live_mode: false,
            bridge: telegram::TelegramBridge::spawn(),
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
        ui.set_reply_message(0);
        ui.set_edit_message_id(0);
        ui.set_chat_query(Default::default());
        ui.set_chat_search_open(false);
        self.chats[index].row.unread = 0;
        ui.set_draft(self.chats[index].draft.clone());
        ui.set_show_chat(true);
        ui.set_has_chat(true);
        if self.live_mode {
            self.bridge.load_history(id);
            self.bridge.read(id);
        }
        self.refresh_chats(ui);
        self.refresh_history(ui);
        ui.invoke_focus_composer();
    }

    fn send(&mut self, ui: &Desktop) {
        if ui.get_message_busy() {
            return;
        }
        let draft = ui.get_draft();
        let body = draft.trim();
        if body.is_empty() || self.chats.is_empty() {
            return;
        }
        let idx = self.active.min(self.chats.len() - 1);
        let peer_id = self.chats[idx].row.id;
        if matches!(self.chats[idx].row.channel_access, 1 | 3) {
            return;
        }
        if self.live_mode {
            ui.set_message_busy(true);
            ui.set_network_status("Отправка…".into());
            if ui.get_edit_message_id() > 0 {
                self.bridge.edit(peer_id, ui.get_edit_message_id(), body);
            } else {
                self.bridge.send_message(
                    peer_id,
                    body,
                    (ui.get_reply_message() > 0).then(|| ui.get_reply_message()),
                );
            }
            return;
        }
        let chat = &mut self.chats[idx];
        chat.messages.push(message(body, "сейчас", true));
        chat.row.preview = body.into();
        chat.row.time = "сейчас".into();
        chat.row.outgoing = true;
        chat.draft = Default::default();
        ui.set_draft(Default::default());
        self.refresh_chats(ui);
        self.refresh_history(ui);
    }

    fn poll_telegram(&mut self, ui: &Desktop) {
        let mut avatars_changed = false;
        for _ in 0..64 {
            let Some(ev) = self.bridge.try_recv() else {
                break;
            };
            match ev {
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
                                let (preview, time, outgoing) = d
                                    .messages
                                    .last()
                                    .map(|m| (m.body.as_str(), m.time.as_str(), m.outgoing))
                                    .unwrap_or(("", "", false));
                                Chat {
                                    row: ChatRow {
                                        id: d.id,
                                        title: chat_preview(&d.title),
                                        initials: d.initials.into(),
                                        accent: Color::from_rgb_u8(bot.0, bot.1, bot.2),
                                        accent_top: Color::from_rgb_u8(top.0, top.1, top.2),
                                        preview: chat_preview(preview),
                                        time: time.into(),
                                        unread: d.unread,
                                        outgoing,
                                        last_message_id: d.messages.last().map_or(0, |m| m.id),
                                        read_outbox_max_id: d.read_outbox_max_id,
                                        folder: d.folder,
                                        channel_access: d.channel as i32,
                                        ..Default::default()
                                    },
                                    messages: Rc::new(VecModel::from(
                                        d.messages.iter().map(live_message).collect::<Vec<_>>(),
                                    )),
                                    draft: Default::default(),
                                }
                            })
                            .collect();
                        self.active = 0;
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
                            chat.row.preview = chat_preview(&last.body);
                            chat.row.time = last.time.clone().into();
                            chat.row.outgoing = last.outgoing;
                            chat.row.last_message_id = last.id;
                        }
                        chat.messages.set_vec(rows);
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
                    ui.set_message_busy(false);
                    ui.set_network_status(Default::default());
                    if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
                        chat.draft = Default::default();
                    }
                    if self.chats.get(self.active).map(|c| c.row.id) == Some(peer_id) {
                        ui.set_draft(Default::default());
                        ui.set_reply_message(0);
                        ui.set_edit_message_id(0);
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
                    ui.set_message_busy(false);
                    ui.set_network_status(message.into());
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
                        chat.row.channel_access = d.channel as i32;
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
                        chat.row.channel_access = d.channel as i32;
                        self.chats.push(chat);
                        self.refresh_chats(ui);
                    }
                }
                telegram::TgEvent::LoggedOut => {
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
                    ui.set_message_busy(false);
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
    }

    fn upsert_message(
        &mut self,
        ui: &Desktop,
        peer_id: i32,
        msg: &telegram::TgMessage,
        incoming: bool,
    ) {
        let active = self.chats.get(self.active).map(|c| c.row.id) == Some(peer_id);
        if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == peer_id) {
            let index = chat.messages.iter().position(|m| m.id == msg.id);
            if let Some(i) = index {
                chat.messages.set_row_data(i, live_message(msg));
            } else {
                chat.messages.push(live_message(msg));
            }
            if chat.messages.iter().last().is_some_and(|m| m.id == msg.id) {
                chat.row.preview = chat_preview(&msg.body);
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
                self.refresh_history(ui);
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
        id: 0,
        date: Default::default(),
        has_media: false,
        is_photo: false,
        body: body.into(),
        time: time.into(),
        outgoing,
        channel_post: false,
        service: false,
    }
}

fn live_message(m: &telegram::TgMessage) -> MessageRow {
    MessageRow {
        id: m.id,
        date: m.date.clone().into(),
        has_media: m.has_media,
        is_photo: m.is_photo,
        body: m.body.clone().into(),
        time: m.time.clone().into(),
        outgoing: m.outgoing,
        channel_post: m.channel_post,
        service: m.service,
    }
}

fn chat_preview(text: &str) -> slint::SharedString {
    text.split_whitespace().collect::<Vec<_>>().join(" ").into()
}

fn install_rendering(ui: &Desktop) {
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

fn main() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("skia".into())
        .select()?;
    let ui = Desktop::new()?;
    install_rendering(&ui);
    menu_animation::install(&ui);
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
    ui.on_close_window(|| {
        if let Err(error) = slint::quit_event_loop() {
            eprintln!("Could not close the application: {error}");
        }
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

    // Poll MTProto bridge events every 100ms
    let poll_timer = Timer::default();
    {
        let weak = ui.as_weak();
        let state = Rc::clone(&demo);
        poll_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            if let Some(ui) = weak.upgrade() {
                state.borrow_mut().poll_telegram(&ui);
            }
        });
    }

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
    ui.on_send_file(move || {
        let Some(ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        if !state.live_mode || ui.get_message_busy() || ui.get_file_path().trim().is_empty() {
            return;
        }
        let Some(id) = state.chats.get(state.active).map(|c| c.row.id) else {
            return;
        };
        ui.set_message_busy(true);
        ui.set_file_open(false);
        ui.set_network_status("Загрузка файла…".into());
        state.bridge.send_file(
            id,
            ui.get_file_path().as_str(),
            ui.get_draft().as_str(),
            (ui.get_reply_message() > 0).then(|| ui.get_reply_message()),
        );
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
                ui.set_draft(Default::default());
            }
            "reply" if mid > 0 => {
                ui.set_reply_message(mid);
                ui.set_edit_message_id(0);
                ui.set_network_status(format!("Ответ: {}", ui.get_selected_body()).into());
                ui.invoke_focus_composer();
            }
            "edit" if mid > 0 && ui.get_selected_outgoing() => {
                ui.set_edit_message_id(mid);
                ui.set_reply_message(0);
                ui.set_draft(ui.get_selected_body());
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
    ui.run()
}
