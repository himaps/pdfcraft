//! pdfcraft-activation — the local activation marker.
//!
//! Not copy protection, and there is no online check: the code only marks that this
//! installation was registered with the author, who counts the machine codes that arrive and
//! answers each one with an activation code of the same length. The machine identifier never
//! leaves the machine unless the user sends the word code themselves, and the derivation below
//! ships inside the binary, so a determined user can compute an activation code without asking.
//! That is accepted by design (the requirement is a ledger, not a lock).
//!
//! The words exist so a code can be read out over a phone call: two-syllable Chinese words are
//! short and unambiguous where hex digits trip over pronunciation.
//!
//! Layout of a code: SHA-256 of the machine identifier (canonicalised) gives `digest`; the code
//! length is 4 + `digest[31] % 3` (4–6 words); the machine code takes `digest[i] & 63` as the
//! word index of position `i`. The activation code is the same length and derives from the
//! machine code the user sent — `sha256(KEY ++ machine_indices)[i] & 63` — because the author
//! never sees the machine identifier, only its words.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::PathBuf;

/// The 64 activation words, by index. Given by the author; do not reorder.
pub const WORDS: [&str; 64] = [
    "富强", "民主", "文明", "和谐", "自由", "平等", "公正", "法治", "爱国", "敬业", "诚信", "友善", "昌隆", "安泰", "开化", "盛平", "兴邦", "民本",
    "崇文", "和睦", "自主", "均平", "公允", "遵法", "忠邦", "恪职", "立信", "亲睦", "弘道", "笃行", "崇信", "尚和", "守正", "公义", "平允", "依规",
    "严明", "循法", "明德", "持公", "普惠", "安民", "固本", "维新", "合众", "宽和", "明断", "守矩", "崇德", "厚善", "守诚", "敬责", "报国", "敦睦",
    "慎行", "守信", "向善", "弘毅", "克己", "怀仁", "崇礼", "持节", "谦恭", "淳良",
];

/// Separates the words of a code, in both directions.
pub const SEPARATOR: char = '-';

/// The response key. Embedded in the shipped binary, so it is an obfuscation, not a secret: it
/// only makes the activation code non-obvious to someone who has not read past this comment.
const KEY: &[u8] = b"PdfCraft Kylin activation ledger key 1 (a marker, not a secret)";

#[derive(Debug, thiserror::Error)]
pub enum ActivationError {
    #[error("cannot read a hardware identifier on this machine (tried {0})")]
    NoMachineId(String),
    #[error("reading the hardware identifier: {0}")]
    Io(String),
    #[error("cannot write {0}: {1}")]
    Write(String, String),
    #[error("{word:?} is not one of the {total} activation words")]
    BadWord { word: String, total: usize },
    #[error("a code has {expected} words, this one has {got}")]
    WrongLength { expected: usize, got: usize },
    #[error("the activation code does not match this machine")]
    Mismatch,
    #[error("no activation is stored")]
    NotActivated,
}

/// A code: one word index per position, each in `0..64`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Code {
    pub indices: Vec<u8>,
}

impl Code {
    /// The words joined with [`SEPARATOR`], in the form users read and type.
    pub fn render(&self) -> String {
        self.indices.iter().map(|&i| word(i)).collect::<Vec<_>>().join(&SEPARATOR.to_string())
    }

    /// The number of words, which both codes of one machine share.
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// The word at `index` (mod 64, so a raw hash byte is a valid index).
pub fn word(index: u8) -> &'static str {
    WORDS[index as usize % WORDS.len()]
}

/// The index of `word`, or [`ActivationError::BadWord`].
pub fn index(word: &str) -> Result<u8, ActivationError> {
    WORDS.iter().position(|w| *w == word).map(|i| i as u8).ok_or_else(|| ActivationError::BadWord { word: word.to_owned(), total: WORDS.len() })
}

/// Parses a typed or transmitted code: words separated by [`SEPARATOR`] (whitespace around a
/// word is kept out), each of them one of [`WORDS`].
pub fn parse(text: &str) -> Result<Code, ActivationError> {
    let indices = text
        .split(SEPARATOR)
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .map(index)
        .collect::<Result<Vec<_>, _>>()?;
    if indices.is_empty() {
        return Err(ActivationError::BadWord { word: text.trim().to_owned(), total: WORDS.len() });
    }
    Ok(Code { indices })
}

/// The machine identifier the codes are derived from. The name says CPU serial, but processor
/// serial numbers have been disabled since the Pentium III, so this is the first stable
/// hardware identifier the OS exposes: on Windows the installation's MachineGuid, on Linux the
/// DMI product UUID or board serial, falling back to `/etc/machine-id` (per installation, and
/// what the activation actually marks: this installation on this machine).
pub fn machine_id() -> Result<String, ActivationError> {
    machine_id_platform().map(|id| id.trim().to_lowercase())
}

