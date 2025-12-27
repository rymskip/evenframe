//! FlatBuffers lexer using the logos crate.

use logos::Logos;

/// Tokens for FlatBuffers schema files.
#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
pub enum Token {
    // Keywords
    #[token("namespace")]
    Namespace,

    #[token("include")]
    Include,

    #[token("attribute")]
    Attribute,

    #[token("table")]
    Table,

    #[token("struct")]
    Struct,

    #[token("enum")]
    Enum,

    #[token("union")]
    Union,

    #[token("root_type")]
    RootType,

    #[token("file_identifier")]
    FileIdentifier,

    #[token("file_extension")]
    FileExtension,

    #[token("rpc_service")]
    RpcService,

    // Scalar types
    #[token("bool")]
    Bool,

    #[token("byte")]
    Byte,

    #[token("ubyte")]
    UByte,

    #[token("short")]
    Short,

    #[token("ushort")]
    UShort,

    #[token("int")]
    Int,

    #[token("uint")]
    UInt,

    #[token("long")]
    Long,

    #[token("ulong")]
    ULong,

    #[token("float")]
    Float,

    #[token("double")]
    Double,

    #[token("int8")]
    Int8,

    #[token("uint8")]
    UInt8,

    #[token("int16")]
    Int16,

    #[token("uint16")]
    UInt16,

    #[token("int32")]
    Int32,

    #[token("uint32")]
    UInt32,

    #[token("int64")]
    Int64,

    #[token("uint64")]
    UInt64,

    #[token("float32")]
    Float32,

    #[token("float64")]
    Float64,

    #[token("string")]
    String,

    // Punctuation
    #[token("{")]
    LBrace,

    #[token("}")]
    RBrace,

    #[token("(")]
    LParen,

    #[token(")")]
    RParen,

    #[token("[")]
    LBracket,

    #[token("]")]
    RBracket,

    #[token(";")]
    Semicolon,

    #[token(":")]
    Colon,

    #[token(",")]
    Comma,

    #[token("=")]
    Equals,

    #[token(".")]
    Dot,

    // Literals
    #[regex(r#""([^"\\]|\\.)*""#, |lex| {
        let s = lex.slice();
        s[1..s.len()-1].to_string()
    })]
    StringLiteral(std::string::String),

    #[regex(r"-?[0-9]+", |lex| lex.slice().parse::<i64>().ok())]
    IntegerLiteral(i64),

    #[regex(r"-?[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?", |lex| lex.slice().parse::<f64>().ok())]
    FloatLiteral(f64),

    #[regex(r"0[xX][0-9a-fA-F]+", |lex| {
        let s = lex.slice();
        i64::from_str_radix(&s[2..], 16).ok()
    })]
    HexLiteral(i64),

    #[token("true")]
    True,

    #[token("false")]
    False,

    // Identifiers (must come after keywords)
    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
    Identifier(std::string::String),

    // Comments
    #[regex(r"//[^\n]*", |lex| lex.slice()[2..].trim().to_string())]
    LineComment(std::string::String),

    #[regex(r"///[^\n]*", |lex| lex.slice()[3..].trim().to_string())]
    DocComment(std::string::String),

    #[regex(r"/\*[^*]*\*+(?:[^/*][^*]*\*+)*/", |lex| {
        let s = lex.slice();
        s[2..s.len()-2].trim().to_string()
    })]
    BlockComment(std::string::String),
}

/// A token with its span in the source.
#[derive(Debug, Clone)]
pub struct SpannedToken {
    pub token: Token,
    pub span: std::ops::Range<usize>,
}

/// Tokenize a FlatBuffers schema source.
pub fn tokenize(source: &str) -> Result<Vec<SpannedToken>, LexError> {
    let mut tokens = Vec::new();
    let mut lexer = Token::lexer(source);

    while let Some(result) = lexer.next() {
        match result {
            Ok(token) => {
                // Skip regular comments, keep doc comments
                if !matches!(token, Token::LineComment(_) | Token::BlockComment(_)) {
                    tokens.push(SpannedToken {
                        token,
                        span: lexer.span(),
                    });
                }
            }
            Err(()) => {
                return Err(LexError {
                    span: lexer.span(),
                    message: format!(
                        "Unexpected token: '{}'",
                        &source[lexer.span().start..lexer.span().end.min(source.len())]
                    ),
                });
            }
        }
    }

    Ok(tokens)
}

/// Lexer error.
#[derive(Debug, Clone)]
pub struct LexError {
    pub span: std::ops::Range<usize>,
    pub message: String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lex error at {:?}: {}", self.span, self.message)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_simple_table() {
        let source = r#"
            table Person {
                name: string;
                age: int32;
            }
        "#;

        let tokens = tokenize(source).unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.token, Token::Table)));
        assert!(tokens
            .iter()
            .any(|t| matches!(&t.token, Token::Identifier(s) if s == "Person")));
    }

    #[test]
    fn test_tokenize_with_metadata() {
        let source = r#"
            table User (validate: "required") {
                email: string (validate: "email");
            }
        "#;

        let tokens = tokenize(source).unwrap();
        assert!(tokens.iter().any(|t| matches!(&t.token, Token::LParen)));
        assert!(tokens
            .iter()
            .any(|t| matches!(&t.token, Token::Identifier(s) if s == "validate")));
    }
}
