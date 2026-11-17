use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

const MAGIC: &[u8; 5] = b"MLSM\x01";

static KEY: LazyLock<[u8; 32]> = LazyLock::new(|| {
    let seed = fs::read_to_string("/etc/machine-id")
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|_| {
            format!(
                "{}:{}",
                std::env::var("HOSTNAME").unwrap_or_default(),
                std::env::var("USER").unwrap_or_default()
            )
        });
    let mut h = Sha256::new();
    h.update(b"melissm local store v1");
    h.update(seed.as_bytes());
    h.finalize().into()
});

static CIPHER: LazyLock<Aes256Gcm> = LazyLock::new(|| Aes256Gcm::new((&*KEY).into()));

pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let ct = CIPHER
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(MAGIC.len() + 12 + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 17 || &data[..5] != MAGIC {
        return None;
    }
    CIPHER
        .decrypt(Nonce::from_slice(&data[5..17]), &data[17..])
        .ok()
}

pub fn seal_file(path: &Path, plain: &[u8]) -> Result<(), String> {
    let sealed = seal(plain)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, sealed).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn open_file(path: &Path) -> Option<Vec<u8>> {
    open(&fs::read(path).ok()?)
}