#[cfg(target_os = "linux")]
fn machine_id_platform() -> Result<String, ActivationError> {
    const FILES: [&str; 4] =
        ["/sys/class/dmi/id/product_uuid", "/sys/class/dmi/id/board_serial", "/etc/machine-id", "/var/lib/dbus/machine-id"];
    let found = FILES.iter().find_map(|f| std::fs::read_to_string(f).ok()).filter(|s| !s.trim().is_empty());
    found.ok_or_else(|| ActivationError::NoMachineId(FILES.join(", ")))
}

#[cfg(target_os = "windows")]
fn machine_id_platform() -> Result<String, ActivationError> {
    use winsafe::{HKEY, RegistryValue, co::RRF};
    match HKEY::LOCAL_MACHINE.RegGetValue(Some(r"SOFTWARE\Microsoft\Cryptography"), Some("MachineGuid"), RRF::RT_REG_SZ) {
        Ok(RegistryValue::Sz(v)) if !v.trim().is_empty() => Ok(v),
        _ => Err(ActivationError::NoMachineId(r"HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid".to_owned())),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn machine_id_platform() -> Result<String, ActivationError> {
    Err(ActivationError::NoMachineId("this platform".to_owned()))
}

/// The digest both codes derive from: the canonicalised machine identifier with a domain tag.
fn digest(machine_id: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(machine_id.trim().to_lowercase());
    h.update(b"\npdfcraft-activation-v1");
    h.finalize().to_vec()
}

/// How many words the codes for this machine have (4–6; the length itself is part of the
/// digest, so the two codes always agree on it).
fn count(digest: &[u8]) -> usize {
    4 + (digest[digest.len() - 1] % 3) as usize
}

/// The code this machine shows to its user, to be sent to the author.
pub fn machine_code(machine_id: &str) -> Code {
    let d = digest(machine_id);
    let n = count(&d);
    Code { indices: (0..n).map(|i| d[i] & 63).collect() }
}

/// The code the author answers with: same length as `machine`, derived from the machine code
/// the user sent through the response key — so the author, who never sees the machine
/// identifier, can compute it from the words alone.
pub fn activation_response(machine: &Code) -> Code {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(KEY);
    h.update(&machine.indices);
    let r = h.finalize();
    Code { indices: (0..machine.len()).map(|i| r[i] & 63).collect() }
}

/// The code the author answers with, for this machine.
pub fn activation_code(machine_id: &str) -> Code {
    activation_response(&machine_code(machine_id))
}

/// Where the activation is stored: beside the installed files (the install directory) —
/// `<exe>/../share/pdfcraft/activation.txt` on Linux (FHS `bin/` → `share/`, like the OCR
/// models), `<exe>/pdfcraft-activation.txt` on Windows. `None` without an executable path.
pub fn storage_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    Some(if cfg!(target_os = "linux") { dir.join("../share/pdfcraft/activation.txt") } else { dir.join("pdfcraft-activation.txt") })
}

/// Whether `path` holds the activation code of this machine.
pub fn status_at(path: &PathBuf, machine_id: &str) -> Result<Code, ActivationError> {
    let stored = parse(&std::fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ActivationError::NotActivated
        } else {
            ActivationError::Io(e.to_string())
        }
    })?)?;
    let expected = activation_code(machine_id);
    if stored.len() != expected.len() {
        return Err(ActivationError::WrongLength { expected: expected.len(), got: stored.len() });
    }
    if stored.indices != expected.indices {
        return Err(ActivationError::Mismatch);
    }
    Ok(stored)
}

/// Whether this installation is activated (see [`storage_path`]).
pub fn status(machine_id: &str) -> Result<Code, ActivationError> {
    match storage_path() {
        Some(p) => status_at(&p, machine_id),
        None => Err(ActivationError::NoMachineId("no executable path".to_owned())),
    }
}

/// Verifies the typed activation code against this machine and stores it. `at` is
/// [`storage_path`]; passing it in keeps this testable.
pub fn activate_at(at: &PathBuf, machine_id: &str, typed: &str) -> Result<Code, ActivationError> {
    let typed = parse(typed)?;
    let expected = activation_code(machine_id);
    if typed.len() != expected.len() {
        return Err(ActivationError::WrongLength { expected: expected.len(), got: typed.len() });
    }
    if typed.indices != expected.indices {
        return Err(ActivationError::Mismatch);
    }
    std::fs::write(at, typed.render()).map_err(|e| ActivationError::Write(at.display().to_string(), e.to_string()))?;
    Ok(typed)
}

