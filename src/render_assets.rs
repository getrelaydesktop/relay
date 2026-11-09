use std::{collections::HashMap, fs, path::Path};

use image::imageops::FilterType;
use slint::{Color, Image, Rgba8Pixel, SharedPixelBuffer};
use tiny_skia::{
    FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, SpreadMode,
    Transform,
};

pub struct Assets {
    raw_icons: HashMap<&'static str, image::RgbaImage>,
    icon_cache: HashMap<(&'static str, u32, u32, [u8; 4]), Image>,
    circle_cache: HashMap<(u32, [u8; 4], [u8; 4], bool), Image>,
    pill_cache: HashMap<(u32, u32, [u8; 4], [u8; 4]), Image>,
    bubble_cache: HashMap<(u32, u32, u32, [u8; 4], u8), Image>,
    qr_cache: HashMap<(String, u32, [u8; 4]), Image>,
}

impl Assets {
    pub fn new() -> Self {
        let mut raw_icons = HashMap::new();
        for name in [
            "advanced",
            "back",
            "call",
            "channel",
            "chat-settings",
            "chats",
            "clip",
            "close",
            "edit-folders",
            "folder",
            "groups_create",
            "language",
            "maximize",
            "menu",
            "mic",
            "minimize",
            "moon",
            "more",
            "notifications",
            "panel",
            "personal",
            "phone",
            "power",
            "privacy",
            "profile",
            "read",
            "saved",
            "search",
            "send",
            "settings",
            "smile",
            "tail-left",
            "tail-right",
            "window-close",
        ] {
            let path = Path::new("ui/icons").join(format!("{name}.png"));
            if let Ok(bytes) = fs::read(&path)
                && let Ok(img) = image::load_from_memory(&bytes)
            {
                raw_icons.insert(name, img.to_rgba8());
            }
        }
        Self {
            raw_icons,
            icon_cache: HashMap::new(),
            circle_cache: HashMap::new(),
            pill_cache: HashMap::new(),
            bubble_cache: HashMap::new(),
            qr_cache: HashMap::new(),
        }
    }

    pub fn icon(&mut self, name: &str, color: Color, scale: f32) -> Image {
        let Some((&static_name, raw)) = self.raw_icons.get_key_value(name) else {
            return Image::default();
        };

        let logical_w = (raw.width() as f32 / 3.0).round().max(1.0);
        let logical_h = (raw.height() as f32 / 3.0).round().max(1.0);
        let target_w = (logical_w * scale).round().max(1.0) as u32;
        let target_h = (logical_h * scale).round().max(1.0) as u32;
        let rgba = [color.red(), color.green(), color.blue(), color.alpha()];
        let key = (static_name, target_w, target_h, rgba);
        if let Some(cached) = self.icon_cache.get(&key) {
            return cached.clone();
        }
        let resized = if raw.width() == target_w && raw.height() == target_h {
            raw.clone()
        } else {
            image::imageops::resize(raw, target_w, target_h, FilterType::Lanczos3)
        };
        let mut buf = SharedPixelBuffer::<Rgba8Pixel>::new(target_w, target_h);
        let slice = buf.make_mut_slice();
        for (dst, src) in slice.iter_mut().zip(resized.pixels()) {
            let a = ((u32::from(src[3]) * u32::from(rgba[3])) + 127) / 255;
            *dst = Rgba8Pixel {
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: a as u8,
            };
        }
        let image = Image::from_rgba8(buf);
        self.icon_cache.insert(key, image.clone());
        image
    }

