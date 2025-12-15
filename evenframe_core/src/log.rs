/// Internal macro for common logging logic
#[macro_export]
#[doc(hidden)]
macro_rules! __internal_log_impl {
    // Standard variant - uses ABSOLUTE_PATH_TO_EVENFRAME env var
    ($content:expr, $log_subdir:expr, standard) => {{
        let filename = format!("{}.log", chrono::Local::now().format("%Y_%m_%d_%H_%M_%S"));
        let logs_dir = format!(
            "{}/{}",
            std::env::var("ABSOLUTE_PATH_TO_EVENFRAME")
                .expect("ABSOLUTE_PATH_TO_EVENFRAME not set"),
            $log_subdir
        );

        $crate::__internal_log_impl!($content, logs_dir, filename, false, impl);
    }};

    ($content:expr, $log_subdir:expr, $filename:expr, standard) => {{
        let logs_dir = format!(
            "{}/{}",
            std::env::var("ABSOLUTE_PATH_TO_EVENFRAME")
                .expect("ABSOLUTE_PATH_TO_EVENFRAME not set"),
            $log_subdir
        );

        $crate::__internal_log_impl!($content, logs_dir, $filename, false, impl);
    }};

    ($content:expr, $log_subdir:expr, $filename:expr, $append:expr, standard) => {{
        let logs_dir = format!(
            "{}/{}",
            std::env::var("ABSOLUTE_PATH_TO_EVENFRAME")
                .expect("ABSOLUTE_PATH_TO_EVENFRAME not set"),
            $log_subdir
        );

        $crate::__internal_log_impl!($content, logs_dir, $filename, $append, impl);
    }};

    // Core implementation
    ($content:expr, $logs_dir:expr, $filename:expr, $append:expr, impl) => {{
        use std::io::Write;

        // Create logs directory if it doesn't exist
        let _ = std::fs::create_dir_all(&$logs_dir);

        let path_str = &format!("{}/{}", $logs_dir, $filename);
        let path = std::path::Path::new(path_str);

        let mut options = std::fs::OpenOptions::new();
        options.create(true);
        if $append {
            options.append(true);
        } else {
            options.write(true).truncate(true);
        }

        if let Ok(mut file_handle) = options.open(path) {
            // Check if the expression is a format! macro or string literal
            let expr_str = stringify!($content);
            let formatted = if expr_str.starts_with("format!")
                || expr_str.starts_with("&format!")
                || expr_str.starts_with("\"")
                || expr_str.starts_with("String::")
            {
                // For formatted strings, just output the content directly
                format!("{}\n", $content)
            } else if $filename.ends_with(".surql") {
                // For .surql files, output the content as a plain string without debug formatting
                format!("{}\n", $content)
            } else {
                // For other types, use debug output with location info
                let value_str = format!("{:#?}", &$content);

                // Check if it's a multi-line value
                if value_str.contains('\n') || value_str.len() > 80 {
                    format!(
                        "[{}:{}] {} = \n{}\n",
                        file!(),
                        line!(),
                        stringify!($content),
                        value_str
                    )
                } else {
                    format!(
                        "[{}:{}] {} = {}\n",
                        file!(),
                        line!(),
                        stringify!($content),
                        value_str
                    )
                }
            };
            let _ = file_handle.write_all(formatted.as_bytes());
        }
    }};
}

/// Logging macro for the evenframe crate.
///
/// # Examples
///
/// Log to a timestamp-based file (e.g., "2024_01_12_14_30_52.log"):
/// ```no_run
/// # use evenframe_core::evenframe_log;
/// evenframe_log!("Sync started");
/// ```
///
/// Log to a specific file (overwrites existing content):
/// ```no_run
/// # use evenframe_core::evenframe_log;
/// evenframe_log!("Types generated", "output.log");
/// ```
///
/// Log to a specific file with append mode:
/// ```no_run
/// # use evenframe_core::evenframe_log;
/// evenframe_log!("New type added", "output.log", true);
/// ```
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
