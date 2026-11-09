use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use grammers_client::{Client, SignInError, client::PasswordToken, update::Update};
use grammers_mtsender::SenderPool;
use grammers_session::{Session, storages::SqliteSession, types::PeerRef};
use grammers_tl_types as tl;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};
use tokio::sync::mpsc as async_mpsc;

pub const TG_API_ID: i32 = 39270007;
pub const TG_API_HASH: &str = match option_env!("TG_API_HASH") {
    Some(value) => value,
    None => "",
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, Default)]
pub struct TgMessage {
    pub id: i32,
    pub date: String,
    pub has_media: bool,
    pub is_photo: bool,
    pub body: String,
    pub time: String,
    pub outgoing: bool,
    pub channel_post: bool,
    pub service: bool,
    pub sender: String,
    pub reply_to: i32,
    pub reply_quote: String,
    pub reply_external: bool,
    pub photo_width: i32,
    pub photo_height: i32,
    pub media_key: String,
}
#[derive(Debug, Clone)]
pub struct TgDialog {
    pub id: i32,
    pub title: String,
    pub initials: String,
    pub unread: i32,
    pub read_outbox_max_id: i32,
    pub folder: i32,
    pub saved: bool,
    pub channel: ChannelAccess,
    pub messages: Vec<TgMessage>,
    pub presence: String,
    pub online: bool,
}
#[derive(Debug, Clone, Copy)]
pub enum ChannelAccess {
    Chat = 0,
    ReadOnly = 1,
    Publish = 2,
    CanJoin = 3,
}

impl ChannelAccess {
    fn for_peer(peer: &grammers_client::peer::Peer) -> Self {
        let grammers_client::peer::Peer::Channel(channel) = peer else {
            return Self::Chat;
        };
        if channel.raw.creator
            || matches!(&channel.raw.admin_rights, Some(tl::enums::ChatAdminRights::Rights(rights)) if rights.post_messages)
        {
            Self::Publish
        } else if channel.raw.left {
            Self::CanJoin
        } else {
            Self::ReadOnly
        }
    }
}
#[derive(Debug, Clone)]
pub struct TgFolder {
    pub id: i32,
    pub title: String,
    pub chat_ids: Vec<i32>,
}
#[derive(Debug, Clone)]
pub enum TgEvent {
    Presence {
        peer_id: i32,
        text: String,
        online: bool,
    },
    Thumbnail {
        peer_id: i32,
        message_id: i32,
        key: String,
        pixels: Option<image::RgbaImage>,
    },
    ReplyResolved {
        peer_id: i32,
        message_id: i32,
        original: Option<TgMessage>,
    },
    JumpReady {
        peer_id: i32,
        message_id: i32,
        messages: Vec<TgMessage>,
    },
    Qr {
        matrix: String,
    },
    CodeSent {
        phone: String,
        delivery: String,
    },
    NeedPassword {
        hint: String,
    },
    Authorized {
        name: String,
        phone: String,
        username: String,
        initials: String,
        dialogs: Vec<TgDialog>,
        folders: Vec<TgFolder>,
    },
    History {
        peer_id: i32,
        messages: Vec<TgMessage>,
    },
    OutboxRead {
        peer_id: i32,
        max_id: i32,
    },
    NewMessage {
        peer_id: i32,
        message: TgMessage,
    },
    MessageSent {
        peer_id: i32,
        message: TgMessage,
    },
    MessageEdited {
        peer_id: i32,
        message: TgMessage,
    },
    MessagesDeleted {
        peer_id: i32,
        ids: Vec<i32>,
    },
    OlderHistory {
        peer_id: i32,
        messages: Vec<TgMessage>,
    },
    SearchResults {
        peer_id: i32,
        messages: Vec<TgMessage>,
    },
    OperationDone {
        message: String,
    },
    EditCompleted {
        peer_id: i32,
        message_id: i32,
    },
    SendFailed {
        message: String,
    },
    DialogOpened {
        dialog: TgDialog,
    },
    DialogAdded {
        dialog: TgDialog,
    },
    PhotoReady {
        peer_id: i32,
        message_id: i32,
        pixels: image::RgbaImage,
    },
    AvatarReady {
        peer_id: i32,
        pixels: image::RgbaImage,
    },
    ChannelJoined {
        peer_id: i32,
        access: ChannelAccess,
    },
    PhotoFailed {
        peer_id: i32,
        message_id: i32,
        message: String,
    },
    Folders {
        folders: Vec<TgFolder>,
    },
    LoggedOut,
    Error {
        message: String,
    },
}
pub(crate) enum TgCommand {
    Thumbnail(i32, i32),
    ResolveReply(i32, i32),
    JumpReply(i32, i32),
    StartQr,
    CancelQr,
    SendCode(String),
    SignInCode(String),
    SignInPassword(String),
    LoadAvatar(i32),
    LoadHistory(i32),
    SendMessage(i32, String, Option<i32>),
    SendFile(i32, PathBuf, String, Option<i32>),
    Download(i32, i32),
    ViewPhoto(i32, i32),
    JoinChannel(i32),
    Edit(i32, i32, String),
    Delete(i32, i32),
    Older(i32, i32),
    Search(i32, String),
    Read(i32),
    OpenUsername(String),
    Forward(i32, i32, i32),
    Logout,
}
pub struct TelegramBridge {
    commands: async_mpsc::UnboundedSender<TgCommand>,
    events: mpsc::Receiver<TgEvent>,
    errors: EventSink,
}

type Wake = Box<dyn Fn() + Send + Sync>;
#[derive(Clone)]
pub struct EventSink {
    tx: mpsc::Sender<TgEvent>,
    wake: Arc<Mutex<Option<Wake>>>,
    notified: Arc<AtomicBool>,
}
impl EventSink {
    fn new(tx: mpsc::Sender<TgEvent>) -> Self {
        Self {
            tx,
            wake: Default::default(),
            notified: Default::default(),
        }
    }
    pub fn send(&self, event: TgEvent) -> std::result::Result<(), mpsc::SendError<TgEvent>> {
        self.tx.send(event)?;
        if !self.notified.swap(true, Ordering::AcqRel) {
            if let Some(wake) = self.wake.lock().unwrap().as_ref() {
                wake();
            }
        }
        Ok(())
    }
}
impl TelegramBridge {
    pub fn notify_with(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.errors.wake.lock().unwrap() = Some(Box::new(wake));
        self.errors.notified.store(false, Ordering::Release);
        if let Some(wake) = self.errors.wake.lock().unwrap().as_ref() {
            wake();
        }
    }
    pub fn begin_poll(&self) {
        self.errors.notified.store(false, Ordering::Release);
    }
    pub fn thumbnail(&self, peer: i32, id: i32) {
        self.send(TgCommand::Thumbnail(peer, id));
    }
    pub fn resolve_reply(&self, peer: i32, id: i32) {
        self.send(TgCommand::ResolveReply(peer, id));
    }
    pub fn jump_reply(&self, peer: i32, id: i32) {
        self.send(TgCommand::JumpReply(peer, id));
    }
    #[cfg(test)]
    pub fn offline_for_rendering() -> (Self, async_mpsc::UnboundedReceiver<TgCommand>, EventSink) {
        let (tx, events) = mpsc::channel();
        let errors = EventSink::new(tx);
        let (commands, receiver) = async_mpsc::unbounded_channel();
        (
            Self {
                commands,
                events,
                errors: errors.clone(),
            },
            receiver,
            errors,
        )
    }

