//! Writing the files: JSON in the two shapes the fixtures use, the directory
//! walk, and the manifest with its SHA-256 (written here: no hashing crate).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Pretty JSON with a final newline — the snapshot and report files.
pub fn pretty(value: &impl serde::Serialize) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("fixture values serialise");
    text.push('\n');
    text
}

/// One JSON Lines record: compact, no newline inside (JSON escapes every
/// control character in strings), one newline after.
pub fn jsonl(value: &Value) -> String {
    let mut text = serde_json::to_string(value).expect("fixture values serialise");
    debug_assert!(!text.contains('\n'));
    text.push('\n');
    text
}

/// Every file under `root`, relative to it, in byte order of the path — the
/// order the manifest lists them in. Directories are walked sorted, so the
/// answer does not depend on the file system's own order.
pub fn files_under(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    walk(root, Path::new(""), &mut found)?;
    found.sort_by(|a, b| a.as_os_str().as_encoded_bytes().cmp(b.as_os_str().as_encoded_bytes()));
    Ok(found)
}

fn walk(root: &Path, relative: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let dir = root.join(relative);
    let mut entries: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<Result<_, _>>()
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    entries.sort();
    for name in entries {
        let path = relative.join(&name);
        if root.join(&path).is_dir() {
            walk(root, &path, found)?;
        } else {
            found.push(path);
        }
    }
    Ok(())
}

/// `sha256sum`'s format: `<hex>  <path>`, one line per file under each of
/// `dirs` (relative to `root`), sorted by path. `sha256sum -c` checks it.
pub fn manifest(root: &Path, dirs: &[&str]) -> Result<String, String> {
    let mut lines = String::new();
    for dir in dirs {
        for file in files_under(&root.join(dir))? {
            let path = Path::new(dir).join(&file);
            let bytes = fs::read(root.join(&path)).map_err(|e| format!("{}: {e}", path.display()))?;
            let name = path.to_str().ok_or_else(|| format!("{} is not UTF-8", path.display()))?;
            lines.push_str(&format!("{}  {name}\n", hex(&sha256(&bytes))));
        }
    }
    Ok(lines)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 (FIPS 180-4), for the manifest only.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for chunk in message.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, add) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(add);
        }
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // Over one block, and a length that pads into a second.
        assert_eq!(hex(&sha256(&[b'a'; 1000])), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn jsonl_is_one_line_per_record() {
        let line = jsonl(&serde_json::json!({ "text": "two\nlines\r\tand a tab", "n": 1 }));
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        assert_eq!(serde_json::from_str::<Value>(line.trim_end()).unwrap()["text"], "two\nlines\r\tand a tab");
    }

    #[test]
    fn the_manifest_is_sorted_and_checkable() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("set/b.txt"), "b").unwrap();
        write(&dir.path().join("set/a/z.txt"), "z").unwrap();
        write(&dir.path().join("set/a.txt"), "a").unwrap();
        let text = manifest(dir.path(), &["set"]).unwrap();
        let names: Vec<&str> = text.lines().map(|line| line.split_once("  ").unwrap().1).collect();
        assert_eq!(names, vec!["set/a.txt", "set/a/z.txt", "set/b.txt"]);
        assert!(text.starts_with(&format!("{}  set/a.txt\n", hex(&sha256(b"a")))));
        assert_eq!(pretty(&serde_json::json!([1])), "[\n  1\n]\n");
    }
}
