//! Computes CRC32/MD5/SHA-1/SHA-256 checksums for a single file, from the
//! context menu's "Checksums" entry. Runs on a background thread (a large
//! file can take a while to hash) and reports back through a channel,
//! following the same "start an async job, poll it each frame" shape
//! `core::compress`'s `compress_paths_async` already uses.

use crc32fast::Hasher as Crc32Hasher;
use crossbeam_channel::Sender;
use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::Sha256;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Chunk size for the single streaming read that feeds all four hashers -
/// avoids loading a large file fully into memory.
const CHUNK_SIZE: usize = 1024 * 1024;

pub struct ChecksumResults {
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

pub fn compute_checksums(path: &Path) -> Result<ChecksumResults, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?;

    let mut crc32 = Crc32Hasher::new();
    let mut md5 = Md5::new();
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();

    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        crc32.update(chunk);
        md5.update(chunk);
        sha1.update(chunk);
        sha256.update(chunk);
    }

    Ok(ChecksumResults {
        crc32: format!("{:08x}", crc32.finalize()),
        md5: hex_lower(&md5.finalize()),
        sha1: hex_lower(&sha1.finalize()),
        sha256: hex_lower(&sha256.finalize()),
    })
}

pub fn compute_checksums_async(path: PathBuf, tx: Sender<Result<ChecksumResults, String>>) {
    std::thread::spawn(move || {
        let _ = tx.send(compute_checksums(&path));
    });
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn known_content_hashes_match_reference_values() {
        let dir = std::env::temp_dir().join("eden_explorer_checksum_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("known.txt");
        {
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(b"hello world").unwrap();
        }

        let results = compute_checksums(&path).unwrap();

        // Reference values for the ASCII bytes "hello world" (no trailing
        // newline), independently verifiable via `md5sum`/`sha1sum`/
        // `sha256sum`/any CRC32 calculator.
        assert_eq!(results.crc32, "0d4a1185");
        assert_eq!(results.md5, "5eb63bbbe01eeed093cb22bb8f5acdc3");
        assert_eq!(results.sha1, "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed");
        assert_eq!(
            results.sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_file_hashes_match_reference_values() {
        let dir = std::env::temp_dir().join("eden_explorer_checksum_test_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::File::create(&path).unwrap();

        let results = compute_checksums(&path).unwrap();

        assert_eq!(results.crc32, "00000000");
        assert_eq!(results.md5, "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(results.sha1, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            results.sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
