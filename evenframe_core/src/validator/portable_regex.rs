//! The regex syntax Rust's `regex` and JavaScript's `RegExp` without the `u`
//! flag read the same way. A pattern in `#[validators(...)]` runs in both: the
//! Rust read and the schema's `string::matches` use Rust's engine, and every
//! TypeScript output uses JavaScript's, so a construct the two read
//! differently would let a value pass one and fail the other.

use regex_syntax::ast::{
    Assertion, AssertionKind, Ast, ClassBracketed, ClassSet, ClassSetItem, GroupKind,
    HexLiteralKind, Literal, LiteralKind, RepetitionKind, SpecialLiteralKind, parse::Parser,
};

/// Checks that `pattern` is in the portable subset, naming the first construct
/// that is not and what to write instead.
pub fn check(pattern: &str) -> Result<(), String> {
    let ast = Parser::new()
        .parse(pattern)
        .map_err(|error| format!("not a valid regex: {error}"))?;
    walk(&ast, false)
}

/// `whole_characters` holds for the direct operand of `*` or `+`, where a
/// construct matching any character consumes a whole one in both engines.
fn walk(ast: &Ast, whole_characters: bool) -> Result<(), String> {
    match ast {
        Ast::Empty(_) => Ok(()),
        Ast::Flags(_) => Err(
            "inline flags such as `(?i)` are Rust syntax JavaScript does not read; spell the \
             cases out, such as `[Aa]`"
                .to_owned(),
        ),
        Ast::Literal(literal) => check_literal(literal),
        Ast::Dot(_) => Err(
            "`.` matches `\\r`, U+2028 and U+2029 in Rust but not in JavaScript, and half of an \
             emoji in JavaScript: use a class such as `[^\\n]`, under `*` or `+`"
                .to_owned(),
        ),
        Ast::Assertion(assertion) => check_assertion(assertion),
        Ast::ClassUnicode(_) => Err(
            "`\\p{...}` is a Unicode class in Rust but the text `p{...}` in JavaScript without \
             the `u` flag: list the characters in a class"
                .to_owned(),
        ),
        Ast::ClassPerl(_) => Err(
            "`\\d`, `\\w` and `\\s` are Unicode-aware in Rust and ASCII in JavaScript: write \
             `[0-9]`, `[A-Za-z0-9_]` or `[ \\t\\n\\r\\f\\v]`"
                .to_owned(),
        ),
        Ast::ClassBracketed(class) => check_class(class, whole_characters),
        Ast::Repetition(repetition) => walk(
            &repetition.ast,
            matches!(
                repetition.op.kind,
                RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore
            ),
        ),
        Ast::Group(group) => {
            match &group.kind {
                GroupKind::CaptureName {
                    starts_with_p: true,
                    ..
                } => {
                    return Err(
                        "`(?P<name>...)` is Rust syntax: write `(?<name>...)`, which both read"
                            .to_owned(),
                    );
                }
                GroupKind::NonCapturing(flags) if !flags.items.is_empty() => {
                    return Err(
                        "a group's flags such as `(?i:...)` are Rust syntax JavaScript \
                                does not read; spell the cases out, such as `[Aa]`"
                            .to_owned(),
                    );
                }
                GroupKind::CaptureIndex(_)
                | GroupKind::CaptureName { .. }
                | GroupKind::NonCapturing(_) => {}
            }
            walk(&group.ast, false)
        }
        Ast::Alternation(alternation) => alternation
            .asts
            .iter()
            .try_for_each(|branch| walk(branch, false)),
        Ast::Concat(concat) => concat.asts.iter().try_for_each(|item| walk(item, false)),
    }
}

