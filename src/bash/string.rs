use alloc::borrow::Cow;
use core::borrow::{Borrow, BorrowMut};
use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::ops::{Deref, DerefMut};
use core::str::{FromStr, Utf8Error};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;

use anyhow::{Context, Result};
use format_bytes::{DisplayBytes, format_bytes};
use tempfile::NamedTempFile;

use super::exec;

fn escape(data: &[u8]) -> Result<Box<str>> {
    let mut temp = NamedTempFile::new()?;
    temp.write_all(data)?;
    let temp = temp.into_temp_path();

    let (dir, file) = exec::resolve_file(&temp)?;
    let command = format_bytes!(b"OUTPUT=\"$(cat '{}')\"", file.as_bytes());

    Ok(exec::rbash_with_output_at(&command, &dir)?.into())
}

fn unescape(text: &str) -> Result<Box<[u8]>> {
    let command = format_bytes!(
        b"INPUT={}
        echo -n \"$INPUT\"",
        text.trim().as_bytes(),
    );

    Ok(exec::rbash(&command)?.into())
}

#[derive(Clone, Eq)]
pub struct BashString {
    escaped: Box<str>,
    raw: Box<[u8]>,
}

impl BashString {
    fn from_escaped_boxed(escaped: Box<str>) -> Result<Self> {
        let raw = unescape(&escaped).with_context(|| format!("while parsing possibly escaped text: {escaped:?}"))?;
        Ok(Self { escaped, raw })
    }

    #[inline]
    pub fn from_escaped(text: impl Into<Box<str>>) -> Result<Self> {
        Self::from_escaped_boxed(text.into())
    }

    fn from_raw_boxed(raw: Box<[u8]>) -> Result<Self> {
        let escaped = escape(&raw).with_context(|| format!("while escaping raw bytes: {}", repr_byte_str(&raw)))?;
        Ok(Self { escaped, raw })
    }

    #[inline]
    pub fn from_raw(bytes: impl Into<Box<[u8]>>) -> Result<Self> {
        Self::from_raw_boxed(bytes.into())
    }

    #[inline]
    #[must_use]
    pub const fn as_escaped(&self) -> &str {
        &self.escaped
    }

    #[inline]
    #[must_use]
    pub const fn as_raw(&self) -> &[u8] {
        &self.raw
    }

    #[inline]
    pub const fn as_utf8(&self) -> Result<&str, Utf8Error> {
        core::str::from_utf8(self.as_raw())
    }

    #[must_use]
    pub fn to_utf8_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_raw())
    }

    #[inline]
    #[must_use]
    pub fn as_repr(&self) -> String {
        repr_byte_str(self.as_raw())
    }
}

const fn repr_byte(byte: u8) -> [u8; 4] {
    const fn hex(num: u8) -> u8 {
        match num {
            0..=9 => b'0' + num,
            10..=16 => b'A' + (num - 10),
            _ => panic!("number not in hexadecimal range"),
        }
    }

    #[expect(clippy::integer_division_remainder_used, reason = "optmized by the compiler")]
    [b'\\', b'x', hex(byte / 16), hex(byte % 16)]
}

fn repr_byte_str(bytes: &[u8]) -> String {
    let content = bytes.utf8_chunks().flat_map(|chunk| {
        let valid = chunk.valid().escape_debug();
        let invalid = chunk.invalid().iter().copied().flat_map(repr_byte).map(Into::into);
        valid.chain(invalid)
    });

    let mut out = String::with_capacity(bytes.len());
    out.push_str("b\"");
    out.extend(content);
    out.push('"');
    out
}

impl fmt::Debug for BashString {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_escaped())
    }
}

impl fmt::Display for BashString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.as_raw().utf8_chunks() {
            fmt::Display::fmt(chunk.valid(), f)?;
            if !chunk.invalid().is_empty() {
                fmt::Display::fmt(&char::REPLACEMENT_CHARACTER, f)?;
            }
        }
        Ok(())
    }
}

impl DisplayBytes for BashString {
    #[inline]
    fn display_bytes(&self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(self.as_raw())
    }
}

impl<T: AsRef<[u8]>> PartialEq<T> for BashString {
    #[inline]
    fn eq(&self, other: &T) -> bool {
        self.as_raw().eq(other.as_ref())
    }
}

impl<T: AsRef<[u8]>> PartialOrd<T> for BashString {
    #[inline]
    fn partial_cmp(&self, other: &T) -> Option<Ordering> {
        self.as_raw().partial_cmp(other.as_ref())
    }
}

impl Ord for BashString {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_raw().cmp(other.as_raw())
    }
}

impl Hash for BashString {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_raw().hash(state);
    }
}

impl<T: ?Sized> AsRef<T> for BashString
where
    Box<[u8]>: AsRef<T>,
{
    #[inline]
    fn as_ref(&self) -> &T {
        self.raw.as_ref()
    }
}

impl<T: ?Sized> AsMut<T> for BashString
where
    Box<[u8]>: AsMut<T>,
{
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        self.raw.as_mut()
    }
}

impl<T: ?Sized> Borrow<T> for BashString
where
    Box<[u8]>: Borrow<T>,
{
    #[inline]
    fn borrow(&self) -> &T {
        self.raw.borrow()
    }
}

impl<T: ?Sized> BorrowMut<T> for BashString
where
    Box<[u8]>: BorrowMut<T>,
{
    #[inline]
    fn borrow_mut(&mut self) -> &mut T {
        self.raw.borrow_mut()
    }
}

impl Deref for BashString {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.raw
    }
}

impl DerefMut for BashString {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.raw
    }
}

impl FromStr for BashString {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> Result<Self> {
        Self::from_escaped(text).or_else(|_| Self::from_raw(text.as_bytes()))
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    #[test]
    fn partial_eq() {
        let string = BashString::from_raw(b"Hello \xF0\x90\x80World".as_slice()).unwrap();
        assert_eq!(string.to_utf8_lossy(), "Hello �World");
        assert_ne!(string, "Hello �World");

        let string = BashString::from_escaped("'Simple string!'").unwrap();
        assert_eq!(string.as_utf8(), Ok("Simple string!"));
        assert_eq!(string.to_utf8_lossy(), "Simple string!");
        assert_eq!(string, "Simple string!");
    }

    #[test]
    fn deref() {
        let data = b"has invalid UTF8: \xF0\x90\x80 !!!".as_slice();
        let string = BashString::from_raw(data).unwrap();
        assert_eq!(string.trim_ascii(), data);
    }

    #[test]
    fn from_str() {
        assert_eq!(BashString::from_str("'escaped text'").unwrap(), "escaped text");
        assert_eq!(BashString::from_str("just normal text").unwrap(), "just normal text");
    }

    #[test]
    fn debug_fmt() {
        let string = BashString::from_raw(b"Hello \xF0\x90\x80World".as_slice()).unwrap();
        assert_eq!(string.as_repr(), stringify!(b"Hello \xF0\x90\x80World"));
        assert_eq!(format!("{:?}", string), "$'Hello \\360\\220\\200World'");

        let text = "another string";
        let string = BashString::from_str(text).unwrap();
        assert_eq!(format!("{string:?}"), format!("'{text}'"));
    }
}
