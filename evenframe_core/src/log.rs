//! File-based debug logging for working on evenframe itself.
//!
//! Set `EVENFRAME_LOG_DIR` to a directory and `evenframe_log!` writes each
//! entry to a file there. Unset, nothing is logged and the macro's arguments
//! are never evaluated.

use std::path::{Path, PathBuf};

/// The variable naming the directory `evenframe_log!` writes to.
pub const LOG_DIR_VAR: &str = "EVENFRAME_LOG_DIR";

/// The directory `evenframe_log!` writes to, when `EVENFRAME_LOG_DIR` names one.
#[doc(hidden)]
pub fn log_dir() -> Option<PathBuf> {
    std::env::var_os(LOG_DIR_VAR)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// A log file name stamped with the current local time.
#[doc(hidden)]
pub fn timestamped_filename() -> String {
    format!("{}.log", chrono::Local::now().format("%Y_%m_%d_%H_%M_%S"))
}

/// Write one `evenframe_log!` entry to `filename` in `dir`. Debug logging
/// must not fail the run, so a failure is reported as a warning instead.
#[doc(hidden)]
pub fn write_entry(dir: &Path, filename: &str, append: bool, entry: &str) {
    use std::io::Write;

    if let Err(error) = std::fs::create_dir_all(dir) {
        tracing::warn!(dir = %dir.display(), %error, "evenframe_log could not create its directory");
        return;
    }
    let path = dir.join(filename);
    let mut options = std::fs::OpenOptions::new();
    options.create(true);
    if append {
        options.append(true);
    } else {
        options.write(true).truncate(true);
    }
    if let Err(error) = options
        .open(&path)
        .and_then(|mut file| file.write_all(entry.as_bytes()))
    {
        tracing::warn!(path = %path.display(), %error, "evenframe_log could not write");
    }
}

/// Render `$content` as a log entry and write it to `$filename` in `$dir`.
#[macro_export]
#[doc(hidden)]
macro_rules! __internal_log_impl {
    ($dir:expr, $content:expr, $filename:expr, $append:expr) => {{
        let filename: &str = &$filename;
        let expr_str = stringify!($content);
        let entry = if expr_str.starts_with("format!")
            || expr_str.starts_with("&format!")
            || expr_str.starts_with("\"")
            || expr_str.starts_with("String::")
            || filename.ends_with(".surql")
        {
            format!("{}\n", $content)
        } else {
            let value_str = format!("{:#?}", &$content);
            let separator = if value_str.contains('\n') || value_str.len() > 80 {
                " = \n"
            } else {
                " = "
            };
            format!(
                "[{}:{}] {}{}{}\n",
                file!(),
                line!(),
                stringify!($content),
                separator,
                value_str
            )
        };
        $crate::log::write_entry(&$dir, filename, $append, &entry);
    }};
}

/// `evenframe_log!` against an explicit directory: `None` logs nothing and
/// evaluates nothing.
#[macro_export]
#[doc(hidden)]
macro_rules! __evenframe_log_to {
    ($dir:expr, $content:expr) => {{
        if let Some(dir) = $dir {
            let filename = $crate::log::timestamped_filename();
            $crate::__internal_log_impl!(dir, $content, filename, false);
        }
    }};
    ($dir:expr, $content:expr, $filename:expr) => {{
        if let Some(dir) = $dir {
            $crate::__internal_log_impl!(dir, $content, $filename, false);
        }
    }};
    ($dir:expr, $content:expr, $filename:expr, $append:expr) => {{
        if let Some(dir) = $dir {
            $crate::__internal_log_impl!(dir, $content, $filename, $append);
        }
    }};
}

/// File-based debug logging, active while `EVENFRAME_LOG_DIR` is set.
///
/// # Examples
///
/// ```no_run
/// # use evenframe_core::evenframe_log;
/// evenframe_log!("Sync started");
/// evenframe_log!("Types generated", "output.log");
/// evenframe_log!("New type added", "output.log", true);
/// ```
#[macro_export]
macro_rules! evenframe_log {
    ($content:expr) => {
        $crate::__evenframe_log_to!($crate::log::log_dir(), $content)
    };
    ($content:expr, $filename:expr) => {
        $crate::__evenframe_log_to!($crate::log::log_dir(), $content, $filename)
    };
    ($content:expr, $filename:expr, $append:expr) => {
        $crate::__evenframe_log_to!($crate::log::log_dir(), $content, $filename, $append)
    };
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::path::PathBuf;

    #[test]
    fn nothing_is_evaluated_or_written_without_a_log_directory() {
        let evaluations = Cell::new(0);
        let render = || {
            evaluations.set(evaluations.get() + 1);
            String::from("rendered")
        };
        let none: Option<PathBuf> = None;
        crate::__evenframe_log_to!(none.clone(), render());
        crate::__evenframe_log_to!(none.clone(), render(), "file.log");
        crate::__evenframe_log_to!(none, render(), "file.log", true);
        assert_eq!(evaluations.get(), 0);
    }

    #[test]
    fn entries_are_written_and_appended_in_the_log_directory() {
        let dir = tempfile::tempdir().unwrap();
        let logs = dir.path().join("nested");
        crate::__evenframe_log_to!(Some(logs.clone()), String::from("first"), "run.log");
        crate::__evenframe_log_to!(Some(logs.clone()), String::from("second"), "run.log", true);
        assert_eq!(
            std::fs::read_to_string(logs.join("run.log")).unwrap(),
            "first\nsecond\n"
        );
        crate::__evenframe_log_to!(Some(logs.clone()), String::from("fresh"), "run.log");
        assert_eq!(
            std::fs::read_to_string(logs.join("run.log")).unwrap(),
            "fresh\n"
        );
    }
}