/// Refuses a pattern that anchors itself to the start or end of the text,
/// for one placed inside a pattern that does its own anchoring. A word
/// boundary is not an anchor.
pub fn refuse_anchors(pattern: &str) -> Result<(), String> {
    let ast = Parser::new()
        .parse(pattern)
        .map_err(|error| format!("not a valid regex: {error}"))?;
    if anchors(&ast) {
        return Err(
            "the pattern anchors itself with `^`, `$`, `\\A` or `\\z`; the validator anchors it \
             where it belongs, so drop them"
                .to_owned(),
        );
    }
    Ok(())
}

fn anchors(ast: &Ast) -> bool {
    match ast {
        Ast::Assertion(assertion) => matches!(
            assertion.kind,
            AssertionKind::StartLine
                | AssertionKind::EndLine
                | AssertionKind::StartText
                | AssertionKind::EndText
        ),
        Ast::Repetition(repetition) => anchors(&repetition.ast),
        Ast::Group(group) => anchors(&group.ast),
        Ast::Alternation(alternation) => alternation.asts.iter().any(anchors),
        Ast::Concat(concat) => concat.asts.iter().any(anchors),
        Ast::Empty(_)
        | Ast::Flags(_)
        | Ast::Literal(_)
        | Ast::Dot(_)
        | Ast::ClassUnicode(_)
        | Ast::ClassPerl(_)
        | Ast::ClassBracketed(_) => false,
    }
}

fn check_literal(literal: &Literal) -> Result<(), String> {
    if u32::from(literal.c) > 0xFFFF {
        return Err(format!(
            "`{}` lies outside the Basic Multilingual Plane, which JavaScript without the `u` \
             flag reads as two halves",
            literal.c
        ));
    }
    match &literal.kind {
        LiteralKind::Verbatim
        | LiteralKind::Meta
        | LiteralKind::Superfluous
        | LiteralKind::HexFixed(HexLiteralKind::X | HexLiteralKind::UnicodeShort)
        | LiteralKind::Special(
            SpecialLiteralKind::FormFeed
            | SpecialLiteralKind::Tab
            | SpecialLiteralKind::LineFeed
            | SpecialLiteralKind::CarriageReturn
            | SpecialLiteralKind::VerticalTab,
        ) => Ok(()),
        LiteralKind::HexFixed(HexLiteralKind::UnicodeLong) | LiteralKind::HexBrace(_) => Err(
            "`\\x{...}`, `\\u{...}` and `\\U...` are Rust syntax: write `\\xFF` or `\\uFFFF`"
                .to_owned(),
        ),
        LiteralKind::Octal => {
            Err("an octal escape reads differently in the two engines: write `\\xFF`".to_owned())
        }
        LiteralKind::Special(SpecialLiteralKind::Bell) => {
            Err("`\\a` is Rust syntax JavaScript reads as `a`: write `\\x07`".to_owned())
        }
        LiteralKind::Special(SpecialLiteralKind::Space) => {
            Err("an escaped space is Rust's verbose-mode syntax: write the space".to_owned())
        }
    }
}

fn check_assertion(assertion: &Assertion) -> Result<(), String> {
    match assertion.kind {
        AssertionKind::StartLine | AssertionKind::EndLine => Ok(()),
        AssertionKind::StartText | AssertionKind::EndText => {
            Err("`\\A` and `\\z` are Rust syntax: write `^` and `$`".to_owned())
        }
        AssertionKind::WordBoundary
        | AssertionKind::NotWordBoundary
        | AssertionKind::WordBoundaryStart
        | AssertionKind::WordBoundaryEnd
        | AssertionKind::WordBoundaryStartAngle
        | AssertionKind::WordBoundaryEndAngle
        | AssertionKind::WordBoundaryStartHalf
        | AssertionKind::WordBoundaryEndHalf => Err(
            "word boundaries are Unicode-aware in Rust and ASCII in JavaScript: match the \
             characters on either side instead"
                .to_owned(),
        ),
    }
}

