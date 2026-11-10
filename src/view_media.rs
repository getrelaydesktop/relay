//! Residency of disposable view pixels. Message text, drafts and the Telegram
//! session are not caches and are deliberately retained.
use crate::{Demo, Desktop, GiftRow, Theme, local_preview, profiles, telegram};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};
use telegram::{GiftMediaKind as Kind, GiftMediaSource as Source};

const GIFT_BYTES: usize = 12 * 1024 * 1024;
const GIFT_JOBS: usize = 8;
const GIFT_ROWS: usize = 32;
const AVATAR_COUNT: usize = 64; // 256px maximum: at most 16 MiB, not per-chat forever.

#[derive(Default)]
pub(super) struct Residency {
    pub visible_details: HashSet<(i32, i32)>,
    pub gift_sources: HashMap<String, profiles::GiftSources>,
    gift_view: Vec<usize>,
    gift_size: u32,
    gift_pending: HashMap<(String, Kind), u32>,
    gift_sizes: HashMap<(String, Kind), u32>,
    gift_failed: HashSet<(String, Kind)>,
    gift_resident: HashSet<usize>,
    pub gift_bytes: usize,
    pub gifts_requested: bool,
    avatar_lru: VecDeque<i32>,
}

fn image_bytes(image: &slint::Image) -> usize {
    let size = image.size();
    (size.width as usize)
        .saturating_mul(size.height as usize)
        .saturating_mul(4)
}
fn row_bytes(row: &GiftRow) -> usize {
    [
        &row.photo,
        &row.pattern,
        &row.sender_avatar,
        &row.owner_avatar,
    ]
    .into_iter()
    .map(image_bytes)
    .sum()
}
fn clear_row(row: &mut GiftRow) {
    row.photo = Default::default();
    row.has_photo = false;
    row.pattern = Default::default();
    row.has_pattern = false;
    row.sender_avatar = Default::default();
    row.has_sender_avatar = false;
    row.owner_avatar = Default::default();
    row.has_owner_avatar = false;
}
fn initials(name: &str) -> slint::SharedString {
    name.split_whitespace()
        .take(2)
        .filter_map(|word| word.chars().next())
        .collect::<String>()
        .into()
}
fn profile_visible(ui: &Desktop) -> bool {
    ui.get_window_visible() && !ui.get_auth_required() && ui.get_profile_open()
}

