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

/// Control characters accepted in text/log/config content. NUL is always rejected.
/// ESC and other harmless C0 bytes appear in ANSI-colored logs.
fn is_allowed_control(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\t' | '\u{c}' | '\u{1b}' | '\u{7}' | '\u{8}' | '\u{b}'
    )
}

/// True when the decoded text still looks like a binary blob.
///
/// Allowlisted text-like names may keep any C0 except NUL (common in logs); other
/// files only keep the ANSI-safe subset above. NUL always fails.
pub fn has_binary_content(text: &str, allowlisted: bool) -> bool {
    text.chars().any(|c| {
        if c == '\0' {
            return true;
        }
        if !c.is_control() {
            return false;
        }
        if is_allowed_control(c) {
            return false;
        }
        // Remaining C0 on allowlisted names stay; DEL and non-allowlisted C0 fail.
        !(allowlisted && (c as u32) < 0x20)
    })
}

/// Basename heuristics for files that are usually text even without a UTF-8 BOM.
///
/// Extensions cover common logs, configs and source; bare names cover Docker/Make/
/// license files; `*.conf.*` and dotted env/config stems cover versioned configs.
pub fn likely_text_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.is_empty() || name == "." || name == ".." {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    const NAMES: &[&str] = &[
        "dockerfile",
        "makefile",
        "gnumakefile",
        "readme",
        "license",
        "licence",
        "copying",
        "authors",
        "changelog",
        "changes",
        "gemfile",
        "rakefile",
        "procfile",
        "vagrantfile",
        "cmakelists.txt",
    ];
    if NAMES
        .iter()
        .any(|n| lower == *n || lower.starts_with(&format!("{n}.")))
    {
        return true;
    }
    // Versioned config: app.conf.default, nginx.conf.template, .env.local
    if lower.contains(".conf.") || lower.starts_with(".env") {
        return true;
    }
    let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    matches!(
        ext,
        "txt"
            | "log"
            | "md"
            | "markdown"
            | "json"
            | "yml"
            | "yaml"
            | "toml"
            | "ini"
            | "conf"
            | "cfg"
            | "env"
            | "xml"
            | "csv"
            | "tsv"
            | "properties"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "py"
            | "rs"
            | "go"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "mjs"
            | "cjs"
            | "css"
            | "scss"
            | "less"
            | "html"
            | "htm"
            | "svg"
            | "sql"
            | "service"
            | "timer"
            | "socket"
            | "desktop"
            | "list"
            | "nfo"
            | "rst"
            | "adoc"
            | "tex"
            | "c"
            | "h"
            | "cc"
            | "cpp"
            | "hpp"
            | "java"
            | "kt"
            | "rb"
            | "php"
            | "pl"
            | "lua"
            | "r"
            | "swift"
            | "m"
            | "mm"
            | "cs"
            | "fs"
            | "ex"
            | "exs"
            | "erl"
            | "hrl"
            | "vim"
            | "gradle"
            | "kts"
            | "tf"
            | "hcl"
            | "nix"
            | "vue"
            | "svelte"
            | "astro"
    )
}

fn detect_bom(bytes: &[u8]) -> Option<(Encoding, usize)> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        Some((Encoding::Utf8, 3))
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        Some((Encoding::Utf16Le, 2))
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        Some((Encoding::Utf16Be, 2))
    } else {
        None
    }
}

fn decode_strict(bytes: &[u8], encoding: Encoding, bom_len: usize) -> Result<TextFile> {
    let offset = if bom_len > 0 { bom_len } else { 0 };
    let (text, errors) = encoding
        .codec()
        .decode_without_bom_handling(&bytes[offset..]);
    if errors {
        bail!(
            "Invalid {} text; choose the correct file encoding",
            encoding.label()
        );
    }
    Ok(TextFile {
        text: text.into_owned(),
        encoding,
        bom: offset > 0,
    })
}

/// Decode strictly; preserve BOM and reject NUL / clearly binary control characters.
pub fn decode_file(bytes: &[u8], requested: Option<Encoding>) -> Result<TextFile> {
    let detected = detect_bom(bytes);
    let encoding = requested.or(detected.map(|d| d.0)).unwrap_or_default();
    let bom_len = detected.filter(|d| d.0 == encoding).map_or(0, |d| d.1);
    let file = decode_strict(bytes, encoding, bom_len)?;
    if has_binary_content(&file.text, false) {
        bail!("Binary content cannot be opened in the text editor");
    }
    Ok(file)
}

