//! SLIP (Serial Line Internet Protocol) packet framing for Reticulum RNode serial transport.

pub const SLIP_END: u8 = 0xC0;
pub const SLIP_ESC: u8 = 0xDB;
pub const SLIP_ESC_END: u8 = 0xDC;
pub const SLIP_ESC_ESC: u8 = 0xDD;

/// Encodes raw payload data into SLIP framed bytes in a single pass.
pub fn slip_encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 2 + (data.len() / 32).max(14));
    out.push(SLIP_END);
    for &b in data {
        match b {
            SLIP_END => {
                out.push(SLIP_ESC);
                out.push(SLIP_ESC_END);
            }
            SLIP_ESC => {
                out.push(SLIP_ESC);
                out.push(SLIP_ESC_ESC);
            }
            _ => out.push(b),
        }
    }
    out.push(SLIP_END);
    out
}

pub const MAX_SLIP_FRAME_SIZE: usize = 65_536;

/// Decodes SLIP framed bytes into raw payload data.
pub fn slip_decode(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() > MAX_SLIP_FRAME_SIZE {
        return Err("SLIP frame exceeds maximum allowed size".to_string());
    }
    let mut out = Vec::with_capacity(data.len());
    let mut escaping = false;

    for &b in data {
        if escaping {
            match b {
                SLIP_ESC_END => out.push(SLIP_END),
                SLIP_ESC_ESC => out.push(SLIP_ESC),
                _ => out.push(b),
            }
            escaping = false;
        } else if b == SLIP_ESC {
            escaping = true;
        } else if b == SLIP_END {
            // Ignore boundary END markers if buffer is empty
            continue;
        } else {
            out.push(b);
        }
    }

    if escaping {
        return Err("dangling SLIP escape character at EOF".to_string());
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slip_roundtrip() {
        let payload = vec![0x01, 0x02, SLIP_END, 0x03, SLIP_ESC, 0x04];
        let encoded = slip_encode(&payload);
        assert!(encoded.starts_with(&[SLIP_END]));
        assert!(encoded.ends_with(&[SLIP_END]));
        let decoded = slip_decode(&encoded).unwrap();
        assert_eq!(payload, decoded);
    }

    #[test]
    fn test_slip_dangling_escape_rejected() {
        let payload = vec![SLIP_END, 0x01, SLIP_ESC];
        let err = slip_decode(&payload).unwrap_err();
        assert!(err.contains("dangling SLIP escape"));
    }

    #[test]
    fn test_slip_oversized_frame_rejected() {
        let oversized = vec![0x00; MAX_SLIP_FRAME_SIZE + 1];
        let err = slip_decode(&oversized).unwrap_err();
        assert!(err.contains("maximum allowed size"));
    }
}
