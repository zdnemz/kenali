#![forbid(unsafe_code)]

//! Model fetch and verification. Uses `models/models.lock` as the single
//! source of truth: each row lists name,url,sha256,licence,upstream_commit.
//! rejected. The `Fetch` and `Verify` actions are dispatched from main.rs;
//! `fetch` downloads, `verify` recomputes SHA-256 and rejects mismatches
//! BEFORE any model is used.

use anyhow::{Context, bail};
use std::fs;
use std::path::Path;

/// Entry point dispatched by main.rs. Exactly one of `fetch`/`verify` is true.
pub fn models(fetch: bool, verify: bool) -> anyhow::Result<()> {
    if !fetch && !verify {
        bail!("models: pass `fetch` or `verify`");
    }
    if fetch && verify {
        bail!("models: `fetch` and `verify` are mutually exclusive");
    }

    let lock_path = Path::new("models").join("models.lock");
    let lock_text = fs::read_to_string(&lock_path)
        .with_context(|| format!("reading {}", lock_path.display()))?;
    let rows = parse_lock(&lock_text)?;
    if rows.is_empty() {
        bail!("models.lock contains no asset rows");
    }

    if fetch {
        fetch_assets(&rows)
    } else {
        verify_assets(&rows)
    }
}

/// A single parsed row of models.lock.
#[derive(Debug, Clone)]
struct ModelRow {
    name: String,
    url: String,
    sha256: String,
    #[allow(dead_code)]
    licence: String,
    #[allow(dead_code)]
    upstream_commit: String,
}

/// Parse the CSV-like lock manifest. Blank lines and `#` comments are ignored.
/// The header row starting with `name,` is skipped.
fn parse_lock(text: &str) -> anyhow::Result<Vec<ModelRow>> {
    let mut rows = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("name,") {
            continue;
        }
        let columns: Vec<&str> = trimmed.split(',').collect();
        if columns.len() < 5 {
            bail!("models.lock line {}: expected 5 columns, got {}", line_no + 1, columns.len());
        }
        rows.push(ModelRow {
            name: columns[0].trim().to_owned(),
            url: columns[1].trim().to_owned(),
            sha256: columns[2].trim().to_owned(),
            licence: columns[3].trim().to_owned(),
            upstream_commit: columns[4].trim().to_owned(),
        });
    }
    Ok(rows)
}

/// True when the lock cell is an unfilled VERIFY placeholder.
fn is_placeholder(digest: &str) -> bool {
    digest.is_empty() || digest.starts_with("VERIFY")
}

/// Fetch action (`fn fetch` in the command contract) — download assets.
fn fetch_assets(rows: &[ModelRow]) -> anyhow::Result<()> {
    println!("xtask models fetch — {} asset(s) from models.lock", rows.len());
    let dir = Path::new("models");
    if !dir.is_dir() {
        fs::create_dir_all(dir)
            .with_context(|| format!("creating {}", dir.display()))?;
    }
    for row in rows {
        let dest = dir.join(&row.name);
        if is_placeholder(&row.sha256) {
            println!(
                "  {}: sha256 is a VERIFY placeholder; downloading but verify will refuse to use it",
                row.name
            );
        }
        println!("  -> curl -fL -o {} {}", dest.display(), row.url);
        let status = std::process::Command::new("curl")
            .arg("-fL")
            .arg("-o")
            .arg(&dest)
            .arg(&row.url)
            .status()
            .with_context(|| format!("spawning curl for {}", row.name))?;
        if !status.success() {
            bail!("fetch of {} failed with status {status}", row.name);
        }
    }
    println!("xtask models fetch: complete (run `verify` before use)");
    Ok(())
}

/// Verify action (`fn verify` in the command contract) — recompute SHA-256.
fn verify_assets(rows: &[ModelRow]) -> anyhow::Result<()> {
    println!("xtask models verify — {} asset(s) from models.lock", rows.len());
    let mut failures = 0usize;
    for row in rows {
        let path = Path::new("models").join(&row.name);
        if !path.is_file() {
            println!("  {}: MISSING ({})", row.name, path.display());
            failures += 1;
            continue;
        }
        if is_placeholder(&row.sha256) {
            println!(
                "  {}: REFUSED — models.lock sha256 is an unfilled VERIFY placeholder",
                row.name
            );
            failures += 1;
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let actual = sha256_hex(&bytes);
        let expected = row.sha256.trim().to_ascii_lowercase();
        if actual == expected {
            println!("  {}: OK ({})", row.name, actual);
        } else {
            println!(
                "  {}: MISMATCH — expected {expected}, got {actual}",
                row.name
            );
            failures += 1;
        }
    }
    if failures > 0 {
        bail!("models verify: {failures} asset(s) rejected; do not use any model");
    }
    println!("xtask models verify: all assets verified");
    Ok(())
}

// ---------------------------------------------------------------------------
// Minimal pure-Rust SHA-256 (FIPS 180-4). Kept local so xtask needs no extra
// dependency; identical digest to sha2::Sha256. No unsafe, no panic paths.
// ---------------------------------------------------------------------------

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

fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        let j = i * 4;
        w[i] = u32::from_be_bytes([block[j], block[j + 1], block[j + 2], block[j + 3]]);
    }
    for i in 16..64 {
        let s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
        let s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];
    for i in 0..64 {
        let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Compute the lowercase hex SHA-256 digest of `data`.
fn sha256_hex(data: &[u8]) -> String {
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = data.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in padded.chunks_exact(64) {
        let mut block = [0u8; 64];
        block.copy_from_slice(chunk);
        compress(&mut state, &block);
    }
    let mut out = String::with_capacity(64);
    for word in state {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parse_lock_skips_comments_and_header() {
        let text = "# comment\nname,url,sha256,licence,upstream_commit\na.onnx,u,deadbeef,MIT,c\n";
        let rows = parse_lock(text).unwrap_or_default();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "a.onnx");
    }
}
