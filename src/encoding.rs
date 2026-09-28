//! Strict file transcoding and incremental terminal decoding.
use anyhow::{Result, bail};
use encoding_rs::{BIG5, GB18030, GBK, UTF_8, UTF_16BE, UTF_16LE};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Encoding {
    #[default]
    #[serde(rename = "UTF-8")]
    Utf8,
    #[serde(rename = "GBK")]
    Gbk,
    #[serde(rename = "GB18030")]
    Gb18030,
    #[serde(rename = "Big5")]
    Big5,
    #[serde(rename = "UTF-16 LE")]
    Utf16Le,
    #[serde(rename = "UTF-16 BE")]
    Utf16Be,
}
impl Encoding {
    pub const TERMINAL: [Self; 4] = [Self::Utf8, Self::Gbk, Self::Gb18030, Self::Big5];
    pub const FILE: [Self; 6] = [
        Self::Utf8,
        Self::Gbk,
        Self::Gb18030,
        Self::Big5,
        Self::Utf16Le,
        Self::Utf16Be,
    ];
    /// Canonical import/export and UI name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Gbk => "GBK",
            Self::Gb18030 => "GB18030",
            Self::Big5 => "Big5",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
    /// UTF-16 terminal streams are intentionally unsupported.
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Utf16Le | Self::Utf16Be)
    }
    /// Standard encoding implementation used for decoding.
    pub fn codec(self) -> &'static encoding_rs::Encoding {
        match self {
            Self::Utf8 => UTF_8,
            Self::Gbk => GBK,
            Self::Gb18030 => GB18030,
            Self::Big5 => BIG5,
            Self::Utf16Le => UTF_16LE,
            Self::Utf16Be => UTF_16BE,
        }
    }
}

/// A decoded document retains its BOM decision independently from later edits.
#[derive(Debug, Clone)]
pub struct TextFile {
    pub text: String,
    pub encoding: Encoding,
    pub bom: bool,
}

/// Decode strictly; preserve BOM and reject binary control characters.
pub fn decode_file(bytes: &[u8], requested: Option<Encoding>) -> Result<TextFile> {
    let detected = if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        Some((Encoding::Utf8, 3))
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        Some((Encoding::Utf16Le, 2))
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        Some((Encoding::Utf16Be, 2))
    } else {
        None
    };
    let encoding = requested.or(detected.map(|d| d.0)).unwrap_or_default();
    let offset = detected.filter(|d| d.0 == encoding).map_or(0, |d| d.1);
    let (text, errors) = encoding
        .codec()
        .decode_without_bom_handling(&bytes[offset..]);
    if errors {
        bail!(
            "Invalid {} text; choose the correct file encoding",
            encoding.label()
        );
    }
    if text
        .chars()
        .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}')))
    {
        bail!("Binary content cannot be opened in the text editor");
    }
    Ok(TextFile {
        text: text.into_owned(),
        encoding,
        bom: offset > 0,
    })
}

/// Encode without replacement characters. UTF-16 is written explicitly because encoding_rs encodes it as UTF-8.
pub fn encode(text: &str, encoding: Encoding, bom: bool) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match encoding {
        Encoding::Utf16Le | Encoding::Utf16Be => {
            if bom {
                out.extend(if encoding == Encoding::Utf16Le {
                    [0xff, 0xfe]
                } else {
                    [0xfe, 0xff]
                });
            }
            for word in text.encode_utf16() {
                out.extend(if encoding == Encoding::Utf16Le {
                    word.to_le_bytes()
                } else {
                    word.to_be_bytes()
                });
            }
        }
        _ => {
            let (bytes, _, errors) = encoding.codec().encode(text);
            if errors {
                bail!(
                    "Text cannot be represented in {}; nothing was written",
                    encoding.label()
                );
            }
            if bom && encoding == Encoding::Utf8 {
                out.extend([0xef, 0xbb, 0xbf]);
            }
            out.extend(bytes.iter());
        }
    }
    Ok(out)
}

/// Stateful decoding prevents split multibyte characters from becoming corrupt terminal output.
pub struct TerminalDecoder {
    decoder: encoding_rs::Decoder,
}
impl TerminalDecoder {
    /// Initialize a stream decoder without BOM-driven encoding changes.
    pub fn new(encoding: Encoding) -> Self {
        Self {
            decoder: encoding.codec().new_decoder_without_bom_handling(),
        }
    }
    /// Decode one arbitrary network chunk, reporting malformed input to the caller.
    pub fn feed(&mut self, input: &[u8]) -> (String, bool) {
        let capacity = self
            .decoder
            .max_utf8_buffer_length(input.len())
            .unwrap_or(input.len() * 4 + 16);
        let mut output = String::with_capacity(capacity);
        let (_, _, errors) = self.decoder.decode_to_string(input, &mut output, false);
        (output, errors)
    }
}
