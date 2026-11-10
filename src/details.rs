use crate::MessageRow;
use slint::{Model, VecModel};

const MAX_PREVIEW_RGBA_BYTES: u64 = 64 * 1024 * 1024;
const OVERSIZED_PREVIEW: &str = "Фото требует больше 64 МБ: превью отключено, файл можно отправить";

fn check_preview_rgba_bytes(width: u32, height: u32) -> Result<(), String> {
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(4));
    match bytes {
        Some(bytes) if bytes <= MAX_PREVIEW_RGBA_BYTES => Ok(()),
        _ => Err(OVERSIZED_PREVIEW.into()),
    }
}

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
    limits.max_alloc = Some(MAX_PREVIEW_RGBA_BYTES);
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "Не удалось прочитать фото")?;
    // Decoder allocation limits depend on source color depth. A highly
    // compressed grayscale image may fit those limits but not an RGBA buffer.
    check_preview_rgba_bytes(image.width(), image.height())?;
    let image = if image.width() > 960 || image.height() > 960 {
        image.resize(960, 960, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    Ok(Some(image.into_rgba8()))
}

pub fn retain(row: &mut MessageRow, old: &MessageRow) {
    row.send_token = old.send_token;
    row.receive_token = old.receive_token;
    if row.is_photo && old.is_photo && row.photo_key == old.photo_key {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replies_follow_edits_but_keep_explicit_quotes_and_external_authors() {
        let rows = VecModel::from(vec![
            MessageRow {
                id: 1,
                sender: "Анна".into(),
                body: "Изменённый оригинал".into(),
                ..Default::default()
            },
            MessageRow {
                id: 2,
                reply_to: 1,
                ..Default::default()
            },
            MessageRow {
                id: 3,
                reply_to: 1,
                reply_quoted: true,
                reply_body: "Выбранный фрагмент".into(),
                ..Default::default()
            },
            MessageRow {
                id: 4,
                reply_to: 1,
                reply_external: true,
                reply_author: "Другой чат".into(),
                ..Default::default()
            },
        ]);
        resolve_local(&rows, "Группа");
        assert_eq!(rows.row_data(1).unwrap().reply_body, "Изменённый оригинал");
        assert_eq!(rows.row_data(1).unwrap().reply_author, "Анна");
        assert_eq!(rows.row_data(2).unwrap().reply_body, "Выбранный фрагмент");
        assert_eq!(rows.row_data(3).unwrap().reply_author, "Другой чат");
    }
    #[test]
    fn attachment_preview_decodes_bounds_and_rejects_corrupt_images() {
        let dir = std::env::temp_dir().join(format!(
            "relay-preview-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let photo = dir.join("image.png");
        image::RgbaImage::from_pixel(1200, 900, image::Rgba([20, 160, 80, 255]))
            .save(&photo)
            .unwrap();
        let preview = file_preview(photo.to_str().unwrap()).unwrap().unwrap();
        assert_eq!((preview.width(), preview.height()), (960, 720));
        let tiny = dir.join("tiny.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([20, 160, 80, 255]))
            .save(&tiny)
            .unwrap();
        let preview = file_preview(tiny.to_str().unwrap()).unwrap().unwrap();
        assert_eq!((preview.width(), preview.height()), (3, 2));
        std::fs::remove_file(tiny).unwrap();
        let corrupt = dir.join("corrupt.png");
        std::fs::write(&corrupt, b"not an image").unwrap();
        assert!(file_preview(corrupt.to_str().unwrap()).is_err());
        assert!(file_preview("").unwrap().is_none());
        std::fs::remove_file(photo).unwrap();
        std::fs::remove_file(corrupt).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn rgba_limit_is_checked_without_overflow_and_allows_the_boundary() {
        assert!(check_preview_rgba_bytes(4096, 4096).is_ok());
        assert!(check_preview_rgba_bytes(4097, 4096).is_err());
        assert!(check_preview_rgba_bytes(u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn highly_compressed_grayscale_cannot_bypass_rgba_memory_limit() {
        let path = std::env::temp_dir().join(format!(
            "relay-preview-grayscale-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Raw grayscale fits the decoder limit and both dimensions are valid,
        // but expansion to RGBA would exceed 64 MiB by one column.
        image::GrayImage::new(4097, 4096).save(&path).unwrap();
        assert!(path.metadata().unwrap().len() < 16 * 1024 * 1024);
        let result = file_preview(path.to_str().unwrap());
        std::fs::remove_file(path).unwrap();
        assert_eq!(result.unwrap_err(), OVERSIZED_PREVIEW);
    }
}
