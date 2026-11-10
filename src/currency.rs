//! Gift appraisal formatting derived from Telegram Desktop v7.4.1
//! ui/text/format_values.cpp/.h at 157f5cfc85e2dd1fa67f129a5e31a81db5ddeb19.
//! GPL-3.0-only; see licenses/NOTICE.md. Amounts are currency MINOR units,
//! not sale prices. Integer arithmetic avoids rounding away the last cent.

pub fn value(amount: i64, currency: &str, approximately: bool) -> String {
    let amount_abs = amount.unsigned_abs();
    if currency == "XTR" {
        return format!("⭐{}", grouped(amount_abs, ','));
    }
    // The same currency rules (including the fallback) as the pinned source.
    // Tuple: thousands, decimal, left, space, exponent.
    let (thousands, decimal, left, space, exponent) = match currency {
        "AED" | "BDT" | "DZD" | "EGP" | "HNL" | "LBP" | "LKR" | "MAD" | "NIO"
        | "PAB" | "PEN" | "QAR" | "SAR" | "SYP" | "YER" | "ZAR" | "ILS" =>
            (',', '.', true, true, 2),
        "AZN" | "BGN" | "BYN" | "CZK" | "EUR" | "GEL" | "HUF" | "PLN" | "RUB"
        | "UZS" => (' ', ',', false, true, 2),
        "AMD" | "MDL" | "MVR" => (',', '.', false, true, 2),
        "BAM" | "HRK" | "RON" | "RSD" | "SEK" | "TRY" => ('.', ',', false, true, 2),
        "ARS" | "BOB" | "BRL" | "COP" | "UYU" | "VEF" => ('.', ',', true, true, 2),
        "BND" | "CRC" | "IDR" => ('.', ',', true, false, 2),
        "ALL" => ('.', ',', false, false, 2),
        "BHD" | "IQD" => (',', '.', true, true, 3),
        "JOD" => (',', '.', true, false, 3),
        "CLP" | "PYG" => ('.', ',', true, true, 0),
        "ISK" | "VND" => ('.', ',', false, true, 0),
        "JPY" | "KRW" | "UGX" => (',', '.', true, false, 0),
        "CHF" => ('\'', '.', false, true, 2),
        "DKK" => ('\0', ',', false, true, 2),
        "IRR" => (',', '/', false, true, 2),
        "KGS" => (' ', '-', false, true, 2),
        "KZT" => (' ', '-', true, false, 2),
        "MNT" => (' ', ',', true, false, 2),
        "NOK" => (' ', ',', true, true, 2),
        "TJS" => (' ', ';', false, true, 2),
        "UAH" => (' ', ',', false, false, 2),
        _ => (',', '.', true, false, 2),
    };
    let symbol = match currency {
        "AUD" => "AU$",
        "BRL" => "R$",
        "CAD" => "CA$",
        "CNY" => "CN¥",
        "EUR" => "€",
        "GBP" => "£",
        "HKD" => "HK$",
        "ILS" => "₪",
        "INR" => "₹",
        "JPY" => "¥",
        "KRW" => "₩",
        "MXN" => "MX$",
        "NZD" => "NZ$",
        "THB" => "฿",
        "TWD" => "NT$",
        "USD" => "$",
        "VND" => "₫",
        _ => currency,
    };
    let unit = 10u64.pow(exponent);
    let whole = grouped(amount_abs / unit, thousands);
    let number = if exponent == 0 {
        whole
    } else {
        format!("{whole}{decimal}{:0width$}", amount_abs % unit, width = exponent as usize)
    };
    let separator = if space { " " } else { "" };
    let signed = if amount < 0 { "−" } else { "" };
    let approximate = if approximately { "~" } else { "" };
    if left {
        format!("{approximate}{signed}{symbol}{separator}{number}")
    } else {
        format!("{approximate}{signed}{number}{separator}{symbol}")
    }
}

fn grouped(value: u64, separator: char) -> String {
    let digits = value.to_string();
    if separator == '\0' {
        return digits;
    }
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index).is_multiple_of(3) {
            result.push(separator);
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appraisals_use_minor_units_and_are_not_resale_asks() {
        assert_eq!(value(12345, "USD", true), "~$123.45");
        assert_eq!(value(12300, "USD", true), "~$123.00");
        assert_eq!(value(123456, "EUR", true), "~1 234,56 €");
        assert_eq!(value(123400, "RUB", true), "~1 234,00 RUB");
        assert_eq!(value(12345, "JPY", true), "~¥12,345");
        assert_eq!(value(12345, "BHD", true), "~BHD 12.345");
        assert_eq!(value(1234, "UNKNOWN", true), "~UNKNOWN12.34");
    }

    #[test]
    fn formatting_preserves_negative_values_and_large_integer_precision() {
        assert_eq!(value(-1200, "USD", false), "−$12.00");
        assert_eq!(value(i64::MAX, "USD", true), "~$92,233,720,368,547,758.07");
        assert_eq!(value(i64::MIN, "USD", true), "~−$92,233,720,368,547,758.08");
        // Stars are integral, not USD cents. Their grouping is locale-driven
        // upstream; this deterministic fallback uses the formatter's comma.
        assert_eq!(value(12345, "XTR", false), "⭐12,345");
    }
}
