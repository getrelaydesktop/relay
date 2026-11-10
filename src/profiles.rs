//! Read-only Telegram profiles and publicly displayed gifts.
use crate::telegram::{EventSink, GiftMediaKind, GiftMediaSource, TgEvent};
use grammers_client::{Client, peer::Peer};
use grammers_session::types::PeerRef;
use grammers_tl_types as tl;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
static MEDIA_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Debug, Clone, Default)]
pub struct Profile {
    pub name: String,
    pub initials: String,
    pub presence: String,
    pub username: String,
    pub phone: String,
    pub about: String,
    pub photo: Option<crate::media::Pixels>,
    pub is_user: bool,
    pub is_self: bool,
    pub is_bot: bool,
    pub online: bool,
    pub birthday: String,
    pub notifications_muted: Option<bool>,
}
#[derive(Debug, Clone, Default)]
pub struct GiftSources {
    pub model: Option<tl::enums::Document>,
    pub pattern: Option<tl::enums::Document>,
    pub sender: Option<PeerRef>,
    pub owner: Option<PeerRef>,
}
#[derive(Debug, Clone, Default)]
pub struct Gift {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub url: String,
    pub sources: GiftSources,
    pub collectible: bool,
    pub center: i32,
    pub edge: i32,
    pub pattern_color: i32,
    pub text_color: i32,
    pub badge: String,
    pub pinned: bool,
    pub mine: bool,
    pub resale: bool,
    pub number: i32,
    pub sender: String,
    pub sender_hidden: bool,
    pub show_sender: bool,
    pub owner: String,
    pub owner_address: String,
    pub date: String,
    pub value: String,
    pub availability: String,
    pub model: String,
    pub model_rarity: String,
    pub symbol: String,
    pub symbol_rarity: String,
    pub backdrop: String,
    pub backdrop_rarity: String,
    pub message: String,
    pub message_entities: Vec<tl::enums::MessageEntity>,
    pub original_info: String,
    pub original_entities: Vec<tl::enums::MessageEntity>,
    pub original_prefix: i32,
    pub to_self: bool,
    pub can_upgrade: bool,
}

pub async fn load(
    client: Client,
    peer: PeerRef,
    key: String,
    gifts: bool,
    tx: EventSink,
) -> Result<()> {
    let resolved = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.resolve_peer(peer),
    )
    .await??;
    let name = resolved.name().unwrap_or("Telegram").to_owned();
    let mut profile = Profile {
        initials: name
            .split_whitespace()
            .take(2)
            .filter_map(|s| s.chars().next())
            .collect(),
        name,
        presence: crate::presence::for_peer(&resolved).0,
        online: crate::presence::for_peer(&resolved).1,
        is_user: matches!(resolved, Peer::User(_)),
        ..Default::default()
    };
    if let Peer::User(user) = &resolved {
        profile.is_self = user.is_self();
        profile.is_bot = user.is_bot();
        profile.username = user.username().map(|s| format!("@{s}")).unwrap_or_default();
        profile.phone = user.phone().map(|s| format!("+{s}")).unwrap_or_default();
    }
    // Basic text is immediately usable, before full-info/media round trips.
    let _ = tx.send(TgEvent::ProfileReady {
        key: key.clone(),
        profile: profile.clone(),
    });
    let info_and_photo = async {
        if matches!(resolved, Peer::User(_)) {
            if let Ok(Ok(tl::enums::users::UserFull::Full(full))) = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                client.invoke(&tl::functions::users::GetFullUser { id: peer.into() }),
            )
            .await
            {
                let tl::enums::UserFull::Full(full) = full.full_user;
                profile.about = full.about.unwrap_or_default();
                profile.birthday = full
                    .birthday
                    .as_ref()
                    .map(birthday_label)
                    .unwrap_or_default();
                let tl::enums::PeerNotifySettings::Settings(settings) = full.notify_settings;
                profile.notifications_muted =
                    Some(settings.mute_until.unwrap_or(0) as i64 > chrono::Utc::now().timestamp());
                let _ = tx.send(TgEvent::ProfileReady {
                    key: key.clone(),
                    profile: profile.clone(),
                });
                // A later photo response must not overwrite a newly toggled mute.
                profile.notifications_muted = None;
            }
        }
        let fetch = async {
            let _permit = MEDIA_SLOTS.acquire().await?;
            let photo = resolved.photo(true).await?.ok_or("Фото недоступно")?;
            let pixels =
                crate::telegram::download_image_fit(&client, &photo, 8 * 1024 * 1024, 4096, 256)
                    .await?;
            profile.photo = Some(pixels.into());
            let _ = tx
                .send_media(TgEvent::ProfileReady {
                    key: key.clone(),
                    profile,
                })
                .await;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        };
        let _ = tokio::time::timeout(std::time::Duration::from_secs(20), fetch).await;
    };
    let gift_list = async {
        if gifts && matches!(resolved, Peer::User(_)) {
            load_gifts(&client, peer, &key, "", &tx).await
        } else {
            let _ = tx.send(TgEvent::GiftsReady {
                key: key.clone(),
                gifts: vec![],
                next: String::new(),
                append: false,
            });
            Ok(())
        }
    };
    let (_, result) = tokio::join!(info_and_photo, gift_list);
    if let Err(error) = result {
        let _ = tx.send(TgEvent::GiftsFailed {
            key,
            message: error.to_string(),
        });
    }
    Ok(())
}

