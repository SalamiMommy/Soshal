//! Pure-Rust QR code matrix generation.
//!
//! Provides on-device QR code generation for NIP-19 Nostr entities,
//! Lightning invoices, NWC pairing strings, and TOTP 2FA URIs using `qrcodegen`.

use qrcodegen::{QrCode, QrCodeEcc};

/// QR Code Error Correction Level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QrEcc {
    /// Allows ~7% of codewords to be restored.
    Low,
    /// Allows ~15% of codewords to be restored (recommended default).
    #[default]
    Medium,
    /// Allows ~25% of codewords to be restored.
    Quartile,
    /// Allows ~30% of codewords to be restored.
    High,
}

impl From<QrEcc> for QrCodeEcc {
    fn from(ecc: QrEcc) -> Self {
        match ecc {
            QrEcc::Low => QrCodeEcc::Low,
            QrEcc::Medium => QrCodeEcc::Medium,
            QrEcc::Quartile => QrCodeEcc::Quartile,
            QrEcc::High => QrCodeEcc::High,
        }
    }
}

/// A 2D QR Code bit matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatrix {
    size: usize,
    modules: Vec<bool>,
}

impl QrMatrix {
    /// Generate a QR matrix for arbitrary UTF-8 text with the given ECC level.
    pub fn encode_text(text: &str, ecc: QrEcc) -> Result<Self, String> {
        let code = QrCode::encode_text(text, ecc.into())
            .map_err(|e| format!("failed to encode QR code: {e:?}"))?;

        let size = code.size() as usize;
        let mut modules = Vec::with_capacity(size * size);
        for y in 0..code.size() {
            for x in 0..code.size() {
                modules.push(code.get_module(x, y));
            }
        }

        Ok(Self { size, modules })
    }

    /// Dimension of the QR matrix (width == height).
    pub fn size(&self) -> usize {
        self.size
    }

    /// Returns `true` if the module at (x, y) is dark (set).
    pub fn get_module(&self, x: usize, y: usize) -> bool {
        if x >= self.size || y >= self.size {
            false
        } else {
            self.modules[y * self.size + x]
        }
    }

    /// Render to an SVG document string with the specified border quiet zone.
    pub fn to_svg(&self, border: usize) -> String {
        let full_size = self.size + border * 2;
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" viewBox=\"0 0 {full_size} {full_size}\" stroke=\"none\">\n"
        );
        svg.push_str("  <rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");
        svg.push_str("  <path fill=\"#000000\" d=\"");

        for y in 0..self.size {
            for x in 0..self.size {
                if self.get_module(x, y) {
                    let rx = x + border;
                    let ry = y + border;
                    svg.push_str(&format!("M{rx},{ry}h1v1h-1z "));
                }
            }
        }

        svg.push_str("\"/>\n</svg>\n");
        svg
    }

    /// Render to ASCII block characters (`██` for dark modules, `  ` for light modules).
    pub fn to_ascii(&self, border: usize) -> String {
        let mut out = String::new();
        let full_size = self.size + border * 2;

        for _ in 0..border {
            out.push_str(&"  ".repeat(full_size));
            out.push('\n');
        }

        for y in 0..self.size {
            out.push_str(&"  ".repeat(border));
            for x in 0..self.size {
                if self.get_module(x, y) {
                    out.push_str("██");
                } else {
                    out.push_str("  ");
                }
            }
            out.push_str(&"  ".repeat(border));
            out.push('\n');
        }

        for _ in 0..border {
            out.push_str(&"  ".repeat(full_size));
            out.push('\n');
        }

        out
    }

    /// Render into a raw RGBA8888 byte buffer for direct image/canvas display.
    ///
    /// Dimension of the resulting square buffer is `(size + 2 * border) * scale`.
    pub fn to_rgba_buffer(&self, border: usize, scale: usize) -> Vec<u8> {
        let scale = scale.max(1);
        let dim = (self.size + border * 2) * scale;
        let mut buf = vec![255u8; dim * dim * 4]; // Start with white opaque

        for y in 0..self.size {
            for x in 0..self.size {
                if self.get_module(x, y) {
                    let px_start = (x + border) * scale;
                    let py_start = (y + border) * scale;

                    for dy in 0..scale {
                        for dx in 0..scale {
                            let idx = ((py_start + dy) * dim + (px_start + dx)) * 4;
                            buf[idx] = 0; // R
                            buf[idx + 1] = 0; // G
                            buf[idx + 2] = 0; // B
                            buf[idx + 3] = 255; // A
                        }
                    }
                }
            }
        }

        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_npub_and_inspect_matrix() {
        let npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
        let qr = QrMatrix::encode_text(npub, QrEcc::Medium).expect("npub QR generation");

        // QR code versions have dimensions 21, 25, 29, 33, 37...
        assert!(qr.size() >= 21);
        assert_eq!(qr.modules.len(), qr.size() * qr.size());

        // Top-left finder pattern: (0,0) is always dark
        assert!(qr.get_module(0, 0));
        assert!(qr.get_module(0, 1));
        assert!(qr.get_module(1, 0));

        // Out-of-bounds returns false
        assert!(!qr.get_module(999, 999));
    }

    #[test]
    fn svg_rendering() {
        let qr = QrMatrix::encode_text("https://soshal.net", QrEcc::Low).unwrap();
        let svg = qr.to_svg(4);

        assert!(svg.starts_with("<svg xmlns="));
        assert!(svg.contains("<rect width=\"100%\""));
        assert!(svg.contains("<path fill=\"#000000\""));
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn ascii_rendering() {
        let qr = QrMatrix::encode_text("test", QrEcc::Low).unwrap();
        let ascii = qr.to_ascii(1);
        assert!(ascii.contains("██"));
        assert!(ascii.contains("  "));
    }

    #[test]
    fn rgba_buffer_rendering() {
        let qr = QrMatrix::encode_text(
            "otpauth://totp/Soshal:alice?secret=JBSWY3DPEHPK3PXP",
            QrEcc::Medium,
        )
        .unwrap();
        let scale = 2;
        let border = 2;
        let dim = (qr.size() + border * 2) * scale;
        let buf = qr.to_rgba_buffer(border, scale);

        assert_eq!(buf.len(), dim * dim * 4);

        // Quiet zone corner is white [255, 255, 255, 255]
        assert_eq!(&buf[0..4], &[255, 255, 255, 255]);

        // Finder pattern pixel is black [0, 0, 0, 255]
        let finder_x = border * scale;
        let finder_y = border * scale;
        let idx = (finder_y * dim + finder_x) * 4;
        assert_eq!(&buf[idx..idx + 4], &[0, 0, 0, 255]);
    }
}
