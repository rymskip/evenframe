/// Root directory for `evenframe_log!` output: `$ABSOLUTE_PATH_TO_EVENFRAME`
/// when set, otherwise the system temp directory. Logging never panics
/// because the variable is missing.
#[cfg(feature = "dev-mode")]
#[doc(hidden)]
pub fn log_root() -> std::path::PathBuf {
    std::env::var_os("ABSOLUTE_PATH_TO_EVENFRAME")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
}

/// A log file name stamped with the current local time.
#[cfg(feature = "dev-mode")]
#[doc(hidden)]
pub fn timestamped_filename() -> String {
    format!("{}.log", chrono::Local::now().format("%Y_%m_%d_%H_%M_%S"))
}

/// Write one `evenframe_log!` entry to `subdir/filename` under
/// [`log_root`]. Debug logging must not fail the run, so a failure is
/// reported as a warning instead.
#[cfg(feature = "dev-mode")]
#[doc(hidden)]
pub fn write_entry(subdir: &str, filename: &str, append: bool, entry: &str) {
    use std::io::Write;

    let logs_dir = log_root().join(subdir);
    if let Err(error) = std::fs::create_dir_all(&logs_dir) {
        tracing::warn!(dir = %logs_dir.display(), %error, "evenframe_log could not create its directory");
        return;
    }
    let path = logs_dir.join(filename);
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

#[cfg(feature = "dev-mode")]
#[macro_export]
#[doc(hidden)]
macro_rules! __internal_log_impl {
    ($content:expr, $log_subdir:expr, standard) => {{
        let filename = $crate::log::timestamped_filename();
        $crate::__internal_log_impl!($content, $log_subdir, filename, false, standard);
    }};

    ($content:expr, $log_subdir:expr, $filename:expr, standard) => {{
        $crate::__internal_log_impl!($content, $log_subdir, $filename, false, standard);
    }};

    ($content:expr, $log_subdir:expr, $filename:expr, $append:expr, standard) => {{
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
        $crate::log::write_entry($log_subdir, filename, $append, &entry);
    }};
}

/// File-based debug logging macro. Only active with the `dev-mode` feature.
///
/// # Examples
///
/// ```no_run
/// # use evenframe_core::evenframe_log;
/// evenframe_log!("Sync started");
/// evenframe_log!("Types generated", "output.log");
/// evenframe_log!("New type added", "output.log", true);
/// ```
#[cfg(feature = "dev-mode")]
#[macro_export]
macro_rules! evenframe_log {
    ($content:expr) => {{
        $crate::__internal_log_impl!($content, "evenframe/logs", standard);
    }};
    ($content:expr, $filename:expr) => {{
        $crate::__internal_log_impl!($content, "evenframe/logs", $filename, standard);
    }};
    ($content:expr, $filename:expr, $append:expr) => {{
        $crate::__internal_log_impl!($content, "evenframe/logs", $filename, $append, standard);
    }};
}

// The arguments are type-checked but never evaluated, so a `format!` passed
// in costs nothing outside dev mode.
#[cfg(not(feature = "dev-mode"))]
#[macro_export]
macro_rules! evenframe_log {
    ($content:expr) => {{
        if false {
            let _ = &$content;
        }
    }};
    ($content:expr, $filename:expr) => {{
        if false {
            let _ = (&$content, &$filename);
        }
    }};
    ($content:expr, $filename:expr, $append:expr) => {{
        if false {
            let _ = (&$content, &$filename, &$append);
        }
    }};
}

#[cfg(all(test, not(feature = "dev-mode")))]
mod tests {
    use std::cell::Cell;

    #[test]
    fn arguments_are_not_evaluated_outside_dev_mode() {
        let evaluations = Cell::new(0);
        let render = || {
            evaluations.set(evaluations.get() + 1);
            String::from("rendered")
        };
        crate::evenframe_log!(render());
        crate::evenframe_log!(render(), "file.log");
        crate::evenframe_log!(render(), "file.log", true);
        assert_eq!(evaluations.get(), 0);
    }
}
