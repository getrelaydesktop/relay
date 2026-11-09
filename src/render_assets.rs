use std::collections::HashMap;

use slint::{Color, Image, Rgba8Pixel, SharedPixelBuffer};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

pub struct Assets {
    qr_cache: HashMap<(String, u32, [u8; 4]), Image>,
}

impl Assets {
    pub fn new() -> Self {
        Self {
            qr_cache: HashMap::new(),
        }
    }

    pub fn qr_card(&mut self, matrix_str: &str, size_px: u32, accent: Color) -> Image {
        let size = size_px.max(64);
        let a_arr = [accent.red(), accent.green(), accent.blue(), accent.alpha()];
        let key = (matrix_str.to_owned(), size, a_arr);
        if let Some(cached) = self.qr_cache.get(&key) {
            return cached.clone();
        }

        let mut pixmap = Pixmap::new(size, size).unwrap();

        // White rounded card background (radius 18px like Telegram Desktop QR box)
        let mut white_paint = Paint::default();
        white_paint.anti_alias = true;
        white_paint.set_color_rgba8(255, 255, 255, 255);
        if let Some(card_path) = rounded_rect_path(0.0, 0.0, size as f32, size as f32, 18.0) {
            pixmap.fill_path(
                &card_path,
                &white_paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        // Preserve every QR module and a four-module quiet zone.
        let rows: Vec<&str> = matrix_str.lines().filter(|l| !l.is_empty()).collect();
        let n = rows.len();
        if n > 0 {
            let cell = (size / (n as u32 + 8)).max(1) as f32;
            let pad = (size as f32 - n as f32 * cell) / 2.0;
            let mut paint = Paint::default();
            paint.anti_alias = false;
            paint.set_color_rgba8(17, 22, 28, 255);
            for (r, row) in rows.iter().enumerate() {
                for (c, ch) in row.chars().enumerate() {
                    if ch == '1' {
                        if let Some(rect) = tiny_skia::Rect::from_xywh(
                            pad + c as f32 * cell,
                            pad + r as f32 * cell,
                            cell,
                            cell,
                        ) {
                            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                        }
                    }
                }
            }
        }
        let image = pixmap_to_slint(&pixmap);
        self.qr_cache.insert(key, image.clone());
        image
    }
}

fn rounded_rect_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    rounded_rect_corners(x, y, w, h, r, r, r, r)
}

fn rounded_rect_corners(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tl: f32,
    tr: f32,
    br: f32,
    bl: f32,
) -> Option<tiny_skia::Path> {
    const K: f32 = 0.55228475;
    let mut pb = PathBuilder::new();
    pb.move_to(x + tl, y);
    pb.line_to(x + w - tr, y);
    if tr > 0.0 {
        pb.cubic_to(
            x + w - tr * (1.0 - K),
            y,
            x + w,
            y + tr * (1.0 - K),
            x + w,
            y + tr,
        );
    }
    pb.line_to(x + w, y + h - br);
    if br > 0.0 {
        pb.cubic_to(
            x + w,
            y + h - br * (1.0 - K),
            x + w - br * (1.0 - K),
            y + h,
            x + w - br,
            y + h,
        );
    }
    pb.line_to(x + bl, y + h);
    if bl > 0.0 {
        pb.cubic_to(
            x + bl * (1.0 - K),
            y + h,
            x,
            y + h - bl * (1.0 - K),
            x,
            y + h - bl,
        );
    }
    pb.line_to(x, y + tl);
    if tl > 0.0 {
        pb.cubic_to(x, y + tl * (1.0 - K), x + tl * (1.0 - K), y, x + tl, y);
    }
    pb.close();
    pb.finish()
}

fn pixmap_to_slint(pixmap: &Pixmap) -> Image {
    let w = pixmap.width();
    let h = pixmap.height();
    let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(w, h);
    let dst = buf.make_mut_slice();
    for (d, s) in dst.iter_mut().zip(pixmap.pixels()) {
        let c = s.demultiply();
        *d = Rgba8Pixel {
            r: c.red(),
            g: c.green(),
            b: c.blue(),
            a: c.alpha(),
        };
    }
    Image::from_rgba8(buf)
}
