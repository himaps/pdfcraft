//! Chinese recognition through the system's tesseract (Apache-2.0, run out of process).
//!
//! The bundled ocrs models read the Latin alphabet only. Chinese recognition needs different
//! models, and the practical source of them is the distro's tesseract: `tesseract-ocr` plus
//! `tesseract-ocr-chi-sim` bring both the recogniser and the trained data, sit in every
//! Debian/Ubuntu/Kylin archive (so the offline deb can carry them), and keep the app itself
//! pure Rust — the recognition is an external program's problem. `chi_sim` reads Simplified
//! Chinese with ASCII and common punctuation, which is the mixed Chinese/English case.

// The web build has no processes: Chinese recognition stays a desktop capability, and the pure
// helpers (TSV parsing, the BMP writer) would be dead code there.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

#[cfg(not(target_arch = "wasm32"))]
use std::process::Command;

use super::{Line, OcrError, Word};

/// The tesseract language pack for Simplified Chinese (its model also covers ASCII).
pub const CHINESE_LANG: &str = "chi_sim";

/// Whether the language is one the tesseract backend reads (the bundled ocrs models cover the
/// rest): `zh-hans` from [`super::LANGUAGES`], or a raw tesseract pack name.
pub fn is_chinese(language: &str) -> bool {
    let l = language.trim().to_ascii_lowercase();
    l.starts_with("zh") || l.starts_with("chi_sim")
}

/// The installed tesseract's version line, or `None` when it is not on `PATH`.
#[cfg(not(target_arch = "wasm32"))]
pub fn version() -> Option<String> {
    let out = Command::new("tesseract").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .chain(String::from_utf8_lossy(&out.stderr).lines())
        .map(str::to_owned)
        .find(|l| !l.trim().is_empty())
}

#[cfg(target_arch = "wasm32")]
pub fn version() -> Option<String> {
    None
}

/// Recognise the text in an RGBA (or RGB, or grey) image with the system tesseract. `lang` is
/// a tesseract language string (`chi_sim`, `chi_sim+eng`, …).
#[cfg(not(target_arch = "wasm32"))]
pub fn recognize(pixels: &[u8], width: u32, height: u32, lang: &str) -> Result<Vec<Line>, OcrError> {
    recognize_with("tesseract", pixels, width, height, lang)
}

/// The web build cannot run tesseract; the caller reports it like any other recognition error.
#[cfg(target_arch = "wasm32")]
pub fn recognize(pixels: &[u8], width: u32, height: u32, lang: &str) -> Result<Vec<Line>, OcrError> {
    let _ = (pixels, width, height, lang);
    Err(OcrError::Recognize(
        "Chinese recognition runs the desktop's tesseract and is not part of the web build".to_owned(),
    ))
}

/// [`recognize`], with the program named explicitly (tests run a fake).
#[cfg(not(target_arch = "wasm32"))]
pub fn recognize_with(
    program: &str,
    pixels: &[u8],
    width: u32,
    height: u32,
    lang: &str,
) -> Result<Vec<Line>, OcrError> {
    if width == 0 || height == 0 || pixels.is_empty() {
        return Err(OcrError::EmptyImage);
    }
    let Some(bmp) = bitmap(pixels, width, height) else {
        return Err(OcrError::Recognize(format!("unsupported image: {width}×{height}, {} bytes", pixels.len())));
    };
    // tesseract reads files, so the image goes to a temporary BMP (written by hand: one more
    // image codec in the dependency tree would be the only reason this module has deps).
    let input = std::env::temp_dir().join(format!("pdfcraft-ocr-{}-{:?}.bmp", std::process::id(), std::time::Instant::now()));
    std::fs::write(&input, &bmp).map_err(|e| OcrError::Recognize(e.to_string()))?;
    let run = Command::new(program)
        .arg(&input)
        .arg("stdout")
        .args(["-l", lang, "--psm", "3", "tsv"])
        .output();
    let _ = std::fs::remove_file(&input);
    let out = run.map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => OcrError::TesseractMissing,
        _ => OcrError::Recognize(e.to_string()),
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lower = stderr.to_lowercase();
        if lower.contains("failed loading language") || lower.contains("read_params_file") || lower.contains("error openning data") {
            return Err(OcrError::TesseractData(lang.to_owned()));
        }
        return Err(OcrError::Recognize(stderr.trim().to_owned()));
    }
    Ok(parse_tsv(&String::from_utf8_lossy(&out.stdout)))
}

