//! Where the app keeps its files.

use std::path::PathBuf;

const FOLDER: &str = "Jelly Voice and Notes";

/// The app's own folder: the database and the log. `JELLY_DATA_DIR`
/// points it elsewhere, for trying the app without touching your notes.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("JELLY_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join(FOLDER)
}

pub fn db_path() -> PathBuf {
    data_dir().join("notes.sqlite")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Where "Save the notes as a file" writes: a folder in Downloads, so the
/// files are easy to find.
pub fn export_dir() -> PathBuf {
    dirs::download_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir).join("Jelly notes")
}
