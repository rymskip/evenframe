use convert_case::{Case, Casing};
use globwalk::GlobWalkerBuilder;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use std::path::Path;
use syn::parse::ParseStream;

/// Arguments: glob pattern, runner function path, file type string
struct Arguments {
    pattern: syn::LitStr,
    called_function: syn::Path,
    file_type: syn::LitStr,
}

impl syn::parse::Parse for Arguments {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let pattern: syn::LitStr = input.parse()?;
        let _: syn::Token!(,) = input.parse()?;
        let call: syn::Path = input.parse()?;
        let _: syn::Token!(,) = input.parse()?;
        let file_type: syn::LitStr = input.parse()?;
        Ok(Self {
            pattern,
            called_function: call,
            file_type,
        })
    }
}

fn to_test_name(input: &str) -> String {
    let name = input.to_case(Case::Snake);
    if matches!(
        name.as_str(),
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "do"
            | "else"
            | "enum"
            | "for"
            | "if"
            | "in"
            | "return"
            | "static"
            | "super"
            | "try"
            | "type"
            | "while"
            | "yield"
    ) {
        format!("{name}_")
    } else if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("_{name}")
    } else {
        name
    }
}

/// Generates one `#[test]` function per file matching a glob pattern.
///
/// Usage:
/// ```ignore
/// tests_macros::gen_tests! {
///     "tests/specs/typesync/*.json",
///     crate::run_test,
///     "typesync"
/// }
/// ```
///
/// Each generated test calls: `run_test(file_path, expected_path, directory, file_type)`
#[proc_macro]
pub fn gen_tests(input: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(input as Arguments);

    let base = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let pattern = args.pattern.value();

    let walker = GlobWalkerBuilder::new(&base, &pattern)
        .build()
        .expect("invalid glob pattern");

    let mut tests = Vec::new();

    for entry in walker.into_iter().filter_map(Result::ok) {
        let path = entry.path().to_path_buf();
        if !path.is_file() {
            continue;
        }
        let file_name = match path.file_name().and_then(|f| f.to_str()) {
            Some(name) if name.contains("expected") => continue,
            Some(name) => name.to_string(),
            None => continue,
        };

        let file_stem = Path::new(&file_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&file_name);

        let test_name = to_test_name(file_stem);
        let test_ident = syn::Ident::new(&test_name, Span::call_site());

        let full_path = path.display().to_string();
        let directory = path.parent().unwrap().display().to_string();

        let mut expected_path = path.clone();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            expected_path.set_extension(format!("expected.{ext}"));
        }
        let expected = expected_path.display().to_string();

        let f = &args.called_function;
        let file_type = &args.file_type;

        tests.push(quote! {
            #[test]
            pub fn #test_ident() {
                #f(#full_path, #expected, #directory, #file_type);
            }
        });
    }

    // Sort by test name for deterministic output
    tests.sort_by_cached_key(|t| t.to_string());

    let output = quote! { #(#tests)* };
    output.into()
}