/// A 24-bit BMP of the image (tesseract reads BMP without help), bottom-up rows padded to four
/// bytes. `None` when `pixels` does not match the size as 4-, 3- or 1-byte pixels.
fn bitmap(pixels: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    let channels = match pixels.len() {
        n if n == w * h * 4 => 4usize,
        n if n == w * h * 3 => 3,
        n if n == w * h => 1,
        _ => return None,
    };
    if w == 0 || h == 0 || w > i32::MAX as usize || h > i32::MAX as usize {
        return None;
    }
    let row = (w * 3).div_ceil(4) * 4;
    let size = 54 + row * h;
    let mut out = vec![0u8; size];
    out[0..2].copy_from_slice(b"BM");
    out[2..6].copy_from_slice(&(size as u32).to_le_bytes());
    out[10..14].copy_from_slice(&54u32.to_le_bytes());
    out[14..18].copy_from_slice(&40u32.to_le_bytes());
    out[18..22].copy_from_slice(&(w as u32).to_le_bytes());
    out[22..26].copy_from_slice(&(h as u32).to_le_bytes());
    out[26..28].copy_from_slice(&1u16.to_le_bytes());
    out[28..30].copy_from_slice(&24u16.to_le_bytes());
    out[34..38].copy_from_slice(&((row * h) as u32).to_le_bytes());
    // Bottom-up rows, BGR, 3 bytes per pixel, grey expanded to three channels.
    for y in 0..h {
        let dst = 54 + (h - 1 - y) * row;
        for x in 0..w {
            let src = (y * w + x) * channels;
            let (r, g, b) = if channels == 1 { (pixels[src], pixels[src], pixels[src]) } else { (pixels[src], pixels[src + 1], pixels[src + 2]) };
            out[dst + x * 3..dst + x * 3 + 3].copy_from_slice(&[b, g, r]);
        }
    }
    Some(out)
}

