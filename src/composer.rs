//! Editable entity ranges, independent of the renderer and clipboard backend.
use crate::{Desktop, rich_text};
use base64::{Engine, engine::general_purpose::STANDARD};
use grammers_tl_types::{Deserializable, Serializable};
use grammers_tl_types::{enums::MessageEntity as Entity, types};

pub fn encode(entities: &[Entity]) -> String {
    STANDARD.encode(entities.to_vec().to_bytes())
}
pub fn decode(data: &str) -> Vec<Entity> {
    STANDARD
        .decode(data)
        .ok()
        .and_then(|b| Vec::<Entity>::from_bytes(&b).ok())
        .unwrap_or_default()
}

#[derive(Default, Clone)]
pub struct Draft {
    pub text: String,
    pub entities: Vec<Entity>,
}
impl Draft {
    pub fn edit(&mut self, text: &str) {
        if self.text == text {
            return;
        }
        let old: Vec<_> = self.text.chars().collect();
        let new: Vec<_> = text.chars().collect();
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let start: i32 = old[..prefix].iter().map(|c| c.len_utf16() as i32).sum();
        let end: i32 = old[..old.len() - suffix]
            .iter()
            .map(|c| c.len_utf16() as i32)
            .sum();
        let delta = text.encode_utf16().count() as i32 - self.text.encode_utf16().count() as i32;
        self.entities.retain_mut(|entity| {
            let a = entity.offset();
            let b = a + entity.length();
            let mapped = if b <= start {
                Some((a, b))
            } else if a >= end {
                Some((a + delta, b + delta))
            } else if a <= start && b >= end {
                Some((a, b + delta))
            } else {
                None
            };
            if let Some((a, b)) = mapped.filter(|(a, b)| a < b) {
                set_range(entity, a, b - a);
                true
            } else {
                false
            }
        });
        self.text = text.into();
    }
    pub fn toggle(&mut self, kind: &str, start: usize, end: usize, url: &str) -> bool {
        let (start, end) = (start.min(end), start.max(end));
        let (Some(left), Some(selected)) = (self.text.get(..start), self.text.get(start..end))
        else {
            return false;
        };
        if selected.is_empty() {
            return false;
        }
        let offset = left.encode_utf16().count() as i32;
        let length = selected.encode_utf16().count() as i32;
        let value = match kind {
            "bold" => Entity::Bold(types::MessageEntityBold { offset, length }),
            "italic" => Entity::Italic(types::MessageEntityItalic { offset, length }),
            "underline" => Entity::Underline(types::MessageEntityUnderline { offset, length }),
            "strike" => Entity::Strike(types::MessageEntityStrike { offset, length }),
            "code" => Entity::Code(types::MessageEntityCode { offset, length }),
            "quote" => Entity::Blockquote(types::MessageEntityBlockquote {
                collapsed: false,
                offset,
                length,
            }),
            "link"
                if url.starts_with("https://")
                    || url.starts_with("http://")
                    || url.starts_with("tg://") =>
            {
                Entity::TextUrl(types::MessageEntityTextUrl {
                    offset,
                    length,
                    url: url.into(),
                })
            }
            "clear" => {
                self.entities
                    .retain(|e| e.offset() + e.length() <= offset || e.offset() >= offset + length);
                return true;
            }
            _ => return false,
        };
        if let Some(index) = self.entities.iter().position(|e| {
            std::mem::discriminant(e) == std::mem::discriminant(&value)
                && e.offset() == offset
                && e.length() == length
        }) {
            self.entities.remove(index);
        } else {
            self.entities.push(value);
        }
        true
    }
}
fn set_range(entity: &mut Entity, offset: i32, length: i32) {
    macro_rules! fields { ($($v:ident),*) => { match entity { $(Entity::$v(e) => {e.offset=offset;e.length=length;},)* _=>{} } }; }
    fields!(
        Bold,
        Italic,
        Underline,
        Strike,
        Code,
        TextUrl,
        Pre,
        Url,
        Mention,
        Hashtag,
        BotCommand,
        Email,
        MentionName,
        Phone,
        Cashtag,
        BankCard,
        Spoiler,
        CustomEmoji,
        Blockquote
    );
}

pub fn preview(ui: &Desktop, draft: &Draft) {
    ui.set_draft_has_formatting(!draft.entities.is_empty());
    ui.set_draft_preview(rich_text::format(&draft.text, &draft.entities, 0));
    ui.set_format_count(draft.entities.len() as i32);
}

#[derive(Default)]
pub struct History {
    undo: Vec<Draft>,
    redo: Vec<Draft>,
    saved: Option<Draft>,
}
impl History {
    pub fn record(&mut self, draft: &Draft) {
        self.undo.push(draft.clone());
        self.redo.clear();
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }
    pub fn action(&mut self, kind: &str, draft: &mut Draft) {
        match kind {
            "undo" => {
                if let Some(previous) = self.undo.pop() {
                    self.redo.push(draft.clone());
                    *draft = previous;
                }
            }
            "redo" => {
                if let Some(next) = self.redo.pop() {
                    self.undo.push(draft.clone());
                    *draft = next;
                }
            }
            "begin" => self.saved = Some(draft.clone()),
            "cancel" => {
                if let Some(saved) = self.saved.take() {
                    self.record(draft);
                    *draft = saved;
                }
            }
            _ => {}
        }
    }
    pub fn sync(&self, ui: &Desktop) {
        ui.set_editor_can_undo(!self.undo.is_empty());
        ui.set_editor_can_redo(!self.redo.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_selection_survives_typing_and_round_trip() {
        let mut d = Draft::default();
        d.edit("🙂 Привет мир");
        let a = "🙂 ".len();
        let b = "🙂 Привет".len();
        assert!(d.toggle("bold", a, b, ""));
        assert_eq!((d.entities[0].offset(), d.entities[0].length()), (3, 6));
        d.edit("🙂 Приветик мир");
        assert_eq!(d.entities[0].length(), 6);
        d.edit("x🙂 Приветик мир");
        assert_eq!(d.entities[0].offset(), 4);
        assert_eq!(decode(&encode(&d.entities)), d.entities);
        assert!(d.toggle("clear", 0, d.text.len(), ""));
        assert!(d.entities.is_empty());
    }
    #[test]
    fn undo_restores_text_and_entities() {
        let mut d = Draft {
            text: "hello".into(),
            ..Default::default()
        };
        let mut h = History::default();
        h.record(&d);
        d.toggle("italic", 0, 5, "");
        h.action("undo", &mut d);
        assert!(d.entities.is_empty());
        h.action("redo", &mut d);
        assert_eq!(d.entities.len(), 1);
        h.action("begin", &mut d);
        d.edit("hello world");
        h.action("cancel", &mut d);
        assert_eq!(d.text, "hello");
        assert_eq!(d.entities.len(), 1);
    }
}
