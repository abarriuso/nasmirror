//! Reading the output of a Windows console process.
//!
//! robocopy writes in the console code page, and since it is launched with
//! `CREATE_NO_WINDOW` (no console to inherit) that is the system **OEM** code
//! page (e.g. 850 on a Spanish Windows), not UTF-8. Reading it as strict UTF-8
//! fails on the first non-ASCII character, which is common: the header
//! includes the localized date ("miércoles, 23 de septiembre…").

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

/// Decodes a line as UTF-8 if valid (console in code page 65001, or plain
/// ASCII), otherwise with the system OEM code page. Trying UTF-8 first avoids
/// mangling accented paths when the output really is UTF-8.
pub fn decode_line(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => decode_oem(bytes),
    }
}

#[cfg(windows)]
fn decode_oem(bytes: &[u8]) -> String {
    use windows::Win32::Globalization::{MultiByteToWideChar, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};
    /// `CP_OEMCP`: the system OEM code page.
    const CP_OEMCP: u32 = 1;
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let needed = unsafe { MultiByteToWideChar(CP_OEMCP, flags, bytes, None) };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    let written = unsafe { MultiByteToWideChar(CP_OEMCP, flags, bytes, Some(&mut wide)) };
    if written <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    String::from_utf16_lossy(&wide[..written as usize])
}

#[cfg(not(windows))]
fn decode_oem(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Like `tokio::io::Lines`, but decoding with `decode_line`.
///
/// `next_line` is cancel-safe like tokio's: if it is dropped midway (callers
/// use a short timeout to react to user cancellation), bytes already read are
/// kept and the next call continues the same line.
pub struct ConsoleLines<R> {
    reader: BufReader<R>,
    buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> ConsoleLines<R> {
    pub fn new(inner: R) -> Self {
        Self {
            reader: BufReader::new(inner),
            buf: Vec::new(),
        }
    }

    pub async fn next_line(&mut self) -> std::io::Result<Option<String>> {
        let read = self.reader.read_until(b'\n', &mut self.buf).await?;
        if read == 0 && self.buf.is_empty() {
            return Ok(None);
        }
        let line = decode_line(trim_eol(&self.buf));
        self.buf.clear();
        Ok(Some(line))
    }
}

fn trim_eol(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && (bytes[end - 1] == b'\n' || bytes[end - 1] == b'\r') {
        end -= 1;
    }
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_wins_when_valid() {
        assert_eq!(decode_line("miércoles".as_bytes()), "miércoles");
        assert_eq!(decode_line(b""), "");
    }

    #[cfg(windows)]
    #[test]
    fn falls_back_to_the_oem_codepage() {
        // 0x82 is "é" in CP850 and invalid as a lone UTF-8 byte.
        let decoded = decode_line(&[b'm', b'i', 0x82, b'r']);
        // What matters is that the line survives intact with no replacement
        // characters; the exact letter depends on the system OEM code page
        // (e.g. 850 or 437).
        assert!(decoded.starts_with("mi"));
        assert!(!decoded.contains('\u{fffd}'));
    }

    #[test]
    fn trims_crlf() {
        assert_eq!(trim_eol(b"hello\r\n"), b"hello");
        assert_eq!(trim_eol(b"hello"), b"hello");
    }

    #[tokio::test]
    async fn reads_lines_including_the_last_one_without_newline() {
        let data: &[u8] = b"one\r\ntwo\nthree";
        let mut lines = ConsoleLines::new(data);
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("one"));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("two"));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("three"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }
}
