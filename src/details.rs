use crate::MessageRow;
use slint::{Model, VecModel};

pub fn file_preview(path: &str) -> Result<Option<image::RgbaImage>, String> {
    let path = std::path::Path::new(path);
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    let metadata = path.metadata().map_err(|_| "Файл не найден".to_string())?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["jpg", "jpeg", "png", "webp", "gif"].contains(&extension.as_str()) {
        return Ok(None);
    }
    if metadata.len() > 16 * 1024 * 1024 {
        return Err("Фото больше 16 МБ: превью отключено, файл можно отправить".into());
    }
    let mut reader = image::ImageReader::open(path).map_err(|_| "Не удалось открыть фото")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "Не удалось прочитать фото")?;
    Ok(Some(
        image
            .resize(960, 960, image::imageops::FilterType::Lanczos3)
            .into_rgba8(),
    ))
}

pub fn retain(row: &mut MessageRow, old: &MessageRow) {
    row.send_token = old.send_token;
    row.receive_token = old.receive_token;
    if row.is_photo && old.is_photo {
        row.preview = old.preview.clone();
        row.has_preview = old.has_preview;
        row.preview_failed = old.preview_failed;
    }
    if row.reply_to > 0 && row.reply_to == old.reply_to && row.reply_external == old.reply_external
    {
        row.reply_author = old.reply_author.clone();
        if row.reply_body.is_empty() {
            row.reply_body = old.reply_body.clone();
        }
        row.reply_resolved = old.reply_resolved;
    }
}

pub fn resolve_local(rows: &VecModel<MessageRow>, title: &str) {
    let originals: std::collections::HashMap<_, _> = rows.iter().map(|row| (row.id, row)).collect();
    for index in 0..rows.row_count() {
        let mut row = rows.row_data(index).unwrap();
        if row.reply_to <= 0 || row.reply_external {
            continue;
        }
        if let Some(original) = originals.get(&row.reply_to) {
            let before = row.clone();
            row.reply_author = if original.outgoing {
                "Вы".into()
            } else if original.sender.is_empty() {
                title.into()
            } else {
                original.sender.clone()
            };
            if !row.reply_quoted {
                row.reply_body = original.body.clone();
            }
            row.reply_resolved = true;
            if row != before {
                rows.set_row_data(index, row);
            }
        }
    }
}
