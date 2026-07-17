use qrcodegen::{QrCode, QrCodeEcc};

const QUIET_ZONE_MODULES: i32 = 2;

pub fn qr_unicode_lines(text: &str) -> Result<Vec<String>, qrcodegen::DataTooLong> {
    let qr = QrCode::encode_text(text, QrCodeEcc::Low)?;
    let size = qr.size();
    let total_modules = size + QUIET_ZONE_MODULES * 2;
    let mut lines = Vec::with_capacity(half_block_line_count(total_modules));

    let mut top_y = -QUIET_ZONE_MODULES;
    while top_y < size + QUIET_ZONE_MODULES {
        let bottom_y = top_y + 1;
        let mut line = String::with_capacity(total_modules as usize * 3);
        for x in -QUIET_ZONE_MODULES..size + QUIET_ZONE_MODULES {
            let top = module_with_quiet_zone(&qr, x, top_y);
            let bottom = module_with_quiet_zone(&qr, x, bottom_y);
            line.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        lines.push(line);
        top_y += 2;
    }

    Ok(lines)
}

fn module_with_quiet_zone(qr: &QrCode, x: i32, y: i32) -> bool {
    (0..qr.size()).contains(&x) && (0..qr.size()).contains(&y) && qr.get_module(x, y)
}

fn half_block_line_count(module_count: i32) -> usize {
    debug_assert!(module_count >= 0);
    ((module_count / 2) + (module_count % 2)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcodegen::{QrCode, QrCodeEcc};

    const QUIET_ZONE: usize = 2;

    #[test]
    fn qr_unicode_lines_renders_structural_shape_with_quiet_zone() {
        let text = "http://100.64.0.5:3000/remote";
        let qr = QrCode::encode_text(text, QrCodeEcc::Low).unwrap();
        let expected_width = qr.size() as usize + QUIET_ZONE * 2;
        let expected_height = expected_width / 2 + usize::from(expected_width & 1 != 0);

        let lines = qr_unicode_lines(text).unwrap();

        assert_eq!(lines.len(), expected_height);
        assert!(lines[0].chars().all(|ch| ch == ' '));
        assert!(lines[1].starts_with("  █▀▀▀▀▀█"));
        assert!(lines.iter().all(|line| !line.contains('\u{1b}')));
        assert!(lines
            .iter()
            .all(|line| line.chars().count() == expected_width));
    }

    #[test]
    fn qr_unicode_lines_uses_half_blocks_for_odd_content_row() {
        let text = "https://x.test/r";
        let qr = QrCode::encode_text(text, QrCodeEcc::Low).unwrap();
        let total_modules = qr.size() as usize + QUIET_ZONE * 2;

        let lines = qr_unicode_lines(text).unwrap();

        assert_eq!(total_modules % 2, 1);
        assert!(lines.len() >= 2);
        assert!(lines[lines.len() - 2].contains('▀'));
        assert!(lines.last().unwrap().chars().all(|ch| ch == ' '));
    }

    #[test]
    fn qr_unicode_lines_is_deterministic_for_short_text() {
        let expected = [
            ".........................",
            "..█▀▀▀▀▀█.██.▄..█▀▀▀▀▀█..",
            "..█.███.█.▀█▀▀█.█.███.█..",
            "..█.▀▀▀.█.▄█▀.▀.█.▀▀▀.█..",
            "..▀▀▀▀▀▀▀.█.█▄█.▀▀▀▀▀▀▀..",
            "..▄▄█▀▀▄▀▄▀▀.█.█▀█.▄█▀█..",
            "..▀█▀█▄▄▀█▄▀█▄..▄▄███▀...",
            "..▀▀..▀.▀▀▄▀▄█▄▄▀▄▀..▄▄..",
            "..█▀▀▀▀▀█..█▄▀..▀▀▄██▀▄..",
            "..█.███.█.██.▄..▄▄██▄▀...",
            "..█.▀▀▀.█.▀▀█▀██▄▄.▄█....",
            "..▀▀▀▀▀▀▀...▀▀.▀▀▀▀..▀...",
            ".........................",
        ]
        .into_iter()
        .map(|line| line.replace('.', " "))
        .collect::<Vec<_>>();

        assert_eq!(qr_unicode_lines("peitho").unwrap(), expected);
    }

    #[test]
    fn qr_unicode_lines_returns_error_for_over_capacity_text() {
        let text = "x".repeat(3 * 1024 + 1);

        assert!(qr_unicode_lines(&text).is_err());
    }
}
