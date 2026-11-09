//! Display only presence explicitly disclosed by Telegram. No inferred history.
use grammers_tl_types as tl;

pub fn describe(status: &tl::enums::UserStatus, now: i64) -> (String, bool) {
    use tl::enums::UserStatus;
    let text = match status {
        UserStatus::Online(value) if i64::from(value.expires) > now => {
            return ("в сети".into(), true);
        }
        UserStatus::Offline(value) if value.was_online > 0 => {
            let Some(date) = chrono::DateTime::from_timestamp(i64::from(value.was_online), 0)
            else {
                return ("не в сети".into(), false);
            };
            let date = date.with_timezone(&chrono::Local);
            let today = chrono::DateTime::from_timestamp(now, 0)
                .unwrap_or_default()
                .with_timezone(&chrono::Local)
                .date_naive();
            if date.date_naive() == today {
                format!("был(а) сегодня в {}", date.format("%H:%M"))
            } else {
                format!(
                    "был(а) {} в {}",
                    date.format("%d.%m.%Y"),
                    date.format("%H:%M")
                )
            }
        }
        UserStatus::Recently(_) => "был(а) недавно".into(),
        UserStatus::LastWeek(_) => "был(а) на этой неделе".into(),
        UserStatus::LastMonth(_) => "был(а) в этом месяце".into(),
        _ => "не в сети".into(),
    };
    (text, false)
}

pub fn for_peer(peer: &grammers_client::peer::Peer) -> (String, bool) {
    use grammers_client::peer::Peer;
    match peer {
        Peer::User(user) if user.is_self() => ("личное облако".into(), false),
        Peer::User(user) if user.is_bot() => ("бот".into(), false),
        Peer::User(user) => describe(user.status(), chrono::Utc::now().timestamp()),
        Peer::Group(_) => ("группа".into(), false),
        Peer::Channel(_) => ("канал".into(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_disclosed_timestamps_are_shown() {
        let now = 1_800_000_000;
        let (text, online) = describe(
            &tl::types::UserStatusOffline {
                was_online: now as i32 - 60,
            }
            .into(),
            now,
        );
        assert!(text.contains(':') && !online);
        let (text, online) = describe(&tl::types::UserStatusRecently { by_me: false }.into(), now);
        assert_eq!(text, "был(а) недавно");
        assert!(!online && !text.contains(':'));
        assert!(
            !describe(
                &tl::types::UserStatusOnline {
                    expires: now as i32 - 1
                }
                .into(),
                now
            )
            .1
        );
        assert!(
            describe(
                &tl::types::UserStatusOnline {
                    expires: now as i32 + 60
                }
                .into(),
                now
            )
            .1
        );
    }
}
