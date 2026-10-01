//! Rejects glob imports in every tracked Rust file. Clippy's
//! `wildcard_imports` covers all of them except `use super::*` in code
//! compiled for tests, which it exempts even with
//! `warn-on-all-wildcard-imports`; this check closes that gap.

use crate::project_root;
use std::fs;
use std::process::Command;
use syn::visit::{self, Visit};
use syn::{ItemUse, UseTree};

pub fn check() -> bool {
    let root = project_root();
    let listed = match Command::new("git")
        .args(["ls-files", "*.rs"])
        .current_dir(&root)
        .output()
    {
        Ok(output) if output.status.success() => output.stdout,
        Ok(output) => {
            eprintln!(
                "git ls-files failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            return false;
        }
        Err(error) => {
            eprintln!("failed to run git ls-files: {error}");
            return false;
        }
    };

    let mut found = Vec::new();
    for relative in String::from_utf8_lossy(&listed).lines() {
        let path = root.join(relative);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("failed to read {}: {error}", path.display());
                return false;
            }
        };
        match glob_lines(&source) {
            Ok(lines) => found.extend(lines.into_iter().map(|line| format!("{relative}:{line}"))),
            Err(error) => {
                eprintln!("failed to parse {relative}: {error}");
                return false;
            }
        }
    }

    if found.is_empty() {
        println!("no glob imports");
        true
    } else {
        eprintln!("glob imports are not allowed; name each imported item:");
        for location in &found {
            eprintln!("  {location}");
        }
        false
    }
}

/// The 1-based line numbers of the `use` items in `source` that import `*`,
/// read from its syntax so text in strings and comments is never mistaken
/// for an import.
fn glob_lines(source: &str) -> Result<Vec<usize>, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut finder = GlobFinder { lines: Vec::new() };
    finder.visit_file(&file);
    Ok(finder.lines)
}

struct GlobFinder {
    lines: Vec<usize>,
}

impl<'ast> Visit<'ast> for GlobFinder {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        if imports_everything(&item.tree) {
            self.lines.push(item.use_token.span.start().line);
        }
        visit::visit_item_use(self, item);
    }
}

fn imports_everything(tree: &UseTree) -> bool {
    match tree {
        UseTree::Glob(_) => true,
        UseTree::Path(path) => imports_everything(&path.tree),
        UseTree::Group(group) => group.items.iter().any(imports_everything),
        UseTree::Name(_) | UseTree::Rename(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::glob_lines;

    #[test]
    fn flags_every_form_of_glob() {
        let source = "use std::fmt;\n\
                      use super::*;\n\
                      pub use crate::a::*;\n\
                      use crate::b::{\n    One,\n    *,\n};\n\
                      // use crate::c::*;\n\
                      fn body() { let product = 2 * 3; }\n";
        assert_eq!(glob_lines(source).unwrap(), vec![2, 3, 4]);
    }

    #[test]
    fn text_that_only_looks_like_a_glob_is_not_one() {
        let source = "const EXAMPLE: &str = \"use super::*;\";\n\
                      fn body() {\n    use std::collections::*;\n}\n";
        assert_eq!(glob_lines(source).unwrap(), vec![3]);
    }
}