impl Demo {
    pub(super) fn evict_preview(&mut self, peer: i32, mid: i32) {
        self.bridge.cancel_thumbnail(peer, mid);
        self.detail_requests.remove(&(peer, mid));
        let mut released = 0;
        self.preview_lru.retain(|&(p, id, bytes)| {
            if (p, id) == (peer, mid) {
                released += bytes;
                false
            } else {
                true
            }
        });
        self.preview_bytes = self.preview_bytes.saturating_sub(released);
        if let Some(chat) = self.chats.iter().find(|c| c.row.id == peer) {
            if let Some(index) = chat.messages.iter().position(|row| row.id == mid) {
                let mut row = chat.messages.row_data(index).unwrap();
                if row.has_preview || row.preview_failed || image_bytes(&row.preview) != 0 {
                    row.preview = Default::default();
                    row.has_preview = false;
                    row.preview_failed = false;
                    chat.messages.set_row_data(index, row);
                }
            }
        }
    }
    pub(super) fn release_history_pixels(&mut self) {
        self.bridge.cancel_thumbnails();
        self.media.visible_details.clear();
        self.detail_requests.clear();
        for chat in &self.chats {
            for index in 0..chat.messages.row_count() {
                let mut row = chat.messages.row_data(index).unwrap();
                if row.has_preview || row.preview_failed || image_bytes(&row.preview) != 0 {
                    row.preview = Default::default();
                    row.has_preview = false;
                    row.preview_failed = false;
                    chat.messages.set_row_data(index, row);
                }
            }
        }
        self.preview_lru.clear();
        self.preview_bytes = 0;
    }
    fn release_gift_pixels(&mut self, ui: &Desktop) {
        self.bridge.cancel_gifts();
        self.media.gift_view.clear();
        self.media.gift_pending.clear();
        self.media.gift_failed.clear();
        self.media.gift_sizes.clear();
        self.media.gift_resident.clear();
        self.media.gift_bytes = 0;
        let model = ui.get_profile_gifts();
        for (index, mut row) in model.iter().enumerate() {
            if row_bytes(&row) != 0 {
                clear_row(&mut row);
                model.set_row_data(index, row);
            }
        }
    }
    pub(super) fn prepare_profile(&mut self, ui: &Desktop) {
        self.bridge.cancel_profile();
        self.bridge.cancel_photo();
        self.viewed_photo = None;
        self.release_gift_pixels(ui);
        self.media.gift_sources.clear();
        self.media.gifts_requested = ui.global::<Theme>().get_show_gifts();
        ui.set_gift_open(false);
        ui.set_selected_gift_index(-1);
        ui.set_photo_from_profile(false);
        ui.set_photo_open(false);
        ui.set_photo_image(Default::default());
        ui.set_profile_gifts(Default::default());
        ui.set_profile_photo(Default::default());
        ui.set_profile_has_photo(false);
        ui.invoke_clear_raster_cache();
    }
    fn release_avatars(&mut self, ui: &Desktop) {
        self.bridge.cancel_avatars();
        let mut ids = vec![0];
        for chat in &mut self.chats {
            ids.push(chat.row.id);
            chat.row.avatar = Default::default();
            chat.row.has_avatar = false;
        }
        self.media.avatar_lru.clear();
        self.bridge.forget_avatars(ids);
        ui.set_user_avatar(Default::default());
        ui.set_user_has_avatar(false);
        let mut active = ui.get_active_chat();
        active.avatar = Default::default();
        active.has_avatar = false;
        ui.set_active_chat(active);
        if ui.get_window_visible() {
            self.refresh_chats(ui);
        }
    }
    pub(super) fn release_media(&mut self, ui: &Desktop) {
        let hidden = !ui.get_window_visible() || ui.get_auth_required();
        if hidden || !ui.get_conversation_visible() {
            self.release_history_pixels();
        }
        if hidden || !ui.get_photo_open() {
            self.bridge.cancel_photo();
            self.viewed_photo = None;
            ui.set_photo_image(Default::default());
            ui.set_photo_status(Default::default());
        }
        if hidden || !ui.get_file_open() || ui.get_file_path().is_empty() {
            local_preview::cancel();
            ui.set_file_preview(Default::default());
            ui.set_has_file_preview(false);
        }
        if hidden || !ui.get_profile_open() {
            self.bridge.cancel_profile();
            self.release_gift_pixels(ui);
            ui.set_gift_open(false);
            ui.set_selected_gift_index(-1);
            // Canceling an unfinished first page must allow a fresh request
            // when a profile-origin viewer returns; pagination keeps its token.
            if ui.get_profile_busy() && ui.get_profile_gifts().row_count() == 0 {
                self.media.gifts_requested = false;
            }
            ui.set_profile_busy(false);
            // Retain only the bounded cover thumbnail for a profile return.
            // Never promote the viewer's (up to 2560 px) raster into an avatar.
            if hidden || !ui.get_photo_from_profile() || !ui.get_photo_open() {
                ui.set_profile_photo(Default::default());
                ui.set_profile_has_photo(false);
            }
            if hidden || !ui.get_photo_from_profile() {
                ui.set_profile_key(Default::default());
                ui.set_profile_gifts(Default::default());
                ui.set_profile_next(Default::default());
                ui.set_profile_busy(false);
                self.media.gift_sources.clear();
                self.media.gifts_requested = false;
            }
        } else if !ui.global::<Theme>().get_show_gifts() {
            ui.set_gift_open(false);
            ui.set_selected_gift_index(-1);
            self.release_gift_pixels(ui);
        }
        if hidden {
            self.release_avatars(ui);
        }
        if hidden
            || (!ui.get_profile_qr_open()
                && (!ui.get_auth_required()
                    || ui.get_auth_step() != 0
                    || ui.get_auth_qr_matrix().is_empty()))
        {
            ui.invoke_clear_raster_cache();
        }
        if !hidden && profile_visible(ui) && ui.global::<Theme>().get_show_gifts() {
            self.refresh_gift_media(ui);
        }
    }
    pub(super) fn thumbnail_ready(
        &mut self,
        ui: &Desktop,
        peer: i32,
        mid: i32,
        key: String,
        pixels: Option<crate::media::Pixels>,
    ) {
        self.bridge.complete_thumbnail(peer, mid);
        self.detail_requests.remove(&(peer, mid));
        if !ui.get_window_visible()
            || ui.get_auth_required()
            || !self.media.visible_details.contains(&(peer, mid))
        {
            return;
        }
        let Some(chat) = self.chats.iter().find(|c| c.row.id == peer) else {
            return;
        };
        let Some(index) = chat.messages.iter().position(|row| row.id == mid) else {
            return;
        };
        let mut row = chat.messages.row_data(index).unwrap();
        if !key.is_empty() && row.photo_key != key {
            return;
        }
        let mut released = 0;
        self.preview_lru.retain(|&(p, id, bytes)| {
            if (p, id) == (peer, mid) {
                released += bytes;
                false
            } else {
                true
            }
        });
        self.preview_bytes = self.preview_bytes.saturating_sub(released);
        row.preview = Default::default();
        row.has_preview = false;
        row.preview_failed = true;
        if let Some(pixels) = pixels {
            let bytes = pixels.len();
            // Do not evict a still-visible image and immediately redownload it
            // in a loop. A budget-limited placeholder retries on the next visit.
            if self.preview_bytes.saturating_add(bytes) <= 24 * 1024 * 1024 {
                row.preview = pixels.image();
                row.has_preview = true;
                row.preview_failed = false;
                self.preview_lru.push_back((peer, mid, bytes));
                self.preview_bytes += bytes;
            }
        }
        chat.messages.set_row_data(index, row);
    }
    pub(super) fn avatar_ready(
        &mut self,
        ui: &Desktop,
        id: i32,
        pixels: crate::media::Pixels,
    ) -> bool {
        if !ui.get_window_visible()
            || ui.get_auth_required()
            || pixels.width() == 0 || pixels.height() == 0
            || pixels.width() > 256
            || pixels.height() > 256
        {
            self.bridge.forget_avatars(vec![id]);
            return false;
        }
        if id != 0 && !self.chats.iter().any(|c| c.row.id == id) {
            self.bridge.forget_avatars(vec![id]);
            return false;
        }
        self.bridge.complete_avatar(id);
        let image = pixels.image();
        if id == 0 {
            ui.set_user_avatar(image.clone());
            ui.set_user_has_avatar(true);
        }
        if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == id) {
            chat.row.avatar = image;
            chat.row.has_avatar = true;
            if ui.get_active_chat().id == id {
                ui.set_active_chat(chat.row.clone());
            }
        }
        self.media.avatar_lru.retain(|peer| *peer != id);
        self.media.avatar_lru.push_back(id);
        while self.media.avatar_lru.len() > AVATAR_COUNT {
            let evicted = self.media.avatar_lru.pop_front().unwrap();
            // Keep the own/active icons while evicting older list icons.
            if evicted == 0 || evicted == ui.get_active_chat().id {
                self.media.avatar_lru.push_back(evicted);
                continue;
            }
            if let Some(chat) = self.chats.iter_mut().find(|c| c.row.id == evicted) {
                chat.row.avatar = Default::default();
                chat.row.has_avatar = false;
            }
            self.bridge.forget_avatars(vec![evicted]);
        }
        true
    }
    pub(super) fn gifts_ready(
        &mut self,
        ui: &Desktop,
        key: String,
        gifts: Vec<profiles::Gift>,
        next: String,
        append: bool,
    ) {
        if !profile_visible(ui) || ui.get_profile_key() != key {
            return;
        }
        ui.set_profile_busy(false);
        if !ui.global::<Theme>().get_show_gifts() {
            return;
        }
        if !append {
            self.release_gift_pixels(ui);
            self.media.gift_sources.clear();
        }
        let mut rows: Vec<GiftRow> = if append {
            ui.get_profile_gifts().iter().collect()
        } else {
            Vec::new()
        };
        let known: HashSet<_> = rows.iter().map(|r| r.key.to_string()).collect();
        for gift in gifts {
            if known.contains(&gift.key) {
                continue;
            }
            self.media
                .gift_sources
                .insert(gift.key.clone(), gift.sources);
            let sender_initials = initials(&gift.sender);
            let owner_initials = initials(&gift.owner);
            let message = crate::rich_text::format(&gift.message, &gift.message_entities, 0);
            let original_info = crate::rich_text::format(
                &gift.original_info, &gift.original_entities, gift.original_prefix,
            );
            let mine = ui.get_profile_is_self();
            let badge = if mine && gift.collectible && !gift.resale && gift.number > 0 {
                format!("#{}", profiles::decimal_count(gift.number as i64))
            } else {
                gift.badge
            };
            rows.push(GiftRow {
                key: gift.key.into(),
                title: gift.title.into(),
                detail: gift.detail.into(),
                url: gift.url.into(),
                collectible: gift.collectible,
                center: slint::Color::from_argb_encoded(0xff000000 | gift.center as u32),
                edge: slint::Color::from_argb_encoded(0xff000000 | gift.edge as u32),
                pattern_color: slint::Color::from_argb_encoded(
                    0xff000000 | gift.pattern_color as u32,
                ),
                text_color: slint::Color::from_argb_encoded(0xff000000 | gift.text_color as u32),
                badge: badge.into(),
                pinned: gift.pinned,
                mine,
                resale: gift.resale,
                number: gift.number,
                sender: gift.sender.into(),
                sender_initials,
                sender_hidden: gift.sender_hidden,
                show_sender: gift.show_sender,
                owner: gift.owner.into(),
                owner_initials,
                owner_address: gift.owner_address.into(),
                date: gift.date.into(),
                value: gift.value.into(),
                availability: gift.availability.into(),
                model: gift.model.into(),
                model_rarity: gift.model_rarity.into(),
                symbol: gift.symbol.into(),
                symbol_rarity: gift.symbol_rarity.into(),
                backdrop: gift.backdrop.into(),
                backdrop_rarity: gift.backdrop_rarity.into(),
                has_message: !gift.message.is_empty(),
                message,
                has_original_info: !gift.original_info.is_empty(),
                original_info,
                to_self: gift.to_self,
                can_upgrade: gift.can_upgrade,
                ..Default::default()
            });
        }
        ui.set_profile_gifts_status(if rows.is_empty() {
            "Нет публичных подарков".into()
        } else {
            Default::default()
        });
        ui.set_profile_gifts(ModelRc::new(VecModel::from(rows)));
        ui.set_profile_next(next.into());
        self.refresh_gift_media(ui);
    }
    pub(super) fn refresh_gift_media(&mut self, ui: &Desktop) {
        if !profile_visible(ui) || !ui.global::<Theme>().get_show_gifts() {
            self.release_gift_pixels(ui);
            return;
        }
        if !self.media.gifts_requested
            && !ui.get_profile_busy()
            && !ui.get_profile_key().is_empty()
            && ui.get_profile_is_user()
        {
            self.media.gifts_requested = true;
            ui.set_profile_busy(true);
            ui.set_profile_gifts_status("Загрузка подарков…".into());
            self.bridge
                .profile(ui.get_profile_key().as_str(), true, Some(String::new()));
        }
        let model = ui.get_profile_gifts();
        let count = model.row_count();
        let first = (ui.get_profile_first_gift().max(0) as usize).min(count);
        let last = (ui.get_profile_last_gift().max(0) as usize).min(count);
        let selected = ui
            .get_gift_open()
            .then_some(ui.get_selected_gift_index())
            .filter(|i| *i >= 0 && (*i as usize) < count)
            .map(|i| i as usize);
        let mut wanted: Vec<_> = selected
            .into_iter()
            .chain(first..last)
            .take(GIFT_ROWS)
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        let size = (80.0 * ui.window().scale_factor())
            .ceil()
            .clamp(80.0, 256.0) as u32;
        if wanted != self.media.gift_view || size != self.media.gift_size {
            self.bridge.cancel_gifts();
            self.media.gift_pending.clear();
            self.media.gift_failed.clear();
            self.media.gift_view = wanted.clone();
            self.media.gift_size = size;
        }
        let evict: Vec<_> = self
            .media
            .gift_resident
            .iter()
            .copied()
            .filter(|i| !wanted.contains(i))
            .collect();
        for index in evict {
            if let Some(mut row) = model.row_data(index) {
                self.media.gift_bytes = self.media.gift_bytes.saturating_sub(row_bytes(&row));
                self.media
                    .gift_sizes
                    .retain(|(key, _), _| key != row.key.as_str());
                clear_row(&mut row);
                model.set_row_data(index, row);
            }
            self.media.gift_resident.remove(&index);
        }
        // Prioritize the opened gift; only eight source jobs may be queued.
        let order: Vec<_> = selected
            .into_iter()
            .chain(wanted.iter().copied().filter(|i| Some(*i) != selected))
            .collect();
        for index in order {
            let Some(row) = model.row_data(index) else {
                continue;
            };
            let Some(sources) = self.media.gift_sources.get(row.key.as_str()) else {
                continue;
            };
            let selected = Some(index) == selected;
            let side = if selected {
                (120.0 * ui.window().scale_factor())
                    .ceil()
                    .clamp(120.0, 256.0) as u32
            } else {
                size
            };
            let requests = [
                (
                    Kind::Model,
                    sources.model.clone().map(Source::Document),
                    side,
                    row.has_photo,
                ),
                (
                    Kind::Pattern,
                    sources.pattern.clone().map(Source::Document),
                    (size / 2).max(40),
                    row.has_pattern,
                ),
                (
                    Kind::Sender,
                    sources
                        .sender
                        .clone()
                        .filter(|_| row.show_sender || (selected && row.has_message))
                        .map(Source::Peer),
                    (size * 3 / 10).max(24),
                    row.has_sender_avatar,
                ),
                (
                    Kind::Owner,
                    sources.owner.clone().filter(|_| selected).map(Source::Peer),
                    (size * 3 / 10).max(24),
                    row.has_owner_avatar,
                ),
            ];
            for (kind, source, size, ready) in requests {
                let token = (row.key.to_string(), kind);
                let ready = ready
                    && self
                        .media
                        .gift_sizes
                        .get(&token)
                        .is_some_and(|loaded| *loaded >= size);
                if ready
                    || self.media.gift_pending.contains_key(&token)
                    || self.media.gift_failed.contains(&token)
                {
                    continue;
                }
                let Some(source) = source else {
                    continue;
                };
                if self.media.gift_pending.len() >= GIFT_JOBS {
                    return;
                }
                self.media.gift_pending.insert(token, size);
                self.bridge.gift_media(
                    ui.get_profile_key().as_str(),
                    row.key.as_str(),
                    source,
                    kind,
                    size,
                );
            }
        }
    }
    pub(super) fn gift_preview(
        &mut self,
        ui: &Desktop,
        key: String,
        gift_key: String,
        pixels: crate::media::Pixels,
        kind: Kind,
    ) {
        let token = (gift_key.clone(), kind);
        let Some(requested_size) = self.media.gift_pending.remove(&token) else {
            return;
        };
        if !profile_visible(ui)
            || ui.get_profile_key() != key
            || !ui.global::<Theme>().get_show_gifts()
        {
            return;
        }
        let model = ui.get_profile_gifts();
        if let Some(index) = self
            .media
            .gift_view
            .iter()
            .copied()
            .find(|i| model.row_data(*i).is_some_and(|row| row.key == gift_key))
        {
            let mut row = model.row_data(index).unwrap();
            let before = row_bytes(&row);
            let replacing = match kind {
                Kind::Model => image_bytes(&row.photo),
                Kind::Pattern => image_bytes(&row.pattern),
                Kind::Sender => image_bytes(&row.sender_avatar),
                Kind::Owner => image_bytes(&row.owner_avatar),
            };
            if self
                .media
                .gift_bytes
                .saturating_sub(replacing)
                .saturating_add(pixels.len())
                <= GIFT_BYTES
            {
                let image = pixels.image();
                match kind {
                    Kind::Model => {
                        row.photo = image;
                        row.has_photo = true;
                    }
                    Kind::Pattern => {
                        row.pattern = image;
                        row.has_pattern = true;
                    }
                    Kind::Sender => {
                        row.sender_avatar = image;
                        row.has_sender_avatar = true;
                    }
                    Kind::Owner => {
                        row.owner_avatar = image;
                        row.has_owner_avatar = true;
                    }
                }
                self.media.gift_bytes =
                    self.media.gift_bytes.saturating_sub(before) + row_bytes(&row);
                // A small source is deliberately not upscaled. Record the
                // requested fit size, not the raster width, to avoid redownload loops.
                self.media.gift_sizes.insert(token, requested_size);
                self.media.gift_resident.insert(index);
                model.set_row_data(index, row);
            } else {
                self.media.gift_failed.insert(token);
            }
        }
        self.refresh_gift_media(ui);
    }
    pub(super) fn gift_failed(&mut self, ui: &Desktop, key: String, gift_key: String, kind: Kind) {
        if ui.get_profile_key() != key {
            return;
        }
        let token = (gift_key, kind);
        if self.media.gift_pending.remove(&token).is_some() {
            self.media.gift_failed.insert(token);
            self.refresh_gift_media(ui);
        }
    }
}

pub(super) fn install(ui: &Desktop, state: &Rc<RefCell<Demo>>) {
    ui.on_invalidate_file_preview(local_preview::cancel);
    let (weak, demo) = (ui.as_weak(), Rc::clone(state));
    ui.on_media_state_changed(move || {
        if let Some(ui) = weak.upgrade() {
            demo.borrow_mut().release_media(&ui);
        }
    });
    let (weak, demo) = (ui.as_weak(), Rc::clone(state));
    ui.on_request_gift_media(move || {
        if let Some(ui) = weak.upgrade() {
            demo.borrow_mut().refresh_gift_media(&ui);
        }
    });
    let (weak, demo) = (ui.as_weak(), Rc::clone(state));
    ui.on_detail_visibility(move |mid, visible| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut state = demo.borrow_mut();
        let peer = ui.get_active_chat().id;
        if visible && ui.get_window_visible() && !ui.get_auth_required() {
            state.media.visible_details.insert((peer, mid));
        } else {
            state.media.visible_details.remove(&(peer, mid));
            state.evict_preview(peer, mid);
        }
    });
}