    pub fn spawn() -> Self {
        let (tx, events) = mpsc::channel();
        let tx = EventSink::new(tx);
        let (commands, rx) = async_mpsc::unbounded_channel();
        let errors = tx.clone();
        thread::spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => {
                    if let Err(e) = rt.block_on(run(rx, tx.clone())) {
                        report(&tx, e);
                    }
                }
                Err(e) => report(&tx, e),
            }
        });
        Self {
            commands,
            events,
            errors,
        }
    }
    pub fn try_recv(&self) -> Option<TgEvent> {
        self.events.try_recv().ok()
    }
    fn send(&self, cmd: TgCommand) {
        if let Err(error) = self.commands.send(cmd) {
            let message = "Соединение Telegram остановлено. Перезапустите приложение.";
            if matches!(
                error.0,
                TgCommand::SendMessage(..) | TgCommand::SendFile(..)
            ) {
                let _ = self.errors.send(TgEvent::SendFailed {
                    message: message.into(),
                });
            } else {
                report(&self.errors, message);
            }
        }
    }
    pub fn start_qr(&mut self) {
        self.send(TgCommand::StartQr);
    }
    pub fn cancel_qr(&mut self) {
        self.send(TgCommand::CancelQr);
    }
    pub fn send_code(&mut self, phone: &str) {
        self.send(TgCommand::SendCode(phone.to_owned()));
    }
    pub fn sign_in_code(&mut self, code: &str) {
        self.send(TgCommand::SignInCode(code.to_owned()));
    }
    pub fn sign_in_password(&mut self, password: &str) {
        self.send(TgCommand::SignInPassword(password.to_owned()));
    }
    pub fn load_history(&mut self, id: i32) {
        self.send(TgCommand::LoadHistory(id));
    }
    pub fn load_avatar(&mut self, id: i32) {
        self.send(TgCommand::LoadAvatar(id));
    }
    pub fn send_message(&mut self, id: i32, text: &str, reply: Option<i32>) {
        self.send(TgCommand::SendMessage(id, text.to_owned(), reply));
    }
    pub fn send_file(&mut self, id: i32, path: &str, caption: &str, reply: Option<i32>) {
        self.send(TgCommand::SendFile(
            id,
            path.trim().trim_matches('"').into(),
            caption.into(),
            reply,
        ));
    }
    pub fn download(&mut self, peer: i32, id: i32) {
        self.send(TgCommand::Download(peer, id));
    }
    pub fn view_photo(&mut self, peer: i32, id: i32) {
        self.send(TgCommand::ViewPhoto(peer, id));
    }
    pub fn join_channel(&mut self, peer: i32) {
        self.send(TgCommand::JoinChannel(peer));
    }
    pub fn edit(&mut self, peer: i32, id: i32, text: &str) {
        self.send(TgCommand::Edit(peer, id, text.into()));
    }
    pub fn delete(&mut self, peer: i32, id: i32) {
        self.send(TgCommand::Delete(peer, id));
    }
    pub fn older(&mut self, peer: i32, before: i32) {
        self.send(TgCommand::Older(peer, before));
    }
    pub fn search(&mut self, peer: i32, query: &str) {
        self.send(TgCommand::Search(peer, query.into()));
    }
    pub fn read(&mut self, peer: i32) {
        self.send(TgCommand::Read(peer));
    }
    pub fn open_username(&mut self, username: &str) {
        self.send(TgCommand::OpenUsername(username.into()));
    }
    pub fn forward(&mut self, from: i32, message: i32, to: i32) {
        self.send(TgCommand::Forward(from, message, to));
    }
    pub fn logout(&mut self) {
        self.send(TgCommand::Logout);
    }
}
fn report(tx: &EventSink, error: impl std::fmt::Display) {
    let _ = tx.send(TgEvent::Error {
        message: error.to_string(),
    });
}
fn config_dir() -> PathBuf {
    if let Some(p) = env::var_os("RELAY_CONFIG_DIR").filter(|p| !p.is_empty()) {
        return p.into();
    }
    if let Some(p) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(p).join("relay-desktop");
    }
    PathBuf::from(".")
}
fn initials(name: &str) -> String {
    name.split_whitespace()
        .take(2)
        .filter_map(|s| s.chars().next())
        .flat_map(char::to_uppercase)
        .collect()
}
fn message(m: &grammers_client::message::Message) -> TgMessage {
    let media = m.media();
    let (photo_width, photo_height) = media_dimensions(media.as_ref());
    let (reply_quote, reply_external) = match m.reply_header() {
        Some(tl::enums::MessageReplyHeader::Header(header)) => (
            header.quote_text.unwrap_or_default(),
            header
                .reply_to_peer_id
                .is_some_and(|peer| grammers_session::types::PeerId::from(peer) != m.peer_id()),
        ),
        _ => (String::new(), false),
    };
    let label = match &media {
        Some(grammers_client::media::Media::Photo(_)) if m.text().is_empty() => "Фото".to_owned(),
        Some(grammers_client::media::Media::Photo(_)) => String::new(),
        Some(grammers_client::media::Media::Document(d)) => {
            format!("[File: {}]", d.name().unwrap_or("attachment"))
        }
        Some(grammers_client::media::Media::Sticker(_)) => "[Sticker]".to_owned(),
        Some(grammers_client::media::Media::Contact(_)) => "[Contact]".to_owned(),
        Some(grammers_client::media::Media::Poll(_)) => "[Poll]".to_owned(),
        Some(_) => "[Media]".to_owned(),
        None => String::new(),
    };
    TgMessage {
        sender: m
            .sender()
            .and_then(|sender| sender.name())
            .unwrap_or("Удалённый аккаунт")
            .to_owned(),
        reply_to: m.reply_to_message_id().unwrap_or_default(),
        reply_quote,
        reply_external,
        photo_width,
        photo_height,
        media_key: media_key(media.as_ref()),
        id: m.id(),
        date: m
            .date()
            .with_timezone(&chrono::Local)
            .format("%d.%m.%Y")
            .to_string(),
        has_media: media.is_some(),
        is_photo: matches!(&media, Some(grammers_client::media::Media::Photo(_)))
            || matches!(&media, Some(grammers_client::media::Media::Document(d)) if d.mime_type().is_some_and(|m| m.starts_with("image/"))),
        body: match (label.is_empty(), m.text().is_empty()) {
            (true, _) => m
                .action()
                .map(service_text)
                .unwrap_or_else(|| m.text().into()),
            (false, true) => label,
            _ => format!("{label}\n{}", m.text()),
        },
        time: m
            .date()
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string(),
        outgoing: m.outgoing(),
        channel_post: m.post(),
        service: m.action().is_some(),
    }
}

