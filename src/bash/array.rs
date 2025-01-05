use core::fmt::{self, Write};
use core::hash::{Hash, Hasher};
use core::iter::FusedIterator;

use anyhow::{Result, bail};
use format_bytes::format_bytes;

use super::BashString;
use super::exec;

pub(super) const fn is_array_source(text: &str) -> bool {
    matches!(text.as_bytes().first().copied(), Some(b'(')) && matches!(text.as_bytes().last().copied(), Some(b')'))
}

fn parse_array_content(text: &str) -> Result<Box<[(BashString, BashString)]>> {
    if !is_array_source(text) {
        bail!("invalid array source: {text}");
    }

    let command = format_bytes!(
        b"declare -A ARR
        ARR={}
        for KEY in \"${}!ARR[@]{}\"; do
            printf '%q=%q\\n' \"$KEY\" \"${}ARR[$KEY]{}\"
        done",
        text.as_bytes(),
        b"{",
        b"}",
        b"{",
        b"}",
    );

    let output = exec::rbash(&command)?;
    super::parse_vars(output, |text| BashString::from_escaped(text))
}

#[derive(Clone, PartialEq, Eq)]
pub struct BashArray {
    source: Box<str>,
    content: Box<[(BashString, BashString)]>,
}

impl BashArray {
    fn new_from_boxed(source: Box<str>) -> Result<Self> {
        let content = parse_array_content(source.trim())?;
        Ok(Self { source, content })
    }

    #[inline]
    pub fn new(source: impl Into<Box<str>>) -> Result<Self> {
        Self::new_from_boxed(source.into())
    }

    #[inline]
    pub fn to_bash_string(&self) -> Result<BashString> {
        BashString::from_escaped(self.source.as_ref())
    }

    pub fn to_concatenated_string(&self) -> Result<BashString> {
        let command = format_bytes!(
            b"declare -A ARRAY
            ARRAY={}
            OUTPUT=\"${}ARRAY[*]{}\"",
            self.source.as_bytes(),
            b"{",
            b"}"
        );

        let output = exec::rbash_with_output(&command)?;
        BashString::from_escaped(output)
    }

    #[inline]
    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&BashString, &BashString)> + ExactSizeIterator + FusedIterator {
        self.into_iter().map(|(key, val)| (key, val))
    }

    #[inline]
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &BashString> + ExactSizeIterator + FusedIterator {
        self.into_iter().map(|(key, _)| key)
    }

    #[inline]
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &BashString> + ExactSizeIterator + FusedIterator {
        self.into_iter().map(|(_, value)| value)
    }

    #[inline]
    pub fn into_keys(self) -> impl DoubleEndedIterator<Item = BashString> + ExactSizeIterator + FusedIterator {
        self.into_iter().map(|(key, _)| key)
    }

    #[inline]
    pub fn into_values(self) -> impl DoubleEndedIterator<Item = BashString> + ExactSizeIterator + FusedIterator {
        self.into_iter().map(|(_, value)| value)
    }
}

impl IntoIterator for BashArray {
    type Item = (BashString, BashString);
    type IntoIter = alloc::vec::IntoIter<Self::Item>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.content.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a BashArray {
    type Item = &'a (BashString, BashString);
    type IntoIter = core::slice::Iter<'a, (BashString, BashString)>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.content.iter()
    }
}

impl fmt::Debug for BashArray {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl fmt::Display for BashArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('(')?;
        for string in self.values() {
            f.write_str(string.as_escaped())?;
        }
        f.write_char(')')?;
        Ok(())
    }
}

impl Hash for BashArray {
    fn hash<H: Hasher>(&self, state: &mut H) {
        b'('.hash(state);
        for (idx, (key, value)) in self.iter().enumerate() {
            if idx > 0 {
                b' '.hash(state);
            }
            key.hash(state);
            b'='.hash(state);
            value.hash(state);
        }
        b')'.hash(state);
    }
}