/// The words and their boxes out of tesseract's TSV (`tesseract in stdout … tsv`): one header
/// row, then rows per level, level 5 being a word with its box and confidence. Rows of one text
/// line are adjacent and share block/paragraph/line numbers, which group the words; the rows
/// arrive in reading order and keep it.
pub fn parse_tsv(tsv: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = Vec::new();
    let mut group: Option<(String, [f32; 4])> = None;
    for row in tsv.lines().skip(1) {
        let cols: Vec<&str> = row.split('\t').collect();
        if cols.len() < 12 {
            continue;
        }
        // level page block par line word left top width height conf text
        if cols[0] != "5" {
            continue;
        }
        let text = cols[11].trim();
        let conf: f32 = cols[10].trim().parse().unwrap_or(-1.0);
        let (left, top): (f32, f32) = (cols[6].trim().parse().unwrap_or(0.0), cols[7].trim().parse().unwrap_or(0.0));
        let (wide, tall): (f32, f32) = (cols[8].trim().parse().unwrap_or(0.0), cols[9].trim().parse().unwrap_or(0.0));
        if text.is_empty() || conf < 0.0 {
            continue;
        }
        let key = format!("{}\t{}\t{}", cols[2], cols[3], cols[4]);
        if group.as_ref().map(|(k, _)| k != &key).unwrap_or(true) {
            lines.push(Line::default());
            group = Some((key, [left, top, left + wide, top + tall]));
        }
        if let Some((_, box_)) = group.as_mut() {
            // The line's box grows around its words, so a line is one rect like ocrs' are.
            box_[0] = box_[0].min(left);
            box_[1] = box_[1].min(top);
            box_[2] = box_[2].max(left + wide);
            box_[3] = box_[3].max(top + tall);
        }
        let rect = [left, top, left + wide, top + tall];
        if let Some(line) = lines.last_mut() {
            line.words.push(Word { text: text.to_owned(), rect });
        }
    }
    lines.retain(|l| !l.words.is_empty());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One header row, two text lines of two words each, a low-confidence row skipped, and a
    /// stray level-3 row ignored. Boxes in tesseract's left/top/width/height form.
    const SAMPLE: &str = "level\tpage\tblock\tpar\tline\tword\tleft\ttop\twidth\theight\tconf\ttext
1\t1\t0\t0\t0\t-1\t0\t0\t640\t480\t-1\t
5\t1\t1\t1\t1\t1\t10\t20\t40\t14\t96.5\t你好
5\t1\t1\t1\t1\t2\t60\t20\t38\t14\t95.0\t世界
5\t1\t2\t1\t2\t1\t12\t60\t90\t14\t91.2\tHello
5\t1\t2\t1\t2\t2\t110\t60\t30\t14\t-1\t
5\t1\t2\t1\t2\t3\t112\t60\t26\t14\t88.0\tworld
";

    #[test]
    fn tsv_rows_group_into_lines_in_reading_order() {
        let lines = parse_tsv(SAMPLE);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].text(), "你好 世界");
        assert_eq!(lines[1].text(), "Hello world");
        // A line's box spans its words.
        assert_eq!(lines[0].words[0].rect, [10.0, 20.0, 50.0, 34.0]);
        assert_eq!(lines[1].words[0].rect, [12.0, 60.0, 102.0, 74.0]);
        // The low-confidence word did not survive.
        assert_eq!(lines[1].words.len(), 2);
    }

    #[test]
    fn empty_and_broken_tsv_read_as_no_text() {
        assert!(parse_tsv("").is_empty());
        assert!(parse_tsv("level\tpage\tblock\tpar\tline\tword\tleft\ttop\twidth\theight\tconf\ttext\n").is_empty());
        assert!(parse_tsv("not a tsv at all\njust one column\n").is_empty());
    }

    #[test]
    fn the_bitmap_is_a_valid_24_bit_bmp() {
        // 2×2 RGBA, four distinct channel values, so a channel mix-up cannot pass.
        let px = [1u8, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255];
        let bmp = bitmap(&px, 2, 2).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize, bmp.len());
        assert_eq!(u16::from_le_bytes(bmp[26..28].try_into().unwrap()), 1, "planes");
        assert_eq!(u16::from_le_bytes(bmp[28..30].try_into().unwrap()), 24, "bits per pixel");
        // Bottom-up: the last BMP row holds the image's first row; BGR order.
        assert_eq!(&bmp[54..57], &[3, 2, 1]);
        assert_eq!(&bmp[57..60], &[6, 5, 4]);
        assert_eq!(&bmp[60 + 4..60 + 4 + 3], &[9, 8, 7]);
        // RGB (3 bytes per pixel) and grey read as the same image.
        let rgb: Vec<u8> = px.chunks(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
        assert_eq!(bitmap(&rgb, 2, 2).unwrap()[54..60], bmp[54..60]);
        let grey = [128u8, 130, 132, 134];
        let g = bitmap(&grey, 2, 2).unwrap();
        assert_eq!(&g[54..57], &[128, 128, 128]);
        assert_eq!(&g[60 + 4..60 + 4 + 3], &[132, 132, 132]);
    }

    #[test]
    fn a_pixel_buffer_that_does_not_match_the_size_is_rejected() {
        let px = vec![255u8; 2 * 2 * 4];
        assert!(bitmap(&px[..px.len() - 3], 2, 2).is_none());
        assert!(recognize_with("tesseract-definitely-not-here", &px, 2, 2, CHINESE_LANG).is_err());
    }

    /// End to end against a fake `tesseract` that prints the sample TSV, which is everything
    /// the backend does besides write a file (unix only: the fake is a shell script).
    #[test]
    #[cfg(unix)]
    fn recognize_runs_the_program_and_parses_its_tsv() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-tesseract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("tesseract");
        std::fs::write(&fake, format!("#!/bin/sh\ncat <<'TSV'\n{SAMPLE}TSV\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let px = vec![255u8; 4 * 4 * 4];
        let lines = recognize_with(fake.to_str().unwrap(), &px, 4, 4, CHINESE_LANG).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "你好 世界");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