fn birthday_label(birthday: &tl::enums::Birthday) -> String {
    let tl::enums::Birthday::Birthday(date) = birthday;
    let months = [
        "января",
        "февраля",
        "марта",
        "апреля",
        "мая",
        "июня",
        "июля",
        "августа",
        "сентября",
        "октября",
        "ноября",
        "декабря",
    ];
    let Some(month) = date
        .month
        .checked_sub(1)
        .and_then(|i| months.get(i as usize))
    else {
        return String::new();
    };
    if !(1..=31).contains(&date.day) {
        return String::new();
    }
    match date.year {
        Some(year) => format!("{} {month} {year}", date.day),
        None => format!("{} {month}", date.day),
    }
}

fn limited_badge(total: i32) -> String {
    if total <= 0 {
        return String::new();
    }
    if total < 10_000 && total % 1000 != 0 {
        return format!("1 из {}", decimal_count(total as i64));
    }
    let (unit, suffix) = if total >= 1_000_000 {
        (1_000_000, "M")
    } else {
        (1000, "K")
    };
    let short = format!("{:.1}", total as f64 / unit as f64);
    format!("1 из {}{suffix}", short.trim_end_matches(".0"))
}

// Profile covers need at most a 256 px thumbnail. The viewer has its own
// cancelable, tracked job so its larger raster never becomes a profile avatar.
pub async fn load_viewer_photo(
    client: &Client,
    peer: PeerRef,
    key: &str,
    tx: &EventSink,
) -> Result<()> {
    let fetch = async {
        let _slot = MEDIA_SLOTS.acquire().await?;
        let resolved = client.resolve_peer(peer).await?;
        let photo = resolved.photo(true).await?.ok_or("Фото недоступно")?;
        let pixels = crate::telegram::download_image_fit(
            client,
            &photo,
            32 * 1024 * 1024,
            8192,
            2560,
        )
        .await?;
        tx.send_media(TgEvent::ProfilePhotoReady {
            key: key.into(),
            pixels: pixels.into(),
        })
        .await
        .map_err(|_| "Просмотр фото закрыт")?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(45), fetch).await??;
    Ok(())
}

fn original_info(
    sender: Option<&str>,
    recipient: &str,
    date: i32,
    message: Option<tl::enums::TextWithEntities>,
) -> (String, Vec<tl::enums::MessageEntity>, i32) {
    let recipient = if recipient.is_empty() { "Получатель недоступен" } else { recipient };
    let mut text = match sender {
        Some(sender) => format!(
            "Подарок от {} для {recipient}",
            if sender.is_empty() { "Неизвестный отправитель" } else { sender }
        ),
        None => format!("Подарок для {recipient}"),
    };
    if let Some(date) = chrono::DateTime::from_timestamp(date as i64, 0) {
        text.push_str(&format!(" · {}", date.with_timezone(&chrono::Local).format("%d.%m.%Y")));
    }
    let mut entities = Vec::new();
    let mut prefix = 0;
    if let Some(tl::enums::TextWithEntities::Entities(message)) = message {
        if !message.text.is_empty() {
            text.push_str("\n\n");
            prefix = i32::try_from(text.encode_utf16().count()).unwrap_or(i32::MAX);
            text.push_str(&message.text);
            entities = message.entities;
        }
    }
    (text, entities, prefix)
}

