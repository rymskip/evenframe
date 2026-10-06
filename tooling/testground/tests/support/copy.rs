//! Copies of the testground for the CLI to run on.

use std::fs;
use std::path::{Path, PathBuf};

/// Copies the testground's config, models and surql files into `to`.
pub fn copy_testground(to: &Path) {
    let testground = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::create_dir_all(to).unwrap();
    for entry in ["Cargo.toml", "evenframe.toml", "src", "surql"] {
        copy(&testground.join(entry), &to.join(entry));
    }
}

fn copy(from: &Path, to: &Path) {
    if from.is_dir() {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            copy(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        fs::copy(from, to).unwrap();
    }
}