fn service_text(action: &tl::enums::MessageAction) -> String {
    use tl::enums::MessageAction;
    match action {
        MessageAction::ChannelCreate(_) => "Канал создан".into(),
        MessageAction::ChatCreate(_) => "Группа создана".into(),
        MessageAction::ChatEditTitle(value) => format!("Название изменено на «{}»", value.title),
        MessageAction::ChatEditPhoto(_) => "Фото чата обновлено".into(),
        MessageAction::ChatDeletePhoto => "Фото чата удалено".into(),
        MessageAction::ChatAddUser(_) => "Добавлен участник".into(),
        MessageAction::ChatDeleteUser(_) => "Участник покинул чат".into(),
        MessageAction::ChatJoinedByLink(_) | MessageAction::ChatJoinedByRequest => {
            "Новый участник в группе".into()
        }
        MessageAction::PinMessage => "Сообщение закреплено".into(),
        MessageAction::HistoryClear => "История очищена".into(),
        MessageAction::ContactSignUp => "Контакт присоединился к Telegram".into(),
        MessageAction::CustomAction(value) => value.message.clone(),
        _ => "Событие в чате".into(),
    }
}
struct LoginToken {
    phone: String,
    hash: String,
}
struct Worker {
    client: Client,
    session: Arc<SqliteSession>,
    tx: EventSink,
    login: Option<LoginToken>,
    password: Option<PasswordToken>,
    qr: bool,
    qr_expires: i64,
    peers: HashMap<i32, PeerRef>,
    avatar_peers: HashMap<i32, grammers_client::peer::Peer>,
    avatar_requests: HashSet<i32>,
    avatar_slots: Arc<tokio::sync::Semaphore>,
    avatar_tasks: tokio::task::JoinSet<()>,
    detail_slots: Arc<tokio::sync::Semaphore>,
    online_until: HashMap<i32, i64>,
    folder_defs: Vec<tl::enums::DialogFilter>,
    folder_dialogs: Vec<(i32, grammers_client::peer::Dialog)>,
}
fn has_peer(list: &[tl::enums::InputPeer], d: &grammers_client::peer::Dialog) -> bool {
    list.iter().any(|p| match p {
        tl::enums::InputPeer::Empty => false,
        tl::enums::InputPeer::PeerSelf => {
            matches!(d.peer(), grammers_client::peer::Peer::User(u) if u.is_self())
        }
        p => PeerRef::from(p).id == d.peer_id(),
    })
}
fn filter_contains(f: &tl::enums::DialogFilter, d: &grammers_client::peer::Dialog) -> bool {
    use grammers_client::peer::Peer;
    match f {
        tl::enums::DialogFilter::Default => true,
        tl::enums::DialogFilter::Chatlist(f) => {
            has_peer(&f.pinned_peers, d) || has_peer(&f.include_peers, d)
        }
        tl::enums::DialogFilter::Filter(f) => {
            if has_peer(&f.exclude_peers, d) {
                return false;
            }
            if has_peer(&f.pinned_peers, d) || has_peer(&f.include_peers, d) {
                return true;
            }
            if let tl::enums::Dialog::Dialog(raw) = &d.raw {
                let tl::enums::PeerNotifySettings::Settings(settings) = &raw.notify_settings;
                if (f.exclude_read && raw.unread_count == 0 && !raw.unread_mark)
                    || (f.exclude_archived && raw.folder_id == Some(1))
                    || (f.exclude_muted
                        && settings
                            .mute_until
                            .is_some_and(|t| i64::from(t) > chrono::Utc::now().timestamp()))
                {
                    return false;
                }
            }
            match d.peer() {
                Peer::User(u) if u.is_bot() => f.bots,
                Peer::User(u) => {
                    let contact = matches!(&u.raw, tl::enums::User::User(raw) if raw.contact);
                    if contact { f.contacts } else { f.non_contacts }
                }
                Peer::Group(_) => f.groups,
                Peer::Channel(_) => f.broadcasts,
            }
        }
    }
}
impl Worker {
    fn presence_update(&mut self, user_id: i64, status: tl::enums::UserStatus) {
        let now = chrono::Utc::now().timestamp();
        for (&id, peer) in &self.peers {
            if peer.id.kind() == grammers_session::types::PeerKind::User
                && peer.id.bare_id_unchecked() == user_id
            {
                self.online_until.remove(&id);
                if let tl::enums::UserStatus::Online(value) = &status {
                    if i64::from(value.expires) > now {
                        self.online_until.insert(id, i64::from(value.expires));
                    }
                }
                let (text, online) = crate::presence::describe(&status, now);
                let _ = self.tx.send(TgEvent::Presence {
                    peer_id: id,
                    text,
                    online,
                });
            }
        }
    }
    fn track_presence(&mut self, id: i32, peer: &grammers_client::peer::Peer) {
        if let grammers_client::peer::Peer::User(user) = peer {
            if let tl::enums::UserStatus::Online(value) = user.status() {
                self.online_until.insert(id, i64::from(value.expires));
            }
        }
    }
    fn load_avatar(&mut self, id: i32) {
        if id == 3 || self.avatar_requests.contains(&id) {
            return;
        }
        let Some(peer) = self.avatar_peers.get(&id).cloned() else {
            return;
        };
        self.avatar_requests.insert(id);
        let has_photo = match &peer {
            grammers_client::peer::Peer::User(user) => user.photo().is_some(),
            grammers_client::peer::Peer::Group(group) => group.photo().is_some(),
            grammers_client::peer::Peer::Channel(channel) => channel.photo().is_some(),
        };
        if !has_photo {
            return;
        }
        let client = self.client.clone();
        let slots = self.avatar_slots.clone();
        let tx = self.tx.clone();
        self.avatar_tasks.spawn(async move {
            let Ok(_permit) = slots.acquire().await else {
                return;
            };
            let download = async {
                let photo = peer.photo(false).await?.ok_or("Аватар недоступен")?;
                download_image(&client, &photo, 1024 * 1024, 1024).await
            };
            if let Ok(Ok(pixels)) = tokio::time::timeout(Duration::from_secs(15), download).await {
                // Keep enough source detail for high-DPI avatars. Skia samples
                // and clips this image at the actual display scale.
                let pixels = if pixels.width() > 256 || pixels.height() > 256 {
                    image::DynamicImage::ImageRgba8(pixels)
                        .resize(256, 256, image::imageops::FilterType::Lanczos3)
                        .into_rgba8()
                } else {
                    pixels
                };
                let _ = tx.send(TgEvent::AvatarReady {
                    peer_id: id,
                    pixels,
                });
            }
        });
    }
    fn outbox_read(&self, peer: grammers_session::types::PeerId, max_id: i32) {
        if let Some((&peer_id, _)) = self.peers.iter().find(|(_, p)| p.id == peer) {
            let _ = self.tx.send(TgEvent::OutboxRead { peer_id, max_id });
        }
    }
    fn folder_rows(&self) -> Vec<TgFolder> {
        let mut rows = vec![];
        for f in &self.folder_defs {
            let (id, title, pinned) = match f {
                tl::enums::DialogFilter::Default => (0, "Все чаты".to_owned(), &[][..]),
                tl::enums::DialogFilter::Filter(f) => {
                    let tl::enums::TextWithEntities::Entities(t) = &f.title;
                    (f.id, t.text.clone(), f.pinned_peers.as_slice())
                }
                tl::enums::DialogFilter::Chatlist(f) => {
                    let tl::enums::TextWithEntities::Entities(t) = &f.title;
                    (f.id, t.text.clone(), f.pinned_peers.as_slice())
                }
            };
            let mut chats: Vec<_> = self
                .folder_dialogs
                .iter()
                .filter(|(_, d)| filter_contains(f, d))
                .collect();
            chats.sort_by_key(|(_, d)| {
                pinned
                    .iter()
                    .position(|p| has_peer(std::slice::from_ref(p), d))
                    .unwrap_or(usize::MAX)
            });
            rows.push(TgFolder {
                id,
                title,
                chat_ids: chats.into_iter().map(|(id, _)| *id).collect(),
            });
        }
        if !rows.iter().any(|f| f.id == 0) {
            rows.insert(
                0,
                TgFolder {
                    id: 0,
                    title: "Все чаты".into(),
                    chat_ids: self.folder_dialogs.iter().map(|(id, _)| *id).collect(),
                },
            );
        }
        rows
    }
    async fn reload_folders(&mut self) -> Result<()> {
        let tl::enums::messages::DialogFilters::Filters(filters) = self
            .client
            .invoke(&tl::functions::messages::GetDialogFilters {})
            .await?;
        self.folder_defs = filters.filters;
        Ok(())
    }
    async fn register_peer(&mut self, peer: &grammers_client::peer::Peer) -> Result<i32> {
        if let Some((&id, _)) = self.peers.iter().find(|(_, p)| p.id == peer.id()) {
            return Ok(id);
        }
        let reference = peer
            .to_ref()
            .await?
            .ok_or("Telegram не вернул доступ к чату.")?;
        let id = self.peers.keys().max().copied().unwrap_or(10) + 1;
        self.peers.insert(id, reference);
        self.avatar_peers.insert(id, peer.clone());
        self.track_presence(id, peer);
        let title = peer.name().unwrap_or("Chat").to_owned();
        let _ = self.tx.send(TgEvent::DialogAdded {
            dialog: TgDialog {
                id,
                initials: initials(&title),
                title,
                unread: 0,
                read_outbox_max_id: 0,
                folder: if matches!(peer, grammers_client::peer::Peer::User(_)) {
                    2
                } else {
                    1
                },
                saved: false,
                channel: ChannelAccess::for_peer(peer),
                messages: vec![],
                presence: crate::presence::for_peer(peer).0,
                online: crate::presence::for_peer(peer).1,
            },
        });
        Ok(id)
    }
    fn clear_qr(&self) {
        let _ = self.tx.send(TgEvent::Qr {
            matrix: String::new(),
        });
    }
    async fn need_password(&mut self) -> Result<()> {
        let tl::enums::account::Password::Password(p) = self
            .client
            .invoke(&tl::functions::account::GetPassword {})
            .await?;
        self.set_password(PasswordToken::new(p));
        Ok(())
    }
    fn set_password(&mut self, token: PasswordToken) {
        self.qr = false;
        self.clear_qr();
        let hint = token.hint().unwrap_or_default().to_owned();
        self.password = Some(token);
        let _ = self.tx.send(TgEvent::NeedPassword { hint });
    }
    async fn refresh_qr(&mut self) -> Result<()> {
        let request = tl::functions::auth::ExportLoginToken {
            api_id: TG_API_ID,
            api_hash: TG_API_HASH.into(),
            except_ids: vec![],
        };
        let mut token = match self.client.invoke(&request).await {
            Ok(t) => t,
            Err(e) if e.to_string().contains("SESSION_PASSWORD_NEEDED") => {
                return self.need_password().await;
            }
            Err(e) => return Err(e.into()),
        };
        if let tl::enums::auth::LoginToken::MigrateTo(m) = token {
            self.session.set_home_dc_id(m.dc_id).await?;
            token = match self
                .client
                .invoke(&tl::functions::auth::ImportLoginToken { token: m.token })
                .await
            {
                Ok(t) => t,
                Err(e) if e.to_string().contains("SESSION_PASSWORD_NEEDED") => {
                    return self.need_password().await;
                }
                Err(e) => return Err(e.into()),
            };
        }
        match token {
            tl::enums::auth::LoginToken::Token(t) => {
                self.qr_expires = i64::from(t.expires);
                let url = format!("tg://login?token={}", URL_SAFE_NO_PAD.encode(t.token));
                let _ = self.tx.send(TgEvent::Qr {
                    matrix: crate::qr::encode_qr_matrix(&url),
                });
            }
            tl::enums::auth::LoginToken::Success(_) => self.authorized().await?,
            _ => return Err("Telegram повторно запросил смену сервера. Обновите QR-код.".into()),
        }
        Ok(())
    }
    async fn authorized(&mut self) -> Result<()> {
        self.qr = false;
        self.login = None;
        self.password = None;
        self.clear_qr();
        let me = self.client.get_me().await?;
        if let Some(p) = me.to_ref().await? {
            self.session
                .cache_peer(&grammers_session::types::PeerInfo::User {
                    id: me.id().bare_id_unchecked(),
                    auth: Some(p.auth),
                    bot: Some(false),
                    is_self: Some(true),
                })
                .await?;
        }
        let name = format!(
            "{} {}",
            me.first_name().unwrap_or_default(),
            me.last_name().unwrap_or_default()
        )
        .trim()
        .to_owned();
        let mut dialogs = vec![];
        self.avatar_tasks.abort_all();
        self.peers.clear();
        self.avatar_peers.clear();
        self.avatar_requests.clear();
        self.online_until.clear();
        self.folder_dialogs.clear();
        let mut iter = self.client.iter_dialogs();
        while dialogs.len() < 500 {
            let Some(d) = iter.next().await? else { break };
            let saved = d.peer_id() == me.id();
            let id = if saved { 3 } else { 10 + dialogs.len() as i32 };
            self.peers.insert(id, d.peer_ref());
            self.avatar_peers.insert(id, d.peer().clone());
            self.track_presence(id, d.peer());
            self.folder_dialogs.push((id, d.clone()));
            let title = if saved {
                "Saved Messages"
            } else {
                d.peer().name().unwrap_or("Chat")
            }
            .to_owned();
            let (unread, read_outbox_max_id) = match &d.raw {
                tl::enums::Dialog::Dialog(d) => (d.unread_count, d.read_outbox_max_id),
                _ => (0, 0),
            };
            dialogs.push(TgDialog {
                id,
                initials: initials(&title),
                title,
                unread,
                read_outbox_max_id,
                folder: if matches!(d.peer(), grammers_client::peer::Peer::User(_)) {
                    2
                } else {
                    1
                },
                saved,
                channel: ChannelAccess::for_peer(d.peer()),
                messages: d.last_message.as_ref().map(message).into_iter().collect(),
                presence: crate::presence::for_peer(d.peer()).0,
                online: crate::presence::for_peer(d.peer()).1,
            });
        }
        let _ = self.tx.send(TgEvent::Authorized {
            initials: initials(&name),
            name,
            phone: me.phone().map(|p| format!("+{p}")).unwrap_or_default(),
            username: me.username().map(|u| format!("@{u}")).unwrap_or_default(),
            dialogs,
            folders: self.folder_rows(),
        });
        if let Ok(Ok(())) =
            tokio::time::timeout(Duration::from_secs(10), self.reload_folders()).await
        {
            let _ = self.tx.send(TgEvent::Folders {
                folders: self.folder_rows(),
            });
        }
        Ok(())
    }
    async fn command(&mut self, cmd: TgCommand) -> Result<()> {
        match cmd {
            TgCommand::Thumbnail(id, mid) | TgCommand::ResolveReply(id, mid) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let thumbnail = matches!(cmd, TgCommand::Thumbnail(..));
                let client = self.client.clone();
                let tx = self.tx.clone();
                let slots = self.detail_slots.clone();
                self.avatar_tasks.spawn(async move {
                    let Ok(_permit) = slots.acquire().await else {
                        return;
                    };
                    let result = tokio::time::timeout(Duration::from_secs(25), async {
                        let source = client
                            .get_messages_by_id(peer, &[mid])
                            .await?
                            .into_iter()
                            .flatten()
                            .next()
                            .ok_or("Сообщение недоступно")?;
                        if thumbnail {
                            let pixels = thumbnail_image(&client, &source).await?;
                            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TgEvent::Thumbnail {
                                peer_id: id,
                                message_id: mid,
                                key: media_key(source.media().as_ref()),
                                pixels: Some(pixels),
                            })
                        } else {
                            let original = source.get_reply().await?.as_ref().map(message);
                            Ok(TgEvent::ReplyResolved {
                                peer_id: id,
                                message_id: mid,
                                original,
                            })
                        }
                    })
                    .await;
                    let event = result
                        .ok()
                        .and_then(|result| result.ok())
                        .unwrap_or_else(|| {
                            if thumbnail {
                                TgEvent::Thumbnail {
                                    peer_id: id,
                                    message_id: mid,
                                    key: String::new(),
                                    pixels: None,
                                }
                            } else {
                                TgEvent::ReplyResolved {
                                    peer_id: id,
                                    message_id: mid,
                                    original: None,
                                }
                            }
                        });
                    let _ = tx.send(event);
                });
            }
            TgCommand::JumpReply(id, mid) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let source = self
                    .client
                    .get_messages_by_id(peer, &[mid])
                    .await?
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or("Сообщение удалено")?;
                let original = source
                    .get_reply()
                    .await?
                    .ok_or("Исходное сообщение удалено или недоступно")?;
                let target = match original.peer() {
                    Some(peer) => self.register_peer(peer).await?,
                    None => id,
                };
                let target_peer = *self.peers.get(&target).ok_or("Чат недоступен")?;
                let mut iter = self
                    .client
                    .iter_messages(target_peer)
                    .offset_id(original.id().saturating_add(20))
                    .limit(40);
                let mut messages = Vec::new();
                while let Some(item) = iter.next().await? {
                    messages.push(message(&item));
                }
                messages.reverse();
                let _ = self.tx.send(TgEvent::JumpReady {
                    peer_id: target,
                    message_id: original.id(),
                    messages,
                });
            }
            TgCommand::LoadAvatar(id) => {
                self.load_avatar(id);
            }
            TgCommand::JoinChannel(id) => {
                let peer = *self.peers.get(&id).ok_or("Канал не найден.")?;
                let result = self
                    .client
                    .invoke(&tl::functions::channels::JoinChannel {
                        channel: peer.into(),
                    })
                    .await?;
                if !matches!(result, tl::enums::messages::ChatInviteJoinResult::Ok(_)) {
                    return Err(
                        "Telegram требует завершить подписку в официальном приложении.".into(),
                    );
                }
                let _ = self.tx.send(TgEvent::ChannelJoined {
                    peer_id: id,
                    access: ChannelAccess::ReadOnly,
                });
            }
            TgCommand::ViewPhoto(id, mid) => {
                let Some(peer) = self.peers.get(&id).copied() else {
                    let _ = self.tx.send(TgEvent::PhotoFailed {
                        peer_id: id,
                        message_id: mid,
                        message: "Чат не найден.".into(),
                    });
                    return Ok(());
                };
                let client = self.client.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let result = tokio::time::timeout(
                        Duration::from_secs(45),
                        fetch_photo(client, peer, mid),
                    )
                    .await;
                    let event = match result {
                        Ok(Ok(pixels)) => TgEvent::PhotoReady {
                            peer_id: id,
                            message_id: mid,
                            pixels,
                        },
                        Ok(Err(e)) => TgEvent::PhotoFailed {
                            peer_id: id,
                            message_id: mid,
                            message: e.to_string(),
                        },
                        Err(_) => TgEvent::PhotoFailed {
                            peer_id: id,
                            message_id: mid,
                            message: "Telegram не загрузил фото вовремя.".into(),
                        },
                    };
                    let _ = tx.send(event);
                });
            }
            TgCommand::OpenUsername(username) => {
                let name = username
                    .trim()
                    .trim_start_matches("https://t.me/")
                    .trim_start_matches('@');
                let peer = self
                    .client
                    .resolve_username(name)
                    .await?
                    .ok_or("Пользователь или канал не найден.")?;
                let reference = peer
                    .to_ref()
                    .await?
                    .ok_or("Telegram не вернул доступ к чату.")?;
                let id = self
                    .peers
                    .iter()
                    .find_map(|(&id, p)| (p.id == peer.id()).then_some(id))
                    .unwrap_or_else(|| self.peers.keys().max().copied().unwrap_or(10) + 1);
                self.peers.insert(id, reference);
                self.avatar_peers.insert(id, peer.clone());
                self.track_presence(id, &peer);
                let title = peer.name().unwrap_or(name).to_owned();
                let _ = self.tx.send(TgEvent::DialogOpened {
                    dialog: TgDialog {
                        id,
                        initials: initials(&title),
                        title,
                        unread: 0,
                        read_outbox_max_id: 0,
                        folder: if matches!(peer, grammers_client::peer::Peer::User(_)) {
                            2
                        } else {
                            1
                        },
                        saved: false,
                        channel: ChannelAccess::for_peer(&peer),
                        messages: vec![],
                        presence: crate::presence::for_peer(&peer).0,
                        online: crate::presence::for_peer(&peer).1,
                    },
                });
            }
            TgCommand::Forward(from, mid, to) => {
                let source = *self.peers.get(&from).ok_or("Исходный чат не найден.")?;
                let target = *self.peers.get(&to).ok_or("Чат назначения не найден.")?;
                for m in self
                    .client
                    .forward_messages(target, &[mid], source)
                    .await?
                    .into_iter()
                    .flatten()
                {
                    let _ = self.tx.send(TgEvent::NewMessage {
                        peer_id: to,
                        message: message(&m),
                    });
                }
                let _ = self.tx.send(TgEvent::OperationDone {
                    message: "Сообщение переслано".into(),
                });
            }
            TgCommand::CancelQr => {
                self.qr = false;
                self.clear_qr();
            }
            TgCommand::StartQr => {
                self.login = None;
                self.password = None;
                self.qr = true;
                self.clear_qr();
                self.refresh_qr().await?;
            }
            TgCommand::SendCode(phone) => {
                self.qr = false;
                self.clear_qr();
                self.login = None;
                self.password = None;
                let phone: String = phone
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '+')
                    .collect();
                if !phone.starts_with('+') || phone.len() < 8 {
                    return Err("Введите номер с кодом страны, например +7…".into());
                }
                let request = tl::functions::auth::SendCode {
                    phone_number: phone.clone(),
                    api_id: TG_API_ID,
                    api_hash: TG_API_HASH.into(),
                    settings: tl::types::CodeSettings {
                        allow_flashcall: false,
                        current_number: false,
                        allow_app_hash: false,
                        allow_missed_call: false,
                        allow_firebase: false,
                        unknown_number: false,
                        logout_tokens: None,
                        token: None,
                        app_sandbox: None,
                    }
                    .into(),
                };
                let mut sent = self.client.invoke(&request).await;
                if let Err(grammers_mtsender::InvocationError::Rpc(ref e)) = sent {
                    if e.code == 303 {
                        if let Some(dc) = e.value {
                            self.session.set_home_dc_id(dc as i32).await?;
                            sent = self.client.invoke(&request).await;
                        }
                    }
                }
                match sent {
                    Ok(tl::enums::auth::SentCode::Code(token)) => {
                        let delivery = match token.r#type {
                            tl::enums::auth::SentCodeType::App(_) => "Код отправлен в Telegram на другом устройстве.".to_owned(),
                            tl::enums::auth::SentCodeType::Sms(_) | tl::enums::auth::SentCodeType::SmsWord(_) | tl::enums::auth::SentCodeType::SmsPhrase(_) => "Код отправлен по SMS.".to_owned(),
                            tl::enums::auth::SentCodeType::Call(_) | tl::enums::auth::SentCodeType::FlashCall(_) | tl::enums::auth::SentCodeType::MissedCall(_) => "Telegram отправит код телефонным звонком.".to_owned(),
                            tl::enums::auth::SentCodeType::EmailCode(e) => format!("Код отправлен на {}", e.email_pattern),
                            tl::enums::auth::SentCodeType::SetUpEmailRequired(_) => return Err("Telegram требует настройку почты. Завершите настройку в официальном приложении или войдите по QR.".into()),
                            _ => "Telegram выбрал другой способ доставки. Используйте QR-вход, если код не приходит.".to_owned(),
                        };
                        self.login = Some(LoginToken { phone: phone.clone(), hash: token.phone_code_hash });
                        let _ = self.tx.send(TgEvent::CodeSent { phone, delivery });
                    }
                    Ok(tl::enums::auth::SentCode::Success(_)) => self.authorized().await?,
                    Ok(tl::enums::auth::SentCode::PaymentRequired(_)) => return Err("Telegram требует оплату доставки кода. Используйте QR-вход или официальное приложение.".into()),
                    Err(e) if e.to_string().contains("SESSION_PASSWORD_NEEDED") => {
                        self.need_password().await?
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TgCommand::SignInCode(code) => {
                let token = self.login.as_ref().ok_or("Сначала запросите код входа.")?;
                match self
                    .client
                    .invoke(&tl::functions::auth::SignIn {
                        phone_number: token.phone.clone(),
                        phone_code_hash: token.hash.clone(),
                        phone_code: Some(code.trim().into()),
                        email_verification: None,
                    })
                    .await
                {
                    Ok(tl::enums::auth::Authorization::Authorization(_)) => {
                        self.authorized().await?
                    }
                    Ok(tl::enums::auth::Authorization::SignUpRequired(_)) => {
                        return Err(
                            "Сначала зарегистрируйте аккаунт в официальном Telegram.".into()
                        );
                    }
                    Err(e) if e.to_string().contains("SESSION_PASSWORD_NEEDED") => {
                        self.need_password().await?
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            TgCommand::SignInPassword(password) => {
                let token = self
                    .password
                    .take()
                    .ok_or("Сначала начните вход в Telegram.")?;
                match self.client.check_password(token, password).await {
                    Ok(_) => self.authorized().await?,
                    Err(SignInError::InvalidPassword(p) | SignInError::PasswordRequired(p)) => {
                        self.password = Some(p);
                        return Err("Неверный облачный пароль.".into());
                    }
                    Err(e) => {
                        self.need_password().await?;
                        return Err(e.into());
                    }
                }
            }
            TgCommand::LoadHistory(id) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let mut iter = self.client.iter_messages(peer).limit(40);
                let mut messages = vec![];
                while let Some(m) = iter.next().await? {
                    messages.push(message(&m));
                }
                messages.reverse();
                let _ = self.tx.send(TgEvent::History {
                    peer_id: id,
                    messages,
                });
                // A chat opened by username or first seen in an update may not
                // have been part of the initial dialog list.
                if !self.folder_dialogs.iter().any(|(key, _)| *key == id) {
                    let request = tl::functions::messages::GetPeerDialogs {
                        peers: vec![tl::types::InputDialogPeer { peer: peer.into() }.into()],
                    };
                    if let Ok(tl::enums::messages::PeerDialogs::Dialogs(response)) =
                        self.client.invoke(&request).await
                    {
                        for dialog in response.dialogs {
                            if let tl::enums::Dialog::Dialog(dialog) = dialog {
                                self.outbox_read(peer.id, dialog.read_outbox_max_id);
                            }
                        }
                    }
                }
            }
            TgCommand::SendMessage(id, text, reply) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let m = self
                    .client
                    .send_message(
                        peer,
                        grammers_client::message::InputMessage::new()
                            .text(text)
                            .reply_to(reply),
                    )
                    .await?;
                let _ = self.tx.send(TgEvent::MessageSent {
                    peer_id: id,
                    message: message(&m),
                });
            }
            TgCommand::SendFile(id, path, caption, reply) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                if !path.is_file() {
                    return Err("Файл не найден. Укажите полный путь к файлу.".into());
                }
                let file = self.client.upload_file(path).await?;
                let m = self
                    .client
                    .send_message(
                        peer,
                        grammers_client::message::InputMessage::new()
                            .text(caption)
                            .file(file)
                            .reply_to(reply),
                    )
                    .await?;
                let _ = self.tx.send(TgEvent::MessageSent {
                    peer_id: id,
                    message: message(&m),
                });
            }
            TgCommand::Read(id) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                self.client.mark_as_read(peer).await?;
                if let Some((_, d)) = self.folder_dialogs.iter_mut().find(|(key, _)| *key == id) {
                    if let tl::enums::Dialog::Dialog(d) = &mut d.raw {
                        d.unread_count = 0;
                        d.unread_mark = false;
                    }
                }
                let _ = self.tx.send(TgEvent::Folders {
                    folders: self.folder_rows(),
                });
            }
            TgCommand::Edit(id, mid, text) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                self.client.edit_message(peer, mid, text).await?;
                if let Some(Some(m)) = self
                    .client
                    .get_messages_by_id(peer, &[mid])
                    .await?
                    .into_iter()
                    .next()
                {
                    let _ = self.tx.send(TgEvent::MessageEdited {
                        peer_id: id,
                        message: message(&m),
                    });
                }
                let _ = self.tx.send(TgEvent::EditCompleted {
                    peer_id: id,
                    message_id: mid,
                });
            }
            TgCommand::Delete(id, mid) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                self.client.delete_messages(peer, &[mid]).await?;
                let _ = self.tx.send(TgEvent::MessagesDeleted {
                    peer_id: id,
                    ids: vec![mid],
                });
            }
            TgCommand::Older(id, before) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let mut iter = self.client.iter_messages(peer).offset_id(before).limit(50);
                let mut messages = vec![];
                while let Some(m) = iter.next().await? {
                    messages.push(message(&m));
                }
                messages.reverse();
                let _ = self.tx.send(TgEvent::OlderHistory {
                    peer_id: id,
                    messages,
                });
            }
            TgCommand::Search(id, query) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let mut iter = self.client.search_messages(peer).query(&query).limit(50);
                let mut messages = vec![];
                while let Some(m) = iter.next().await? {
                    messages.push(message(&m));
                }
                messages.reverse();
                let _ = self.tx.send(TgEvent::SearchResults {
                    peer_id: id,
                    messages,
                });
            }
            TgCommand::Download(id, mid) => {
                let peer = *self.peers.get(&id).ok_or("Чат не найден.")?;
                let m = self
                    .client
                    .get_messages_by_id(peer, &[mid])
                    .await?
                    .into_iter()
                    .flatten()
                    .next()
                    .ok_or("Сообщение не найдено.")?;
                let media = m.media().ok_or("В сообщении нет вложения.")?;
                let suggested = match &media {
                    grammers_client::media::Media::Document(d) => {
                        d.name().unwrap_or("attachment").to_owned()
                    }
                    grammers_client::media::Media::Photo(_) => format!("photo-{mid}.jpg"),
                    _ => format!("attachment-{mid}"),
                };
                let safe: String = suggested
                    .chars()
                    .map(|c| {
                        if c.is_control() || "<>:\"/\\|?*".contains(c) {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect();
                let dir = env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(config_dir)
                    .join("Downloads")
                    .join("Relay Desktop");
                fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{id}-{mid}-{safe}"));
                if !m.download_media(&path).await? {
                    return Err("Этот тип вложения нельзя скачать как файл.".into());
                }
                let _ = self.tx.send(TgEvent::OperationDone {
                    message: format!("Сохранено: {}", path.display()),
                });
            }
            TgCommand::Logout => {
                self.online_until.clear();
                self.client.sign_out().await?;
                self.avatar_tasks.abort_all();
                self.peers.clear();
                self.avatar_peers.clear();
                self.avatar_requests.clear();
                self.folder_defs.clear();
                self.folder_dialogs.clear();
                self.login = None;
                self.password = None;
                let _ = self.tx.send(TgEvent::LoggedOut);
                self.qr = true;
                self.refresh_qr().await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod event_tests {
    use super::*;
    #[test]
    fn bridge_coalesces_notifications_and_stays_idle_without_events() {
        let (bridge, _commands, events) = TelegramBridge::offline_for_rendering();
        let wakeups = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = wakeups.clone();
        bridge.notify_with(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(wakeups.load(Ordering::Relaxed), 1);
        bridge.begin_poll();
        events
            .send(TgEvent::OperationDone {
                message: "one".into(),
            })
            .unwrap();
        events
            .send(TgEvent::OperationDone {
                message: "two".into(),
            })
            .unwrap();
        assert_eq!(wakeups.load(Ordering::Relaxed), 2);
        assert!(
            bridge.try_recv().is_some()
                && bridge.try_recv().is_some()
                && bridge.try_recv().is_none()
        );
        bridge.begin_poll();
        assert_eq!(wakeups.load(Ordering::Relaxed), 2);
        events
            .send(TgEvent::OperationDone {
                message: "three".into(),
            })
            .unwrap();
        assert_eq!(wakeups.load(Ordering::Relaxed), 3);
    }
}
fn media_key(media: Option<&grammers_client::media::Media>) -> String {
    use grammers_client::media::Media;
    match media {
        Some(Media::Photo(photo)) => format!("photo:{}", photo.id()),
        Some(Media::Document(document)) => format!("document:{}", document.id()),
        _ => String::new(),
    }
}
fn media_dimensions(media: Option<&grammers_client::media::Media>) -> (i32, i32) {
    use grammers_client::media::Media;
    match media {
        Some(Media::Photo(photo)) => match &photo.raw.photo {
            Some(tl::enums::Photo::Photo(photo)) => photo
                .sizes
                .iter()
                .filter_map(|size| match size {
                    tl::enums::PhotoSize::Size(size) => Some((size.w, size.h)),
                    tl::enums::PhotoSize::PhotoCachedSize(size) => Some((size.w, size.h)),
                    tl::enums::PhotoSize::Progressive(size) => Some((size.w, size.h)),
                    _ => None,
                })
                .max_by_key(|&(w, h)| i64::from(w) * i64::from(h))
                .unwrap_or((320, 240)),
            _ => (320, 240),
        },
        Some(Media::Document(document)) => match &document.raw.document {
            Some(tl::enums::Document::Document(document)) => document
                .attributes
                .iter()
                .find_map(|attribute| match attribute {
                    tl::enums::DocumentAttribute::ImageSize(size) => Some((size.w, size.h)),
                    _ => None,
                })
                .unwrap_or((320, 240)),
            _ => (320, 240),
        },
        _ => (320, 240),
    }
}

async fn thumbnail_image(
    client: &Client,
    message: &grammers_client::message::Message,
) -> Result<image::RgbaImage> {
    use grammers_client::media::Media;
    let media = message.media().ok_or("Фото недоступно")?;
    let thumbs = match &media {
        Media::Photo(photo) => photo.thumbs(),
        Media::Document(doc) => doc.thumbs(),
        _ => vec![],
    };
    let thumb = thumbs
        .into_iter()
        .filter(|thumb| thumb.size() > 0 && thumb.size() <= 3 * 1024 * 1024)
        .min_by_key(|thumb| match thumb.photo_type().as_str() {
            "x" => 0,
            "y" => 1,
            "m" => 2,
            "w" => 3,
            "s" => 4,
            _ => 5,
        });
    let pixels = if let Some(thumb) = thumb {
        download_image(client, &thumb, 3 * 1024 * 1024, 8192).await?
    } else {
        download_image(client, &media, 4 * 1024 * 1024, 8192).await?
    };
    tokio::task::spawn_blocking(move || {
        image::DynamicImage::ImageRgba8(pixels)
            .resize(960, 960, image::imageops::FilterType::Lanczos3)
            .into_rgba8()
    })
    .await
    .map_err(Into::into)
}

async fn fetch_photo(client: Client, peer: PeerRef, mid: i32) -> Result<image::RgbaImage> {
    let message = client
        .get_messages_by_id(peer, &[mid])
        .await?
        .into_iter()
        .flatten()
        .next()
        .ok_or("Фото больше недоступно.")?;
    let media = message.media().ok_or("Фото больше недоступно.")?;
    download_image(&client, &media, 32 * 1024 * 1024, 8192).await
}

async fn download_image(
    client: &Client,
    media: &impl grammers_client::media::Downloadable,
    byte_limit: usize,
    dimension_limit: u32,
) -> Result<image::RgbaImage> {
    let mut download = client.iter_download(media);
    let mut bytes = Vec::new();
    while let Some(chunk) = download.next().await? {
        if bytes.len() + chunk.len() > byte_limit {
            return Err("Фото слишком большое для просмотра. Его можно сохранить как файл.".into());
        }
        bytes.extend(chunk);
    }
    tokio::task::spawn_blocking(move || -> Result<image::RgbaImage> {
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(128 * 1024 * 1024);
        limits.max_image_width = Some(dimension_limit);
        limits.max_image_height = Some(dimension_limit);
        reader.limits(limits);
        Ok(reader.decode()?.to_rgba8())
    })
    .await?
}
async fn run(mut commands: async_mpsc::UnboundedReceiver<TgCommand>, tx: EventSink) -> Result<()> {
    let dir = config_dir();
    fs::create_dir_all(&dir)?;
    let session = Arc::new(SqliteSession::open(dir.join("telegram-native.session")).await?);
    let SenderPool {
        runner,
        handle,
        updates,
    } = SenderPool::new(session.clone(), TG_API_ID);
    let client = Client::with_configuration(
        handle,
        grammers_client::client::ClientConfiguration {
            retry_policy: Box::new(grammers_client::client::NoRetries),
            ..Default::default()
        },
    );
    let _network = tokio::spawn(runner.run());
    let mut updates = client.stream_updates(updates, Default::default()).await?;
    let mut w = Worker {
        client,
        session,
        tx: tx.clone(),
        login: None,
        password: None,
        qr: true,
        qr_expires: 0,
        peers: HashMap::new(),
        avatar_peers: HashMap::new(),
        avatar_requests: HashSet::new(),
        avatar_slots: Arc::new(tokio::sync::Semaphore::new(4)),
        avatar_tasks: tokio::task::JoinSet::new(),
        detail_slots: Arc::new(tokio::sync::Semaphore::new(3)),
        online_until: HashMap::new(),
        folder_defs: vec![],
        folder_dialogs: vec![],
    };
    match tokio::time::timeout(Duration::from_secs(30), w.client.is_authorized()).await {
        Ok(Ok(true)) => {
            if let Err(e) = w.authorized().await {
                report(&tx, e);
            }
        }
        Ok(Ok(false)) => {
            if let Err(e) = w.refresh_qr().await {
                report(&tx, e);
            }
        }
        Ok(Err(e)) => report(&tx, e),
        Err(_) => report(&tx, "Telegram не отвечает. Проверьте подключение к сети."),
    }
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.tick().await;
    loop {
        let deadline = w.online_until.values().min().copied();
        let delay = deadline
            .map(|at| (at - chrono::Utc::now().timestamp()).max(0) as u64)
            .unwrap_or(86400);
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(delay)), if deadline.is_some() => {
                let now = chrono::Utc::now().timestamp();
                let expired: Vec<_> = w.online_until.iter().filter_map(|(&id, &until)| (until <= now).then_some(id)).collect();
                for id in expired { w.online_until.remove(&id); let _ = tx.send(TgEvent::Presence { peer_id: id, text: "не в сети".into(), online: false }); }
            }
            _ = w.avatar_tasks.join_next(), if !w.avatar_tasks.is_empty() => {}
            cmd = commands.recv() => {
                let Some(cmd) = cmd else { break };
                let sending = matches!(cmd, TgCommand::SendMessage(..) | TgCommand::SendFile(..));
                let timeout = if matches!(cmd, TgCommand::SendFile(..) | TgCommand::Download(..)) { 600 } else { 45 };
                let result = tokio::time::timeout(Duration::from_secs(timeout), w.command(cmd)).await;
                let error = match result { Ok(Ok(())) => None, Ok(Err(e)) => Some(e.to_string()), Err(_) => Some("Telegram не ответил вовремя. Проверьте состояние перед повторной отправкой.".into()) };
                if let Some(message) = error {
                    if sending { let _ = tx.send(TgEvent::SendFailed { message }); } else { report(&tx, message); }
                }
            }
            _ = tick.tick(), if w.qr => {
                if chrono::Utc::now().timestamp() >= w.qr_expires {
                    match tokio::time::timeout(Duration::from_secs(30), w.refresh_qr()).await {
                        Ok(Ok(())) => (), Ok(Err(e)) => { w.clear_qr(); report(&tx, e); }, Err(_) => { w.clear_qr(); report(&tx, "Не удалось обновить QR: Telegram не отвечает."); }
                    }
                }
            }
            update = updates.next() => match update {
                Ok(Update::Raw(raw)) if matches!(&raw.raw, tl::enums::Update::UserStatus(_)) => {
                    if let tl::enums::Update::UserStatus(update) = raw.raw { w.presence_update(update.user_id, update.status); }
                }
                Ok(Update::NewMessage(m)) => {
                    let known = w.peers.iter().find_map(|(&id, p)| (p.id == m.peer_id()).then_some(id));
                    let id = match known { Some(id) => Some(id), None => match m.peer() { Some(peer) => match w.register_peer(peer).await { Ok(id) => Some(id), Err(e) => { report(&tx, e); None } }, None => None } };
                    if let Some(id) = id {
                        if !m.outgoing() && w.folder_defs.iter().any(|f| matches!(f, tl::enums::DialogFilter::Filter(f) if f.exclude_read)) {
                            if let Some((_, d)) = w.folder_dialogs.iter_mut().find(|(key, _)| *key == id)
                                && let tl::enums::Dialog::Dialog(d) = &mut d.raw
                            {
                                d.unread_count = d.unread_count.saturating_add(1);
                                let _ = tx.send(TgEvent::Folders { folders: w.folder_rows() });
                            }
                        }
                        let _ = tx.send(TgEvent::NewMessage { peer_id: id, message: message(&m) });
                    }
                }
                Ok(Update::MessageEdited(m)) => {
                    if let Some((&id, _)) = w.peers.iter().find(|(_, p)| p.id == m.peer_id()) { let _ = tx.send(TgEvent::MessageEdited { peer_id: id, message: message(&m) }); }
                }
                Ok(Update::MessageDeleted(d)) => {
                    for (&id, peer) in &w.peers {
                        let applies = match d.channel_id() { Some(channel) => peer.id.kind() == grammers_session::types::PeerKind::Channel && peer.id.bare_id_unchecked() == channel, None => peer.id.kind() != grammers_session::types::PeerKind::Channel };
                        if applies { let _ = tx.send(TgEvent::MessagesDeleted { peer_id: id, ids: d.messages().to_vec() }); }
                    }
                }
                Ok(Update::Raw(raw)) if w.qr && matches!(raw.raw, tl::enums::Update::LoginToken) => { if let Err(e) = w.refresh_qr().await { report(&tx, e); } }
                Ok(Update::Raw(raw)) if matches!(&raw.raw, tl::enums::Update::ReadHistoryOutbox(_) | tl::enums::Update::ReadChannelOutbox(_)) => {
                    match raw.raw {
                        tl::enums::Update::ReadHistoryOutbox(update) => w.outbox_read(update.peer.into(), update.max_id),
                        tl::enums::Update::ReadChannelOutbox(update) => w.outbox_read(grammers_session::types::PeerId::channel_unchecked(update.channel_id), update.max_id),
                        _ => unreachable!(),
                    }
                }
                Ok(Update::Raw(raw)) if matches!(raw.raw, tl::enums::Update::DialogFilter(_) | tl::enums::Update::DialogFilters | tl::enums::Update::DialogFilterOrder(_)) => {
                    if let Err(e) = w.reload_folders().await { report(&tx, e); } else { let _ = tx.send(TgEvent::Folders { folders: w.folder_rows() }); }
                }
                Err(e) => { report(&tx, e); tokio::time::sleep(Duration::from_secs(2)).await; }
                _ => (),
            }
        }
    }
    Ok(())
}