pub async fn load_gifts(
    client: &Client,
    peer: PeerRef,
    key: &str,
    offset: &str,
    tx: &EventSink,
) -> Result<()> {
    let tl::enums::payments::SavedStarGifts::Gifts(response) = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        client.invoke(&tl::functions::payments::GetSavedStarGifts {
            exclude_unsaved: true,
            exclude_saved: false,
            exclude_unlimited: false,
            exclude_unique: false,
            sort_by_value: false,
            exclude_upgradable: false,
            exclude_unupgradable: false,
            peer_color_available: false,
            exclude_hosted: false,
            peer: peer.into(),
            collection_id: None,
            offset: offset.into(),
            limit: 12,
        }),
    )
    .await??;
    let mut gifts = Vec::new();
    for tl::enums::SavedStarGift::Gift(saved) in response.gifts {
        if saved.unsaved || saved.refunded {
            continue;
        }
        let sender_peer = if saved.name_hidden {
            None
        } else {
            saved.from_id.clone()
        };
        let (mut gift, owner_peer) = match saved.gift {
            tl::enums::StarGift::Gift(g) => {
                let total = g.availability_total.unwrap_or_default();
                let remaining = g.availability_remains.unwrap_or_default();
                (
                    Gift {
                        key: format!(
                            "gift:{}:{}:{}",
                            g.id,
                            saved.date,
                            saved.saved_id.unwrap_or(saved.msg_id.unwrap_or(0) as i64)
                        ),
                        title: g.title.unwrap_or_else(|| "Подарок".into()),
                        detail: format!("{} Stars", decimal_count(g.stars)),
                        value: crate::currency::value(g.stars, "XTR", false),
                        availability: if total > 0 {
                            format!(
                                "{} из {}",
                                decimal_count(remaining as i64),
                                decimal_count(total as i64)
                            )
                        } else {
                            String::new()
                        },
                        badge: limited_badge(total),
                        sources: GiftSources {
                            model: Some(g.sticker),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    None,
                )
            }
            tl::enums::StarGift::Unique(g) => {
                let resale = g.resell_amount.as_ref().is_some_and(|a| !a.is_empty());
                let mut gift = Gift {
                    key: format!("unique:{}", g.id),
                    title: g.title,
                    detail: format!("#{} · коллекционный", decimal_count(g.num as i64)),
                    url: format!("https://t.me/nft/{}", g.slug),
                    collectible: true,
                    number: g.num,
                    resale,
                    owner: g.owner_name.unwrap_or_default(),
                    owner_address: g.owner_address.unwrap_or_default(),
                    value: match (g.value_amount, g.value_currency.as_deref()) {
                        (Some(amount), Some(currency)) if !currency.is_empty() =>
                            crate::currency::value(amount, currency, true),
                        _ => String::new(),
                    },
                    availability: format!(
                        "{} из {}",
                        decimal_count(g.availability_issued as i64),
                        decimal_count(g.availability_total as i64)
                    ),
                    badge: if resale {
                        "В продаже".into()
                    } else if saved.pinned_to_top || key == "self" {
                        format!("#{}", decimal_count(g.num as i64))
                    } else {
                        limited_badge(g.availability_total)
                    },
                    ..Default::default()
                };
                for attribute in g.attributes {
                    match attribute {
                        tl::enums::StarGiftAttribute::Model(m) => {
                            gift.model = m.name;
                            gift.model_rarity = rarity_label(&m.rarity);
                            gift.sources.model = Some(m.document);
                        }
                        tl::enums::StarGiftAttribute::Pattern(p) => {
                            gift.symbol = p.name;
                            gift.symbol_rarity = rarity_label(&p.rarity);
                            gift.sources.pattern = Some(p.document);
                        }
                        tl::enums::StarGiftAttribute::Backdrop(b) => {
                            gift.backdrop = b.name;
                            gift.backdrop_rarity = rarity_label(&b.rarity);
                            gift.center = b.center_color;
                            gift.edge = b.edge_color;
                            gift.pattern_color = b.pattern_color;
                            gift.text_color = b.text_color;
                        }
                        tl::enums::StarGiftAttribute::OriginalDetails(info) => {
                            let (recipient, _) = peer_details(
                                client, &info.recipient_id, &response.users, &response.chats,
                            ).await;
                            let sender = if let Some(sender) = &info.sender_id {
                                Some(peer_details(client, sender, &response.users, &response.chats).await.0)
                            } else {
                                None
                            };
                            (gift.original_info, gift.original_entities, gift.original_prefix) =
                                original_info(sender.as_deref(), &recipient, info.date, info.message);
                        }
                    }
                }
                (gift, g.owner_id)
            }
        };
        gift.pinned = saved.pinned_to_top;
        gift.mine = key == "self";
        gift.can_upgrade = saved.can_upgrade;
        if gift.number == 0 {
            gift.number = saved.gift_num.unwrap_or_default();
        }
        gift.sender_hidden = saved.name_hidden;
        gift.show_sender = !gift.collectible && (sender_peer.is_some() || saved.name_hidden);
        gift.date = chrono::DateTime::from_timestamp(saved.date as i64, 0)
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%d.%m.%Y в %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        if let Some(tl::enums::TextWithEntities::Entities(message)) = saved.message {
            gift.message = message.text;
            gift.message_entities = message.entities;
        }
        if let Some(sender) = sender_peer {
            let (name, reference) =
                peer_details(client, &sender, &response.users, &response.chats).await;
            gift.to_self = !gift.collectible && reference.is_some_and(|sender| sender.id == peer.id);
            gift.sender = name;
            gift.sources.sender = reference;
        }
        if let Some(owner) = owner_peer {
            let (name, reference) =
                peer_details(client, &owner, &response.users, &response.chats).await;
            if !name.is_empty() {
                gift.owner = name;
            }
            gift.sources.owner = reference;
        }
        gifts.push(gift);
    }
    // Metadata only: no image jobs for pages the user has never scrolled to.
    let _ = tx.send(TgEvent::GiftsReady {
        key: key.into(),
        gifts,
        next: response.next_offset.unwrap_or_default(),
        append: !offset.is_empty(),
    });
    Ok(())
}

async fn peer_details(
    client: &Client,
    id: &tl::enums::Peer,
    users: &[tl::enums::User],
    chats: &[tl::enums::Chat],
) -> (String, Option<PeerRef>) {
    let resolved = match id {
        tl::enums::Peer::User(id) => users
            .iter()
            .find(|user| match user {
                tl::enums::User::User(u) => u.id == id.user_id,
                tl::enums::User::Empty(u) => u.id == id.user_id,
            })
            .map(|u| Peer::User(grammers_client::peer::User::from_raw(client, u.clone()))),
        tl::enums::Peer::Chat(id) => chats
            .iter()
            .map(|c| Peer::from_raw(client, c.clone()))
            .find(|p| p.id().bare_id() == Some(id.chat_id) && matches!(p, Peer::Group(_))),
        tl::enums::Peer::Channel(id) => chats
            .iter()
            .map(|c| Peer::from_raw(client, c.clone()))
            .find(|p| p.id().bare_id() == Some(id.channel_id) && !matches!(p, Peer::User(_))),
    };
    if let Some(peer) = resolved {
        (
            peer.name().unwrap_or("").to_owned(),
            peer.to_ref().await.ok().flatten(),
        )
    } else {
        (String::new(), None)
    }
}

pub(crate) fn decimal_count(value: i64) -> String {
    let digits = value.to_string();
    let mut result = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 && ch != '-' {
            result.push(' ');
        }
        result.push(ch);
    }
    result
}

