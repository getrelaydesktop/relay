//! Telegram entities are UTF-16 ranges; Slint's native spans use UTF-8 bytes.
//! Construct spans directly so literal Markdown/HTML and whitespace stay literal.
use grammers_tl_types::enums::MessageEntity;
use i_slint_common::styled_text::{FormattedSpan, Style, StyledTextParagraph};
use slint::private_unstable_api::re_exports::{StyledText, string_to_styled_text};

fn byte_offset(text: &str, offset: i32) -> Option<usize> {
    let offset = usize::try_from(offset).ok()?;
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == offset {
            return Some(byte);
        }
        units += ch.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}

pub fn format(text: &str, entities: &[MessageEntity], prefix: i32) -> StyledText {
    let mut paragraph = StyledTextParagraph {
        text: text.into(),
        formatting: vec![],
        links: vec![],
    };
    for entity in entities {
        if entity.offset() < 0 || entity.length() <= 0 || prefix < 0 {
            continue;
        }
        let Some(start_units) = entity.offset().checked_add(prefix) else {
            continue;
        };
        let Some(end_units) = start_units.checked_add(entity.length()) else {
            continue;
        };
        let (Some(start), Some(end)) =
            (byte_offset(text, start_units), byte_offset(text, end_units))
        else {
            continue;
        };
        if start >= end {
            continue;
        }
        let range = start..end;
        let style = match entity {
            MessageEntity::Bold(_) => Some(Style::Strong),
            MessageEntity::Italic(_) => Some(Style::Emphasis),
            MessageEntity::Underline(_) => Some(Style::Underline),
            MessageEntity::Strike(_) => Some(Style::Strikethrough),
            MessageEntity::Code(_) | MessageEntity::Pre(_) => Some(Style::Code),
            _ => None,
        };
        if let Some(style) = style {
            paragraph.formatting.push(FormattedSpan {
                range: range.clone(),
                style,
            });
        }
        let value = &text[range.clone()];
        let url = match entity {
            MessageEntity::TextUrl(e) => Some(e.url.clone()),
            MessageEntity::Url(_) => Some(if value.contains("://") {
                value.into()
            } else {
                format!("https://{value}")
            }),
            MessageEntity::Email(_) => Some(format!("mailto:{value}")),
            MessageEntity::Mention(_) => {
                Some(format!("https://t.me/{}", value.trim_start_matches('@')))
            }
            MessageEntity::MentionName(e) => Some(format!("tg://user?id={}", e.user_id)),
            _ => None,
        };
        if let Some(url) = url {
            paragraph.formatting.push(FormattedSpan {
                range: range.clone(),
                style: Style::Link,
            });
            paragraph.links.push((range, url));
        }
    }
    // Interpolate a native paragraph, never parse message contents as Markdown.
    StyledText::parse_interpolated("\u{e541}", &[vec![paragraph]])
        .unwrap_or_else(|_| string_to_styled_text(text.into()))
}

pub fn open_link(url: &str) {
    let scheme = url
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(scheme.as_str(), "https" | "http" | "tg" | "mailto") || url.contains('\0') {
        return;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
        let target: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        // ShellExecute receives a URL argument directly, without a command shell.
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grammers_tl_types::types;

    #[test]
    fn unicode_nested_styles_and_literal_markup_survive() {
        let text = "😀 **Привет**\n  мир <tag>";
        let styled = format(
            text,
            &[
                MessageEntity::Bold(types::MessageEntityBold {
                    offset: 5,
                    length: 6,
                }),
                MessageEntity::Italic(types::MessageEntityItalic {
                    offset: 5,
                    length: 3,
                }),
            ],
            0,
        );
        let p = &styled.as_ref()[0];
        assert_eq!(p.text, text);
        assert_eq!(&text[p.formatting[0].range.clone()], "Привет");
        assert_eq!(&text[p.formatting[1].range.clone()], "При");
    }

    #[test]
    fn malformed_utf16_and_overflow_are_ignored() {
        let text = "😀ok";
        let entities = [(1, 1), (-1, 2), (3, i32::MAX), (0, -1)].map(|(offset, length)| {
            MessageEntity::Bold(types::MessageEntityBold { offset, length })
        });
        let styled = format(text, &entities, 0);
        assert_eq!(styled.as_ref()[0].text, text);
        assert!(styled.as_ref()[0].formatting.is_empty());
    }

    #[test]
    fn media_prefix_and_hidden_link_keep_their_exact_ranges() {
        let text = "[File: a.txt]\nОткрыть";
        let styled = format(
            text,
            &[MessageEntity::TextUrl(types::MessageEntityTextUrl {
                offset: 0,
                length: 7,
                url: "https://telegram.org".into(),
            })],
            14,
        );
        let p = &styled.as_ref()[0];
        assert_eq!(&text[p.links[0].0.clone()], "Открыть");
        assert_eq!(p.links[0].1, "https://telegram.org");
    }
}
