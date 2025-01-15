//! Utilities for (byte) strings.

use std::fmt::{self, Debug, Display, Formatter, Write};
use std::iter::FusedIterator;

/// Splits a byte string into lines.
pub fn lines(bytes: &[u8]) -> impl FusedIterator<Item = &[u8]> {
    bytes
        .trim_ascii()
        .split(|&ch| ch == b'\n')
        .skip_while(|line| line.trim_ascii().is_empty())
}

/// Display a byte string as UTF-8, replacing invalid characters.
///
/// Similar to [`String::from_utf8_lossy`] without converting to a string first.
#[inline]
#[must_use]
pub const fn utf8_lossy(bytes: &[u8]) -> impl Debug + Display + Copy {
    Utf8Display::<true> { bytes }
}

/// Display a byte string as UTF-8, escaping invalid characters.
///
/// Escape invalid bytes as hexadecimal.
#[inline]
#[must_use]
pub const fn utf8_escaped(bytes: &[u8]) -> impl Debug + Display + Copy {
    Utf8Display::<false> { bytes }
}

/// Display a byte string converting to UTF-8 data.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct Utf8Display<'a, const LOSSY: bool> {
    /// Inner byte string.
    bytes: &'a [u8],
}

impl<const LOSSY: bool> Debug for Utf8Display<'_, LOSSY> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.bytes, f)
    }
}

impl<const LOSSY: bool> Display for Utf8Display<'_, LOSSY> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for chunk in self.bytes.utf8_chunks() {
            f.write_str(chunk.valid())?;
            if LOSSY {
                if !chunk.invalid().is_empty() {
                    f.write_char(char::REPLACEMENT_CHARACTER)?;
                }
            } else {
                for &byte in chunk.invalid() {
                    f.write_str("\\x")?;
                    for digit in repr_hex(byte) {
                        f.write_char(digit.into())?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Represents a single byte as a hexadecimal pair.
const fn repr_hex(byte: u8) -> [u8; 2] {
    #[inline]
    const fn hex(num: u8) -> u8 {
        match num {
            0..=9 => b'0' + num,
            10..=16 => b'A' + (num - 10),
            _ => panic!("number not in hexadecimal range"),
        }
    }

    macro_rules! hex {
        () => { hex!(0, 1, 2, 3) };
        ($($i:literal),*) => { [$(
            const { hex(4 * $i + 0) },
            const { hex(4 * $i + 1) },
            const { hex(4 * $i + 2) },
            const { hex(4 * $i + 3) },
        )*] };
    }

    const HEX: [u8; 16] = hex!();

    #[expect(clippy::integer_division_remainder_used, reason = "optmized by the compiler")]
    [HEX[(byte / 16) as usize], HEX[(byte % 16) as usize]]
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use test_log::test;

    use super::*;

    fn next_line<'a>(mut lines: impl Iterator<Item = &'a [u8]>) -> Option<&'a str> {
        lines.next().map(|bytes| std::str::from_utf8(bytes).unwrap())
    }

    #[test]
    fn lines_trim_first() {
        let mut lines = lines(
            b"

some text

in the middle

        ",
        );

        assert_eq!(next_line(&mut lines), Some("some text"));
        assert_eq!(next_line(&mut lines), Some(""));
        assert_eq!(next_line(&mut lines), Some("in the middle"));
        assert_eq!(next_line(&mut lines), None);
    }

    #[test]
    fn utf8_representation() {
        let target = b"simple text";
        assert_eq!(utf8_lossy(target).to_string(), "simple text");
        assert_eq!(utf8_escaped(target).to_string(), "simple text");

        let target = b"binary\xff\xffdata";
        assert_eq!(utf8_lossy(target).to_string(), "binary��data");
        assert_eq!(utf8_escaped(target).to_string(), "binary\\xFF\\xFFdata");

        let target = b"text with \0 binary \xff data";
        assert_eq!(utf8_lossy(target).to_string(), "text with \0 binary � data");
        assert_eq!(utf8_escaped(target).to_string(), "text with \0 binary \\xFF data");

        let target = b"some 'quoted string' and \"double\"";
        assert_eq!(utf8_lossy(target).to_string(), "some 'quoted string' and \"double\"");
        assert_eq!(utf8_escaped(target).to_string(), "some 'quoted string' and \"double\"");
    }

    proptest! {
        #[test]
        fn prop_matches_std_utf8_lossy(data: Vec<u8>) {
            prop_assert_eq!(utf8_lossy(&data).to_string(), String::from_utf8_lossy(&data));
        }

        fn prop_utf8_data_is_converted_the_same(text: String) {
            prop_assert_eq!(utf8_lossy(text.as_bytes()).to_string(), text.as_ref());
            prop_assert_eq!(utf8_escaped(text.as_bytes()).to_string(), text.as_ref());
        }
    }

    #[test]
    fn repr_hex_equals_upper_hex() {
        for byte in 0x00..=0xFF {
            assert_eq!(std::str::from_utf8(&repr_hex(byte)).unwrap(), format!("{byte:02X}"));
        }
    }
}
