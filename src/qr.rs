pub fn encode_qr_matrix(text: &str) -> String {
    let Ok(code) = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M)
    else {
        return String::new();
    };
    let n = code.width();
    (0..n)
        .map(|y| {
            (0..n)
                .map(|x| {
                    if code[(x, y)] == qrcode::Color::Dark {
                        '1'
                    } else {
                        '0'
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
