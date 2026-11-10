//! Read-only Telegram profiles and publicly displayed gifts.
use crate::telegram::{EventSink, TgEvent};
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
    pub photo: Option<image::RgbaImage>,
    pub is_user: bool,
    pub is_self: bool,
    pub is_bot: bool,
    pub online: bool,
    pub birthday: String,
    pub notifications_muted: Option<bool>,
}
#[derive(Debug, Clone, Default)]
pub struct Gift {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub url: String,
    pub photo: Option<image::RgbaImage>,
    pub collectible: bool,
    pub center: i32,
    pub edge: i32,
    pub pattern_color: i32,
    pub badge: String,
    pub pinned: bool,
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
                crate::telegram::download_image(&client, &photo, 8 * 1024 * 1024, 4096).await?;
            crate::telegram::resize_image(pixels, 1280).await
        };
        if let Ok(Ok(pixels)) =
            tokio::time::timeout(std::time::Duration::from_secs(20), fetch).await
        {
            profile.photo = Some(pixels);
            let _ = tx.send(TgEvent::ProfileReady {
                key: key.clone(),
                profile,
            });
        }
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
        return format!("1 из {total}");
    }
    if total >= 1_000_000 {
        format!("1 из {}M", total / 1_000_000)
    } else {
        format!("1 из {}K", total / 1000)
    }
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
    let mut previews = Vec::new();
    for tl::enums::SavedStarGift::Gift(saved) in response.gifts {
        if saved.unsaved || saved.refunded {
            continue;
        }
        let (mut gift, doc, pattern) = match saved.gift {
            tl::enums::StarGift::Gift(g) => (
                Gift {
                    key: format!(
                        "gift:{}:{}:{}",
                        g.id,
                        saved.date,
                        saved.saved_id.unwrap_or(saved.msg_id.unwrap_or(0) as i64)
                    ),
                    title: g.title.unwrap_or_else(|| "Подарок Telegram".into()),
                    detail: format!("{} Stars", g.stars),
                    badge: g
                        .availability_total
                        .filter(|n| *n > 0)
                        .map(limited_badge)
                        .unwrap_or_default(),
                    ..Default::default()
                },
                Some(g.sticker),
                None,
            ),
            tl::enums::StarGift::Unique(g) => {
                let mut gift = Gift {
                    key: format!("unique:{}", g.id),
                    title: g.title,
                    detail: format!("#{} · коллекционный", g.num),
                    url: format!("https://t.me/nft/{}", g.slug),
                    collectible: true,
                    badge: if saved.pinned_to_top {
                        format!("#{}", g.num)
                    } else {
                        limited_badge(g.availability_total)
                    },
                    ..Default::default()
                };
                let (mut model, mut pattern) = (None, None);
                for attribute in g.attributes {
                    match attribute {
                        tl::enums::StarGiftAttribute::Model(m) => model = Some(m.document),
                        tl::enums::StarGiftAttribute::Pattern(p) => pattern = Some(p.document),
                        tl::enums::StarGiftAttribute::Backdrop(b) => {
                            gift.center = b.center_color;
                            gift.edge = b.edge_color;
                            gift.pattern_color = b.pattern_color;
                        }
                        _ => {}
                    }
                }
                (gift, model, pattern)
            }
        };
        gift.pinned = saved.pinned_to_top;
        if let Some(doc) = doc {
            previews.push((gift.key.clone(), doc, false));
        }
        if let Some(pattern) = pattern {
            previews.push((gift.key.clone(), pattern, true));
        }
        gifts.push(gift);
    }
    // Show all card metadata now; slow thumbnails must not hide the whole page.
    let _ = tx.send(TgEvent::GiftsReady {
        key: key.into(),
        gifts,
        next: response.next_offset.unwrap_or_default(),
        append: !offset.is_empty(),
    });
    let mut tasks = tokio::task::JoinSet::new();
    for (gift_key, doc, pattern) in previews {
        let client = client.clone();
        let tx = tx.clone();
        let key = key.to_owned();
        tasks.spawn(async move {
            let fetch = async {
                let _permit = MEDIA_SLOTS.acquire().await?;
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
                let pixels =
                    crate::telegram::download_image(&client, thumb, 1024 * 1024, 1024).await?;
                crate::telegram::resize_image(pixels, 256).await
            };
            if let Ok(Ok(pixels)) =
                tokio::time::timeout(std::time::Duration::from_secs(15), fetch).await
            {
                let _ = tx.send(TgEvent::GiftPreview {
                    key,
                    gift_key,
                    pixels,
                    pattern,
                });
            }
        });
    }
    while tasks.join_next().await.is_some() {}
    Ok(())
}
