//! Shared doc comment formatting helpers for typesync generators.

/// Format a description as a JSDoc comment block (`/** ... */`).
///
/// Multi-line descriptions produce a multi-line JSDoc block.
/// Escapes `*/` sequences to prevent premature block closure.
pub fn format_jsdoc(desc: &str, indent: &str) -> String {
    let escaped = desc.replace("*/", "* /");
    let lines: Vec<&str> = escaped.lines().collect();

    if lines.len() == 1 {
        format!("{}/** {} */", indent, lines[0])
    } else {
        let mut out = format!("{}/**\n", indent);
        for line in &lines {
            if line.is_empty() {
                out.push_str(&format!("{} *\n", indent));
            } else {
                out.push_str(&format!("{} * {}\n", indent, line));
            }
        }
        out.push_str(&format!("{} */", indent));
        out
    }
}

/// Format a description as triple-slash comments (`/// ...`), used by FlatBuffers.
pub fn format_triple_slash(desc: &str, indent: &str) -> String {
    let mut out = String::new();
    for line in desc.lines() {
        if line.is_empty() {
            out.push_str(&format!("{}///\n", indent));
        } else {
            out.push_str(&format!("{}/// {}\n", indent, line));
        }
    }
    out
}

/// Format a description as double-slash comments (`// ...`), used by Protobuf.
pub fn format_double_slash(desc: &str, indent: &str) -> String {
    let mut out = String::new();
    for line in desc.lines() {
        if line.is_empty() {
            out.push_str(&format!("{}//\n", indent));
        } else {
            out.push_str(&format!("{}// {}\n", indent, line));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_jsdoc_single_line() {
        assert_eq!(
            format_jsdoc("A user account", ""),
            "/** A user account */"
        );
    }

    #[test]
    fn test_format_jsdoc_single_line_with_indent() {
        assert_eq!(
            format_jsdoc("The email address", "  "),
            "  /** The email address */"
        );
    }

    #[test]
    fn test_format_jsdoc_multi_line() {
        let result = format_jsdoc("Line one\nLine two", "");
        assert_eq!(result, "/**\n * Line one\n * Line two\n */");
    }

    #[test]
    fn test_format_jsdoc_escapes_close() {
        let result = format_jsdoc("contains */ sequence", "");
        assert_eq!(result, "/** contains * / sequence */");
    }

    #[test]
    fn test_format_triple_slash() {
        assert_eq!(
            format_triple_slash("A table", ""),
            "/// A table\n"
        );
    }

    #[test]
    fn test_format_triple_slash_multi_line() {
        assert_eq!(
            format_triple_slash("Line one\nLine two", "    "),
            "    /// Line one\n    /// Line two\n"
        );
    }

    #[test]
    fn test_format_double_slash() {
        assert_eq!(
            format_double_slash("A message", ""),
            "// A message\n"
        );
    }

    #[test]
    fn test_format_double_slash_with_indent() {
        assert_eq!(
            format_double_slash("A field", "    "),
            "    // A field\n"
        );
    }
}