fn rarity_label(rarity: &tl::enums::StarGiftAttributeRarity) -> String {
    match rarity {
        tl::enums::StarGiftAttributeRarity::Rarity(value) => {
            if !(0..=1000).contains(&value.permille) {
                return String::new();
            }
            if value.permille % 10 == 0 {
                format!("{}%", value.permille / 10)
            } else {
                format!("{}.{:01}%", value.permille / 10, value.permille % 10)
            }
        }
        tl::enums::StarGiftAttributeRarity::Uncommon => "Uncommon".into(),
        tl::enums::StarGiftAttributeRarity::Rare => "Rare".into(),
        tl::enums::StarGiftAttributeRarity::Epic => "Epic".into(),
        tl::enums::StarGiftAttributeRarity::Legendary => "Legendary".into(),
    }
}

pub async fn load_gift_media(
    client: Client,
    key: String,
    gift_key: String,
    source: GiftMediaSource,
    kind: GiftMediaKind,
    size: u32,
    tx: EventSink,
) {
    let fetch = async {
        let _permit = MEDIA_SLOTS.acquire().await?;
        let pixels = match source {
            GiftMediaSource::Document(doc) => {
                let document = grammers_client::media::Document::from_raw_media(
                    tl::types::MessageMediaDocument {
                        nopremium: false,
                        spoiler: false,
                        video: false,
                        round: false,
                        voice: false,
                        document: Some(doc),
                        alt_documents: None,
                        video_cover: None,
                        video_timestamp: None,
                        ttl_seconds: None,
                    },
                );
                let thumbs = document.thumbs();
                let thumb = thumbs.last().ok_or("Нет статичного превью")?;
                crate::telegram::download_image_fit(&client, thumb, 1024 * 1024, 1024, size).await?
            }
            GiftMediaSource::Peer(peer) => {
                let peer = client.resolve_peer(peer).await?;
                let photo = peer.photo(false).await?.ok_or("Нет фото отправителя")?;
                crate::telegram::download_image_fit(&client, &photo, 1024 * 1024, 1024, size)
                    .await?
            }
        };
        let _ = tx
            .send_media(TgEvent::GiftPreview {
                key: key.clone(),
                gift_key: gift_key.clone(),
                pixels: pixels.into(),
                kind,
            })
            .await;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    if !matches!(
        tokio::time::timeout(std::time::Duration::from_secs(15), fetch).await,
        Ok(Ok(()))
    ) {
        let _ = tx.send(TgEvent::GiftPreviewFailed {
            key,
            gift_key,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_gift_details_preserve_real_participants_and_utf16_message_styles() {
        let body = "😀 **Привет**";
        let message = tl::types::TextWithEntities {
            text: body.into(),
            entities: vec![tl::types::MessageEntityBold { offset: 5, length: 6 }.into()],
        }.into();
        let (text, entities, prefix) = original_info(
            Some("😀 Алексей"), "Мария", 1_700_000_000, Some(message),
        );
        assert!(text.starts_with("Подарок от 😀 Алексей для Мария"));
        assert!(text.ends_with(body));
        let formatted = crate::rich_text::format(&text, &entities, prefix);
        let paragraph = &formatted.as_ref()[0];
        assert_eq!(paragraph.text, text);
        assert_eq!(&text[paragraph.formatting[0].range.clone()], "Привет");
        let (hidden, entities, _) = original_info(None, "Мария", 1_700_000_000, None);
        assert!(hidden.starts_with("Подарок для Мария"));
        assert!(!hidden.contains("Алексей"));
        assert!(entities.is_empty());
    }

    #[test]
    fn gift_counts_and_rarities_preserve_fractional_metadata() {
        assert_eq!(limited_badge(5000), "1 из 5K");
        assert_eq!(limited_badge(12500), "1 из 12.5K");
        assert_eq!(limited_badge(2_500_000), "1 из 2.5M");
        assert_eq!(decimal_count(123456), "123 456");
        assert_eq!(
            rarity_label(&tl::types::StarGiftAttributeRarity { permille: 17 }.into()),
            "1.7%"
        );
    }
}