/// Decode for in-app open/preview with allowlist-aware encoding fallbacks.
///
/// Returns `(file, lossy)`. When `lossy` is true the text may contain U+FFFD or
/// Latin-1-mapped bytes and must not be saved. Callers that refuse lossy opens
/// should treat `lossy == true` as failure.
pub fn decode_for_open(
    bytes: &[u8],
    path: &str,
    requested: Option<Encoding>,
    allow_lossy: bool,
) -> Result<(TextFile, bool)> {
    let allowlisted = likely_text_name(path);
    let detected = detect_bom(bytes);

    let try_one = |encoding: Encoding, bom_len: usize| -> Result<TextFile> {
        let file = decode_strict(bytes, encoding, bom_len)?;
        if has_binary_content(&file.text, allowlisted) {
            bail!("Binary content cannot be opened in the text editor");
        }
        Ok(file)
    };

    // Explicit encoding or BOM wins first.
    if let Some(encoding) = requested.or(detected.map(|d| d.0)) {
        let bom_len = detected.filter(|d| d.0 == encoding).map_or(0, |d| d.1);
        match try_one(encoding, bom_len) {
            Ok(file) => return Ok((file, false)),
            Err(error) if requested.is_some() || detected.is_some() && !allowlisted => {
                return Err(error);
            }
            Err(_) => {}
        }
    } else {
        // Auto: strict UTF-8 without BOM.
        match try_one(Encoding::Utf8, 0) {
            Ok(file) => return Ok((file, false)),
            Err(error) if !allowlisted => return Err(error),
            Err(_) => {}
        }
    }

    // Allowlisted Auto: try common Chinese encodings before any lossy path.
    if allowlisted && requested.is_none() {
        for encoding in [Encoding::Gbk, Encoding::Gb18030] {
            if let Ok(file) = try_one(encoding, 0) {
                return Ok((file, false));
            }
        }
    }

    if !(allowlisted && allow_lossy) {
        bail!("Invalid UTF-8 text; choose the correct file encoding");
    }

    // Lossy preview: prefer UTF-8 with replacement characters so the UI can show
    // U+FFFD markers; fall back to Latin-1 (every byte maps) when that still
    // looks binary under the allowlisted control policy.
    let (cow, _, had_errors) = UTF_8.decode(bytes);
    let utf8_lossy = TextFile {
        text: cow.into_owned(),
        encoding: Encoding::Utf8,
        bom: false,
    };
    if had_errors && !has_binary_content(&utf8_lossy.text, true) {
        return Ok((utf8_lossy, true));
    }
    if !has_binary_content(&utf8_lossy.text, true) && !had_errors {
        return Ok((utf8_lossy, false));
    }

    let latin1: String = bytes.iter().map(|&b| b as char).collect();
    if latin1.contains('\0') || has_binary_content(&latin1, true) {
        bail!("Binary content cannot be opened in the text editor");
    }
    Ok((
        TextFile {
            text: latin1,
            encoding: Encoding::Utf8,
            bom: false,
        },
        true,
    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_covers_logs_configs_and_common_names() {
        assert!(likely_text_name("/var/log/app.log"));
        assert!(likely_text_name("nginx.conf"));
        assert!(likely_text_name("app.conf.default"));
        assert!(likely_text_name(".env.local"));
        assert!(likely_text_name("Dockerfile"));
        assert!(likely_text_name("Makefile"));
        assert!(likely_text_name("README.md"));
        assert!(likely_text_name("src/main.rs"));
        assert!(!likely_text_name("image.png"));
        assert!(!likely_text_name("payload.bin"));
        assert!(!likely_text_name("."));
    }

    #[test]
    fn control_policy_allows_ansi_esc_rejects_nul() {
        assert!(!has_binary_content("ok\n\t\r\u{c}", false));
        assert!(!has_binary_content("color\u{1b}[31mred\u{1b}[0m", false));
        assert!(has_binary_content("nul\0byte", false));
        assert!(has_binary_content("nul\0byte", true));
        // Device control that is not in the ANSI-safe set fails for non-allowlisted.
        assert!(has_binary_content("x\u{01}y", false));
        assert!(!has_binary_content("x\u{01}y", true));
    }

    #[test]
    fn gbk_allowlisted_falls_back_without_lossy() {
        let bytes = Encoding::Gbk.codec().encode("中文配置\n").0;
        let (file, lossy) = decode_for_open(&bytes, "app.conf", None, false).unwrap();
        assert!(!lossy);
        assert_eq!(file.encoding, Encoding::Gbk);
        assert!(file.text.contains("中文"));
    }

    #[test]
    fn non_allowlisted_invalid_utf8_still_fails() {
        assert!(decode_for_open(&[0xff, 0xff], "payload.bin", None, true).is_err());
    }

    #[test]
    fn allowlisted_invalid_utf8_can_open_lossy_for_preview() {
        let (file, lossy) = decode_for_open(b"plain\xfftext", "notes.txt", None, true).unwrap();
        assert!(lossy);
        assert!(file.text.contains('\u{fffd}') || file.text.chars().any(|c| (c as u32) >= 0x80));
    }

    #[test]
    fn edit_path_rejects_lossy_only_content() {
        assert!(decode_for_open(b"plain\xfftext", "notes.txt", None, false).is_err());
    }
}
