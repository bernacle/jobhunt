const DIGITS: &[u8; 16] = b"0123456789abcdef";

pub(crate) fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

/// Decodes lowercase or uppercase hex into exactly `N` bytes.
pub(crate) fn decode<const N: usize>(input: &str) -> Option<[u8; N]> {
    let raw = input.as_bytes();
    if raw.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    let (pairs, _) = raw.as_chunks::<2>();
    for (slot, [high, low]) in out.iter_mut().zip(pairs) {
        *slot = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(out)
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let bytes = [0x00, 0x7f, 0x80, 0xff, 0x12];
        let encoded = encode(&bytes);
        assert_eq!(encoded, "007f80ff12");
        assert_eq!(decode::<5>(&encoded), Some(bytes));
        assert_eq!(decode::<5>("007F80FF12"), Some(bytes));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(decode::<2>("abc"), None);
        assert_eq!(decode::<2>("zzzz"), None);
    }
}