fn check_class(class: &ClassBracketed, whole_characters: bool) -> Result<(), String> {
    if class.negated && !whole_characters {
        return Err(
            "a negated class matches a whole character in Rust and half of an emoji in \
             JavaScript, so it is portable only directly under `*` or `+`"
                .to_owned(),
        );
    }
    match &class.kind {
        ClassSet::Item(item) => check_class_item(item),
        ClassSet::BinaryOp(_) => Err(
            "class set operations such as `&&` and `--` are Rust syntax: list the characters"
                .to_owned(),
        ),
    }
}

fn check_class_item(item: &ClassSetItem) -> Result<(), String> {
    match item {
        ClassSetItem::Empty(_) => Ok(()),
        ClassSetItem::Literal(literal) => check_literal(literal),
        ClassSetItem::Range(range) => {
            check_literal(&range.start)?;
            check_literal(&range.end)
        }
        ClassSetItem::Ascii(_) => Err(
            "POSIX classes such as `[[:alpha:]]` are Rust syntax: write the range, such as \
             `[A-Za-z]`"
                .to_owned(),
        ),
        ClassSetItem::Unicode(_) => Err(
            "`\\p{...}` is a Unicode class in Rust but the text `p{...}` in JavaScript without \
             the `u` flag: list the characters"
                .to_owned(),
        ),
        ClassSetItem::Perl(_) => Err(
            "`\\d`, `\\w` and `\\s` are Unicode-aware in Rust and ASCII in JavaScript: write \
             `0-9`, `A-Za-z0-9_` or ` \\t\\n\\r\\f\\v` inside the class"
                .to_owned(),
        ),
        ClassSetItem::Bracketed(_) => Err(
            "a class nested in a class is Rust syntax: list the characters in one class".to_owned(),
        ),
        ClassSetItem::Union(union) => union.items.iter().try_for_each(check_class_item),
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn constructs_both_engines_read_alike_are_accepted() {
        for pattern in [
            r"^[A-Z]{3}$",
            r"^/(?:[^/\\\n]+[^\n]*)?$",
            r"^[0-9]+(?:\.[0-9]{1,2})?$",
            r"^(?<area>[0-9]{3})-[0-9]{4}$",
            r"^\x41é[\t\n]$",
            r"^(?:cat|dog)s?$",
            r"^[^@]+@[^@]+$",
        ] {
            assert_eq!(check(pattern), Ok(()), "{pattern}");
        }
    }

    #[test]
    fn anchors_are_refused_but_word_boundaries_are_not() {
        use super::refuse_anchors;
        assert!(refuse_anchors("[A-Z]").is_ok());
        assert!(refuse_anchors(r"\bfoo\b").is_ok());
        assert!(refuse_anchors("[$^]").is_ok());
        assert!(refuse_anchors("^abc").is_err());
        assert!(refuse_anchors("a(?:b|c$)").is_err());
        assert!(refuse_anchors(r"\Aabc").is_err());
    }

    #[test]
    fn constructs_the_engines_read_differently_are_refused_with_the_portable_form() {
        for (pattern, expected) in [
            (r"^a.b$", "[^\\n]"),
            (r"^\d+$", "[0-9]"),
            (r"^[\w-]+$", "A-Za-z0-9_"),
            (r"^\p{L}+$", "Unicode class"),
            (r"(?i)^abc$", "inline flags"),
            (r"^(?i:abc)$", "spell the cases out"),
            (r"\Aabc\z", "`^` and `$`"),
            (r"\bword\b", "word boundaries"),
            (r"^[a-z&&[^aeiou]]$", "set operations"),
            (r"^[a[bc]]$", "nested in a class"),
            (r"^[[:alpha:]]+$", "POSIX"),
            (r"^(?P<name>a)$", "(?<name>...)"),
            (r"^\x{41}$", "Rust syntax"),
            (r"^[^a]$", "negated class"),
            (r"^[^a]{2}$", "negated class"),
            (r"^😀$", "Basic Multilingual Plane"),
            (r"^(a$", "not a valid regex"),
        ] {
            let message = check(pattern).expect_err(pattern);
            assert!(message.contains(expected), "{pattern}: {message}");
        }
    }
}