/// Verifies and stores the activation (see [`storage_path`]).
pub fn activate(machine_id: &str, typed: &str) -> Result<Code, ActivationError> {
    match storage_path() {
        Some(p) => activate_at(&p, machine_id, typed),
        None => Err(ActivationError::NoMachineId("no executable path".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_table_is_the_given_one() {
        assert_eq!(WORDS.len(), 64);
        assert_eq!(WORDS[0], "富强");
        assert_eq!(WORDS[63], "淳良");
        assert_eq!(WORDS.iter().collect::<std::collections::BTreeSet<_>>().len(), 64, "the words are distinct");
        assert!(WORDS.iter().all(|w| w.chars().count() == 2), "two characters each, so codes read evenly");
    }

    #[test]
    fn codes_render_and_parse_round_trip() {
        let code = Code { indices: vec![0, 26, 63, 7] };
        let text = code.render();
        assert_eq!(text, "富强-立信-淳良-法治");
        assert_eq!(parse(&text).unwrap(), code);
        // Whitespace around a word is typing, not part of it.
        assert_eq!(parse(" 富强 - 立信 - 淳良 - 法治 ").unwrap(), code);
    }

    #[test]
    fn unknown_words_and_empty_input_are_rejected() {
        assert!(matches!(parse("富强-民主-文明-和谐-X"), Err(ActivationError::BadWord { .. })));
        assert!(matches!(parse("hello"), Err(ActivationError::BadWord { .. })));
        assert!(matches!(parse("   "), Err(ActivationError::BadWord { .. })));
        assert!(matches!(parse("-"), Err(ActivationError::BadWord { .. })));
    }

    /// Vectors produced by the same derivation in Python (hashlib), so a future refactor of the
    /// hashing is a deliberate change, not an accident. The activation code derives from the
    /// machine *words* (what the author receives), not from the machine identifier.
    #[test]
    fn known_machine_ids_have_known_codes() {
        let cases: [(&str, usize, [u8; 6], [u8; 6]); 3] = [
            ("test-machine-001", 6, [56, 28, 30, 2, 3, 23], [61, 40, 4, 55, 45, 46]),
            ("KYLIN-V20-Desktop", 4, [31, 28, 47, 16], [19, 28, 49, 21]),
            ("linux-abc123", 5, [8, 0, 5, 52, 51], [32, 36, 4, 21, 18]),
        ];
        for (id, n, user, act) in cases {
            let machine = machine_code(id);
            assert_eq!(machine.len(), n, "{id}");
            assert_eq!(machine.indices, user[..n], "{id}");
            // From the identifier (the app's view)…
            let activation = activation_code(id);
            assert_eq!(activation.len(), n, "{id}");
            assert_eq!(activation.indices, act[..n], "{id}");
            // …and from the words alone (the author's view): both agree.
            assert_eq!(activation_response(&parse(&machine.render()).unwrap()), activation, "{id}");
            assert_eq!(parse(&activation.render()).unwrap(), activation, "{id}");
        }
    }

    /// Both codes of one machine always have the same length, somewhere in 4–6.
    #[test]
    fn code_lengths_are_four_to_six_and_agree() {
        for n in 0..200u64 {
            let id = format!("machine-{n}");
            let machine = machine_code(&id);
            let activation = activation_code(&id);
            assert_eq!(machine.len(), activation.len());
            assert!((4..=6).contains(&machine.len()), "{id}: {}", machine.len());
        }
    }

    #[test]
    fn activation_verifies_and_a_wrong_code_does_not() {
        let dir = std::env::temp_dir().join(format!("pdfcraft-activation-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let at = dir.join("activation.txt");
        let id = "test-machine-001";
        assert!(matches!(status_at(&at, id), Err(ActivationError::NotActivated)));
        // The right activation code (typed as a user would: with stray spaces) is accepted and
        // stored.
        let typed = activation_code(id).render();
        activate_at(&at, id, &format!("  {typed}  ")).unwrap();
        assert!(status_at(&at, id).is_ok());
        // Another machine's activation code has the same shape and is rejected.
        let other = activation_code("test-machine-002").render();
        assert!(matches!(activate_at(&at, id, &other), Err(ActivationError::Mismatch)));
        // A code of a different length is a length error before it is a mismatch.
        let long = format!("{typed}-{typed}");
        assert!(matches!(activate_at(&at, id, &long), Err(ActivationError::WrongLength { expected: 6, got: 12 })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn storage_lands_beside_the_installed_files() {
        let p = storage_path().unwrap_or_default();
        if cfg!(target_os = "linux") {
            assert!(p.to_string_lossy().contains("share/pdfcraft/activation.txt"), "{p:?}");
        } else if cfg!(target_os = "windows") {
            assert_eq!(p.file_name().map(|s| s.to_string_lossy().to_string()).as_deref(), Some("pdfcraft-activation.txt"));
        }
    }
}
