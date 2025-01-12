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

/// Apply Rust escaping rules for a byte string.
pub const fn escaped(bytes: &[u8]) -> impl Debug + Display + Copy {
    DisplayEscaped::<false> { bytes }
}

/// Represents a byte array as Rust source, escaping non UTF-8 characters with hexadecimal (`\x`).
pub const fn repr(bytes: &[u8]) -> impl Debug + Display + Copy {
    DisplayEscaped::<true> { bytes }
}

/// Display a byte string applying Rust escaping rules.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct DisplayEscaped<'a, const TAGGED: bool> {
    /// Inner byte string.
    bytes: &'a [u8],
}

impl<const TAGGED: bool> Debug for DisplayEscaped<'_, TAGGED> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.bytes, f)
    }
}

impl<const TAGGED: bool> Display for DisplayEscaped<'_, TAGGED> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if TAGGED {
            f.write_char('b')?;
        }
        f.write_char('"')?;
        Display::fmt(&self.bytes.escape_ascii(), f)?;
        f.write_char('"')?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use pretty_assertions::{assert_eq, assert_ne};
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

    fn repr_byte_str(bytes: &[u8], tagged: bool) -> String {
        let mut out = String::with_capacity(bytes.len());
        if tagged {
            out.push('b');
        }
        out.push('"');
        for chunk in bytes.utf8_chunks() {
            write!(out, "{}", chunk.valid().escape_debug()).unwrap();
            for byte in chunk.invalid() {
                write!(out, "\\x{byte:x}").unwrap();
            }
        }
        out.push('"');
        out
    }

    #[test]
    fn escaped_representation() {
        let target = b"simple text";
        assert_eq!(repr(target).to_string(), stringify!(b"simple text"), "repr via stringify");
        assert_eq!(repr(target).to_string(), format!("b\"{}\"", target.escape_ascii()), "repr via escape_ascii");
        assert_eq!(repr(target).to_string(), repr_byte_str(target, true), "repr via custom implementation");
        assert_eq!(repr(target).to_string(), format!("b{:?}", OsStr::from_bytes(target)), "repr via OsStrs");
        assert_eq!(repr(target).to_string(), format!("b{}", escaped(target)), "repr via escape + b");
        assert_eq!(escaped(target).to_string(), repr_byte_str(target, false), "escape via custom implementation");

        let target = b"binary\xff\xffdata";
        assert_eq!(repr(target).to_string(), stringify!(b"binary\xff\xffdata"), "repr via stringify");
        assert_eq!(repr(target).to_string(), format!("b\"{}\"", target.escape_ascii()), "repr via escape_ascii");
        assert_eq!(repr(target).to_string(), repr_byte_str(target, true), "repr via custom implementation");
        assert_eq!(
            repr(target).to_string().to_lowercase(),
            format!("b{:?}", OsStr::from_bytes(target)).to_lowercase(),
            "repr via OsStrs (case insensitive)"
        );
        assert_eq!(repr(target).to_string(), format!("b{}", escaped(target)), "repr via escape + b");
        assert_eq!(escaped(target).to_string(), repr_byte_str(target, false), "escape via custom implementation");

        let target = b"text with \0 binary \xff data";
        assert_eq!(
            repr(target).to_string(),
            stringify!(b"text with \x00 binary \xff data"),
            "repr via stringify (almost)"
        );
        assert_eq!(repr(target).to_string(), format!("b\"{}\"", target.escape_ascii()), "repr via escape_ascii");
        assert_ne!(repr(target).to_string(), repr_byte_str(target, true), "repr via custom implementation (almost)");
        assert_ne!(repr(target).to_string(), format!("b{:?}", OsStr::from_bytes(target)), "repr via OsStrs (almost)");
        assert_eq!(repr(target).to_string(), format!("b{}", escaped(target)), "repr via escape + b");
        assert_ne!(
            escaped(target).to_string(),
            repr_byte_str(target, false),
            "escape via custom implementation (almost)"
        );

        let target = b"some 'quoted string' and \"double\"";
        assert_eq!(
            repr(target).to_string(),
            stringify!(b"some \'quoted string\' and \"double\""),
            "repr via stringify (almost)"
        );
        assert_eq!(repr(target).to_string(), format!("b\"{}\"", target.escape_ascii()), "repr via escape_ascii");
        assert_eq!(repr(target).to_string(), repr_byte_str(target, true), "repr via custom implementation");
        assert_eq!(repr(target).to_string(), format!("b{:?}", OsStr::from_bytes(target)), "repr via OsStrs (almost)");
        assert_eq!(repr(target).to_string(), format!("b{}", escaped(target)), "repr via escape + b");
        assert_eq!(escaped(target).to_string(), repr_byte_str(target, false), "escape via custom implementation");
    }
}