    pub fn avatar(&mut self, size_px: u32, top: Color, bottom: Color, saved: bool) -> Image {
        let top_arr = [top.red(), top.green(), top.blue(), top.alpha()];
        let bot_arr = [bottom.red(), bottom.green(), bottom.blue(), bottom.alpha()];
        let key = (size_px, top_arr, bot_arr, saved);
        if let Some(cached) = self.circle_cache.get(&key) {
            return cached.clone();
        }
        let size = size_px.max(2);
        let mut pixmap = Pixmap::new(size, size).unwrap();
        let r = size as f32 * 0.5;
        let mut pb = PathBuilder::new();
        pb.push_circle(r, r, r);
        if let Some(path) = pb.finish() {
            let mut paint = Paint::default();
            paint.anti_alias = true;
            if let Some(shader) = LinearGradient::new(
                Point::from_xy(0.0, 0.0),
                Point::from_xy(0.0, size as f32),
                vec![
                    GradientStop::new(
                        0.0,
                        tiny_skia::Color::from_rgba8(
                            top_arr[0], top_arr[1], top_arr[2], top_arr[3],
                        ),
                    ),
                    GradientStop::new(
                        1.0,
                        tiny_skia::Color::from_rgba8(
                            bot_arr[0], bot_arr[1], bot_arr[2], bot_arr[3],
                        ),
                    ),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            ) {
                paint.shader = shader;
            }
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        if saved && let Some(raw_saved) = self.raw_icons.get("saved") {
            let icon_px = ((size as f32) * (24.0 / 46.0)).round().max(1.0) as u32;
            let resized =
                image::imageops::resize(raw_saved, icon_px, icon_px, FilterType::Lanczos3);
            let ox = (size.saturating_sub(icon_px)) / 2;
            let oy = (size.saturating_sub(icon_px)) / 2;
            let data = pixmap.data_mut();
            for y in 0..icon_px {
                for x in 0..icon_px {
                    let sa = u32::from(resized.get_pixel(x, y)[3]);
                    if sa == 0 {
                        continue;
                    }
                    let idx = (((oy + y) * size + (ox + x)) * 4) as usize;
                    let inv = 255 - sa;
                    data[idx] = ((255 * sa + u32::from(data[idx]) * inv + 127) / 255) as u8;
                    data[idx + 1] = ((255 * sa + u32::from(data[idx + 1]) * inv + 127) / 255) as u8;
                    data[idx + 2] = ((255 * sa + u32::from(data[idx + 2]) * inv + 127) / 255) as u8;
                    data[idx + 3] = 255;
                }
            }
        }

        let image = pixmap_to_slint(&pixmap);
        self.circle_cache.insert(key, image.clone());
        image
    }

    pub fn pill(
        &mut self,
        w_px: u32,
        h_px: u32,
        fill: Color,
        border: Color,
        border_px: f32,
    ) -> Image {
        let w = w_px.max(2);
        let h = h_px.max(2);
        let f_arr = [fill.red(), fill.green(), fill.blue(), fill.alpha()];
        let b_arr = [border.red(), border.green(), border.blue(), border.alpha()];
        let key = (w, h, f_arr, b_arr);
        if let Some(cached) = self.pill_cache.get(&key) {
            return cached.clone();
        }
        let mut pixmap = Pixmap::new(w, h).unwrap();
        if border_px > 0.0 && b_arr[3] > 0 {
            if let Some(outer) = rounded_rect_path(0.0, 0.0, w as f32, h as f32, h as f32 * 0.5) {
                let mut paint = Paint::default();
                paint.anti_alias = true;
                paint.set_color_rgba8(b_arr[0], b_arr[1], b_arr[2], b_arr[3]);
                pixmap.fill_path(
                    &outer,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
            let inset = border_px;
            let iw = (w as f32 - inset * 2.0).max(1.0);
            let ih = (h as f32 - inset * 2.0).max(1.0);
            if let Some(inner) = rounded_rect_path(inset, inset, iw, ih, ih * 0.5) {
                let mut paint = Paint::default();
                paint.anti_alias = true;
                paint.set_color_rgba8(f_arr[0], f_arr[1], f_arr[2], f_arr[3]);
                pixmap.fill_path(
                    &inner,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        } else if let Some(path) = rounded_rect_path(0.0, 0.0, w as f32, h as f32, h as f32 * 0.5) {
            let mut paint = Paint::default();
            paint.anti_alias = true;
            paint.set_color_rgba8(f_arr[0], f_arr[1], f_arr[2], f_arr[3]);
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        let image = pixmap_to_slint(&pixmap);
        self.pill_cache.insert(key, image.clone());
        image
    }

    // tail_mode: 0 = none, 1 = left incoming tail, 2 = right outgoing tail
    pub fn bubble(
        &mut self,
        body_w_px: u32,
        h_px: u32,
        tail_w_px: u32,
        radius_px: u32,
        fill: Color,
        tail_mode: u8,
    ) -> Image {
        let total_w = (body_w_px + tail_w_px).max(4);
        let h = h_px.max(4);
        let f_arr = [fill.red(), fill.green(), fill.blue(), fill.alpha()];
        let key = (total_w, h, radius_px, f_arr, tail_mode);
        if let Some(cached) = self.bubble_cache.get(&key) {
            return cached.clone();
        }
        let mut pixmap = Pixmap::new(total_w, h).unwrap();
        let mut paint = Paint::default();
        paint.anti_alias = true;
        paint.set_color_rgba8(f_arr[0], f_arr[1], f_arr[2], f_arr[3]);

        let bx = if tail_mode == 1 {
            tail_w_px as f32
        } else {
            0.0
        };
        let bw = body_w_px as f32;
        let bh = h as f32;
        let r = (radius_px as f32).min(bw * 0.5).min(bh * 0.5);
        let r_bl = if tail_mode == 1 { 0.0 } else { r };
        let r_br = if tail_mode == 2 { 0.0 } else { r };

        if let Some(path) = rounded_rect_corners(bx, 0.0, bw, bh, r, r, r_br, r_bl) {
            pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        if tail_mode == 1 || tail_mode == 2 {
            let tail_name = if tail_mode == 1 {
                "tail-left"
            } else {
                "tail-right"
            };
            if let Some(raw_tail) = self.raw_icons.get(tail_name) {
                let tw = tail_w_px.max(1);
                let th = ((tw as f32) * (10.0 / 6.0)).round().max(1.0) as u32;
                let resized = image::imageops::resize(raw_tail, tw, th, FilterType::Lanczos3);
                let tx = if tail_mode == 1 { 0 } else { body_w_px };
                let ty = h.saturating_sub(th);
                let data = pixmap.data_mut();
                for y in 0..th {
                    for x in 0..tw {
                        let sa = (u32::from(resized.get_pixel(x, y)[3]) * u32::from(f_arr[3])
                            + 127)
                            / 255;
                        if sa == 0 {
                            continue;
                        }
                        let px = tx + x;
                        let py = ty + y;
                        if px < total_w && py < h {
                            let idx = ((py * total_w + px) * 4) as usize;
                            data[idx] = ((u32::from(f_arr[0]) * sa + 127) / 255) as u8;
                            data[idx + 1] = ((u32::from(f_arr[1]) * sa + 127) / 255) as u8;
                            data[idx + 2] = ((u32::from(f_arr[2]) * sa + 127) / 255) as u8;
                            data[idx + 3] = sa as u8;
                        }
                    }
                }
            }
        }

        let image = pixmap_to_slint(&pixmap);
        self.bubble_cache.insert(key, image.clone());
        image
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
