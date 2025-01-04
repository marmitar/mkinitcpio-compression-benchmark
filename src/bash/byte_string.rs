//! Linux version of [`OsString`](std::ffi::OsString), with [`Display`](fmt::Display) via [`String::from_utf8_lossy`].

use alloc::borrow::Cow;
use core::borrow::Borrow;
use core::borrow::BorrowMut;
use core::convert::Infallible;
use core::fmt;
use core::ops::{Deref, DerefMut};
use core::str::FromStr;
use core::str::Utf8Error;
use std::io::{self, Write};

use format_bytes::DisplayBytes;

/// Linux version of [`OsString`](std::ffi::OsString), with [`Display`](fmt::Display) via [`String::from_utf8_lossy`].
///
/// Useful for FFI data in Linux that is mostly UTF8, which usually is the case for shell output.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ByteString {
    /// Original binary data.
    pub data: Box<[u8]>,
}

impl ByteString {
    /// Copies all bytes into a new byte string.
    #[inline]
    #[must_use]
    pub fn new(data: impl AsRef<[u8]>) -> Self {
        Self::from(data.as_ref())
    }

    /// Converts a byte string to a UTF-8 [`str`].
    ///
    /// See [`std::str::from_utf8`].
    ///
    /// # Errors
    ///
    /// If any invalid UTF-8 character is found.
    #[inline]
    pub const fn to_utf8(&self) -> Result<&str, Utf8Error> {
        core::str::from_utf8(&self.data)
    }

    /// Converts a byte string to a UTF-8 [`str`] including invalid characters.
    ///
    /// See [`String::from_utf8_lossy`].
    #[inline]
    #[must_use]
    pub fn to_utf8_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self)
    }
}

impl PartialEq<str> for ByteString {
    fn eq(&self, other: &str) -> bool {
        self.to_utf8() == Ok(other)
    }
}

impl<T: ?Sized> PartialEq<&T> for ByteString
where
    Self: PartialEq<T>,
{
    #[inline]
    fn eq(&self, &other: &&T) -> bool {
        self.eq(other)
    }
}

impl fmt::Debug for ByteString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl fmt::Display for ByteString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for chunk in self.utf8_chunks() {
            fmt::Display::fmt(chunk.valid(), f)?;
            if !chunk.invalid().is_empty() {
                fmt::Display::fmt(&char::REPLACEMENT_CHARACTER, f)?;
            }
        }
        Ok(())
    }
}

impl DisplayBytes for ByteString {
    #[inline]
    fn display_bytes(&self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(&self.data)
    }
}

impl<T: Into<Box<[u8]>>> From<T> for ByteString {
    #[inline]
    fn from(value: T) -> Self {
        Self { data: value.into() }
    }
}

impl<T: ?Sized> AsRef<T> for ByteString
where
    Box<[u8]>: AsRef<T>,
{
    #[inline]
    fn as_ref(&self) -> &T {
        self.data.as_ref()
    }
}

impl<T: ?Sized> AsMut<T> for ByteString
where
    Box<[u8]>: AsMut<T>,
{
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        self.data.as_mut()
    }
}

impl<T: ?Sized> Borrow<T> for ByteString
where
    Box<[u8]>: Borrow<T>,
{
    #[inline]
    fn borrow(&self) -> &T {
        self.data.borrow()
    }
}

impl<T: ?Sized> BorrowMut<T> for ByteString
where
    Box<[u8]>: BorrowMut<T>,
{
    #[inline]
    fn borrow_mut(&mut self) -> &mut T {
        self.data.borrow_mut()
    }
}

impl Deref for ByteString {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        &self.data
    }
}

impl DerefMut for ByteString {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

impl FromStr for ByteString {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Infallible> {
        Ok(Self {
            data: s.as_bytes().into(),
        })
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    #[test]
    fn partial_eq() {
        assert_eq!(ByteString::new(b"Hello \xF0\x90\x80World").to_utf8_lossy(), "Hello �World");
        assert_ne!(ByteString::new(b"Hello \xF0\x90\x80World"), "Hello �World");

        assert_eq!(ByteString::new(b"Simple string!").to_utf8(), Ok("Simple string!"));
        assert_eq!(ByteString::new(b"Simple string!").to_utf8_lossy(), "Simple string!");
        assert_eq!(ByteString::new(b"Simple string!"), "Simple string!");
    }

    #[test]
    fn deref() {
        let data = b"has invalid UTF8: \xF0\x90\x80 !!!";
        let string = ByteString::new(data);
        assert_eq!(string.trim_ascii(), data);
    }

    #[test]
    fn from_str() {
        let text = "just normal text";
        assert_eq!(ByteString::from_str(text).unwrap(), text);
    }
}
