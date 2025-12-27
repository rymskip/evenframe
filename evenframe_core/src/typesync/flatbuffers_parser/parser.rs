//! FlatBuffers parser - converts tokens to AST.

use super::ast::{
    EnumDef, EnumValue, FbsType, FieldDef, FlatBuffersSchema, Metadata, ScalarType, StructDef,
    TableDef, UnionDef, UnionVariant,
};
use super::lexer::{SpannedToken, Token};
use std::collections::HashMap;

/// Parse error with location information.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Option<std::ops::Range<usize>>,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref span) = self.span {
            write!(f, "Parse error at {:?}: {}", span, self.message)
        } else {
            write!(f, "Parse error: {}", self.message)
        }
    }
}

impl std::error::Error for ParseError {}

/// Parser state.
pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    pending_doc_comments: Vec<String>,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>) -> Self {
        Self {
            tokens,
            pos: 0,
            pending_doc_comments: Vec::new(),
        }
    }

    /// Parse a complete FlatBuffers schema.
    pub fn parse(&mut self) -> Result<FlatBuffersSchema, ParseError> {
        let mut schema = FlatBuffersSchema::new();

        while !self.is_at_end() {
            // Collect doc comments
            while self.check_doc_comment() {
                if let Some(SpannedToken {
                    token: Token::DocComment(comment),
                    ..
                }) = self.advance()
                {
                    self.pending_doc_comments.push(comment);
                }
            }

            match self.peek() {
                Some(Token::Namespace) => {
                    schema.namespace = Some(self.parse_namespace()?);
                }
                Some(Token::Include) => {
                    schema.includes.push(self.parse_include()?);
                }
                Some(Token::Attribute) => {
                    schema.attributes.push(self.parse_attribute_decl()?);
                }
                Some(Token::Table) => {
                    schema.tables.push(self.parse_table()?);
                }
                Some(Token::Struct) => {
                    schema.structs.push(self.parse_struct()?);
                }
                Some(Token::Enum) => {
                    schema.enums.push(self.parse_enum()?);
                }
                Some(Token::Union) => {
                    schema.unions.push(self.parse_union()?);
                }
                Some(Token::RootType) => {
                    schema.root_type = Some(self.parse_root_type()?);
                }
                Some(Token::FileIdentifier) => {
                    schema.file_identifier = Some(self.parse_file_identifier()?);
                }
                Some(Token::FileExtension) => {
                    schema.file_extension = Some(self.parse_file_extension()?);
                }
                Some(Token::RpcService) => {
                    // Skip RPC service definitions for now
                    self.skip_rpc_service()?;
                }
                Some(_) => {
                    return Err(self.error("Unexpected token at top level"));
                }
                None => break,
            }
        }

        Ok(schema)
    }

    fn parse_namespace(&mut self) -> Result<String, ParseError> {
        self.expect(Token::Namespace)?;
        let mut parts = vec![self.expect_identifier()?];
        while self.check(&Token::Dot) {
            self.advance();
            parts.push(self.expect_identifier()?);
        }
        self.expect(Token::Semicolon)?;
        Ok(parts.join("."))
    }

    fn parse_include(&mut self) -> Result<String, ParseError> {
        self.expect(Token::Include)?;
        let path = self.expect_string_literal()?;
        self.expect(Token::Semicolon)?;
        Ok(path)
    }

    fn parse_attribute_decl(&mut self) -> Result<String, ParseError> {
        self.expect(Token::Attribute)?;
        let name = self.expect_string_literal()?;
        self.expect(Token::Semicolon)?;
        Ok(name)
    }

    fn parse_table(&mut self) -> Result<TableDef, ParseError> {
        let doc_comment = self.take_doc_comments();
        self.expect(Token::Table)?;
        let name = self.expect_identifier()?;
        let metadata = self.parse_optional_metadata()?;
        self.expect(Token::LBrace)?;

        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) {
            fields.push(self.parse_field()?);
        }

        self.expect(Token::RBrace)?;

        Ok(TableDef {
            name,
            fields,
            metadata,
            doc_comment,
        })
    }

    fn parse_struct(&mut self) -> Result<StructDef, ParseError> {
        let doc_comment = self.take_doc_comments();
        self.expect(Token::Struct)?;
        let name = self.expect_identifier()?;
        let metadata = self.parse_optional_metadata()?;
        self.expect(Token::LBrace)?;

        let mut fields = Vec::new();
        while !self.check(&Token::RBrace) {
            fields.push(self.parse_field()?);
        }

        self.expect(Token::RBrace)?;

        Ok(StructDef {
            name,
            fields,
            metadata,
            doc_comment,
        })
    }

    fn parse_field(&mut self) -> Result<FieldDef, ParseError> {
        // Collect doc comments for this field
        while self.check_doc_comment() {
            if let Some(SpannedToken {
                token: Token::DocComment(comment),
                ..
            }) = self.advance()
            {
                self.pending_doc_comments.push(comment);
            }
        }
        let doc_comment = self.take_doc_comments();

        let name = self.expect_identifier()?;
        self.expect(Token::Colon)?;
        let field_type = self.parse_type()?;

        // Parse optional default value
        let default_value = if self.check(&Token::Equals) {
            self.advance();
            Some(self.parse_scalar_value()?)
        } else {
            None
        };

        let metadata = self.parse_optional_metadata()?;
        self.expect(Token::Semicolon)?;

        Ok(FieldDef {
            name,
            field_type,
            default_value,
            metadata,
            doc_comment,
        })
    }

    fn parse_type(&mut self) -> Result<FbsType, ParseError> {
        // Check for vector type [T]
        if self.check(&Token::LBracket) {
            self.advance();
            let inner = self.parse_type()?;

            // Check for fixed-size array [T:N]
            if self.check(&Token::Colon) {
                self.advance();
                let size = self.expect_integer()? as usize;
                self.expect(Token::RBracket)?;
                return Ok(FbsType::Array(Box::new(inner), size));
            }

            self.expect(Token::RBracket)?;
            return Ok(FbsType::Vector(Box::new(inner)));
        }

        // Check for scalar types
        match self.peek() {
            Some(Token::Bool) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Bool))
            }
            Some(Token::Byte | Token::Int8) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Int8))
            }
            Some(Token::UByte | Token::UInt8) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::UInt8))
            }
            Some(Token::Short | Token::Int16) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Int16))
            }
            Some(Token::UShort | Token::UInt16) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::UInt16))
            }
            Some(Token::Int | Token::Int32) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Int32))
            }
            Some(Token::UInt | Token::UInt32) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::UInt32))
            }
            Some(Token::Long | Token::Int64) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Int64))
            }
            Some(Token::ULong | Token::UInt64) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::UInt64))
            }
            Some(Token::Float | Token::Float32) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Float32))
            }
            Some(Token::Double | Token::Float64) => {
                self.advance();
                Ok(FbsType::Scalar(ScalarType::Float64))
            }
            Some(Token::String) => {
                self.advance();
                Ok(FbsType::String)
            }
            Some(Token::Identifier(_)) => {
                let name = self.expect_identifier()?;
                Ok(FbsType::Named(name))
            }
            _ => Err(self.error("Expected type")),
        }
    }

    fn parse_enum(&mut self) -> Result<EnumDef, ParseError> {
        let doc_comment = self.take_doc_comments();
        self.expect(Token::Enum)?;
        let name = self.expect_identifier()?;
        self.expect(Token::Colon)?;

        // Parse underlying type
        let underlying_type = self.parse_scalar_type()?;
        let metadata = self.parse_optional_metadata()?;
        self.expect(Token::LBrace)?;

        let mut values = Vec::new();
        while !self.check(&Token::RBrace) {
            values.push(self.parse_enum_value()?);
            // Optional comma
            self.check(&Token::Comma).then(|| self.advance());
        }

        self.expect(Token::RBrace)?;

        Ok(EnumDef {
            name,
            underlying_type,
            values,
            metadata,
            doc_comment,
        })
    }

    fn parse_enum_value(&mut self) -> Result<EnumValue, ParseError> {
        // Collect doc comments
        while self.check_doc_comment() {
            if let Some(SpannedToken {
                token: Token::DocComment(comment),
                ..
            }) = self.advance()
            {
                self.pending_doc_comments.push(comment);
            }
        }
        let doc_comment = self.take_doc_comments();

        let name = self.expect_identifier()?;
        let value = if self.check(&Token::Equals) {
            self.advance();
            Some(self.expect_integer()?)
        } else {
            None
        };

        Ok(EnumValue {
            name,
            value,
            doc_comment,
        })
    }

    fn parse_union(&mut self) -> Result<UnionDef, ParseError> {
        let doc_comment = self.take_doc_comments();
        self.expect(Token::Union)?;
        let name = self.expect_identifier()?;
        let metadata = self.parse_optional_metadata()?;
        self.expect(Token::LBrace)?;

        let mut variants = Vec::new();
        while !self.check(&Token::RBrace) {
            let variant_name = self.expect_identifier()?;

            // Check for alias: VariantName: TypeName
            let type_name = if self.check(&Token::Colon) {
                self.advance();
                Some(self.expect_identifier()?)
            } else {
                None
            };

            variants.push(UnionVariant {
                name: variant_name,
                type_name,
            });

            // Optional comma
            self.check(&Token::Comma).then(|| self.advance());
        }

        self.expect(Token::RBrace)?;

        Ok(UnionDef {
            name,
            variants,
            metadata,
            doc_comment,
        })
    }

    fn parse_root_type(&mut self) -> Result<String, ParseError> {
        self.expect(Token::RootType)?;
        let name = self.expect_identifier()?;
        self.expect(Token::Semicolon)?;
        Ok(name)
    }

    fn parse_file_identifier(&mut self) -> Result<String, ParseError> {
        self.expect(Token::FileIdentifier)?;
        let id = self.expect_string_literal()?;
        self.expect(Token::Semicolon)?;
        Ok(id)
    }

    fn parse_file_extension(&mut self) -> Result<String, ParseError> {
        self.expect(Token::FileExtension)?;
        let ext = self.expect_string_literal()?;
        self.expect(Token::Semicolon)?;
        Ok(ext)
    }

    fn skip_rpc_service(&mut self) -> Result<(), ParseError> {
        self.expect(Token::RpcService)?;
        self.expect_identifier()?;
        self.expect(Token::LBrace)?;

        let mut depth = 1;
        while depth > 0 {
            match self.advance() {
                Some(SpannedToken {
                    token: Token::LBrace,
                    ..
                }) => depth += 1,
                Some(SpannedToken {
                    token: Token::RBrace,
                    ..
                }) => depth -= 1,
                None => return Err(self.error("Unexpected end of input in rpc_service")),
                _ => {}
            }
        }
        Ok(())
    }

    fn parse_optional_metadata(&mut self) -> Result<Metadata, ParseError> {
        if !self.check(&Token::LParen) {
            return Ok(Metadata::new());
        }

        self.advance();
        let mut attributes = HashMap::new();

        while !self.check(&Token::RParen) {
            let key = self.expect_identifier()?;

            let value = if self.check(&Token::Colon) {
                self.advance();
                Some(self.parse_metadata_value()?)
            } else {
                None
            };

            attributes.insert(key, value);

            // Optional comma
            self.check(&Token::Comma).then(|| self.advance());
        }

        self.expect(Token::RParen)?;

        Ok(Metadata { attributes })
    }

    fn parse_metadata_value(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Token::StringLiteral(_)) => self.expect_string_literal(),
            Some(Token::IntegerLiteral(_)) => Ok(self.expect_integer()?.to_string()),
            Some(Token::FloatLiteral(_)) => {
                if let Some(SpannedToken {
                    token: Token::FloatLiteral(f),
                    ..
                }) = self.advance()
                {
                    Ok(f.to_string())
                } else {
                    Err(self.error("Expected float"))
                }
            }
            Some(Token::True) => {
                self.advance();
                Ok("true".to_string())
            }
            Some(Token::False) => {
                self.advance();
                Ok("false".to_string())
            }
            Some(Token::Identifier(_)) => self.expect_identifier(),
            _ => Err(self.error("Expected metadata value")),
        }
    }

    fn parse_scalar_type(&mut self) -> Result<ScalarType, ParseError> {
        match self.peek() {
            Some(Token::Byte | Token::Int8) => {
                self.advance();
                Ok(ScalarType::Int8)
            }
            Some(Token::UByte | Token::UInt8) => {
                self.advance();
                Ok(ScalarType::UInt8)
            }
            Some(Token::Short | Token::Int16) => {
                self.advance();
                Ok(ScalarType::Int16)
            }
            Some(Token::UShort | Token::UInt16) => {
                self.advance();
                Ok(ScalarType::UInt16)
            }
            Some(Token::Int | Token::Int32) => {
                self.advance();
                Ok(ScalarType::Int32)
            }
            Some(Token::UInt | Token::UInt32) => {
                self.advance();
                Ok(ScalarType::UInt32)
            }
            Some(Token::Long | Token::Int64) => {
                self.advance();
                Ok(ScalarType::Int64)
            }
            Some(Token::ULong | Token::UInt64) => {
                self.advance();
                Ok(ScalarType::UInt64)
            }
            _ => Err(self.error("Expected integer scalar type for enum")),
        }
    }

    fn parse_scalar_value(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(Token::IntegerLiteral(_)) => Ok(self.expect_integer()?.to_string()),
            Some(Token::HexLiteral(_)) => {
                if let Some(SpannedToken {
                    token: Token::HexLiteral(v),
                    ..
                }) = self.advance()
                {
                    Ok(format!("0x{:x}", v))
                } else {
                    Err(self.error("Expected hex literal"))
                }
            }
            Some(Token::FloatLiteral(_)) => {
                if let Some(SpannedToken {
                    token: Token::FloatLiteral(f),
                    ..
                }) = self.advance()
                {
                    Ok(f.to_string())
                } else {
                    Err(self.error("Expected float"))
                }
            }
            Some(Token::True) => {
                self.advance();
                Ok("true".to_string())
            }
            Some(Token::False) => {
                self.advance();
                Ok("false".to_string())
            }
            Some(Token::StringLiteral(_)) => self.expect_string_literal(),
            Some(Token::Identifier(_)) => {
                // Could be an enum value reference
                self.expect_identifier()
            }
            _ => Err(self.error("Expected scalar value")),
        }
    }

    // Helper methods

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.token)
    }

    fn advance(&mut self) -> Option<SpannedToken> {
        if self.is_at_end() {
            None
        } else {
            let token = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(token)
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn check(&self, expected: &Token) -> bool {
        self.peek().map(|t| std::mem::discriminant(t) == std::mem::discriminant(expected)).unwrap_or(false)
    }

    fn check_doc_comment(&self) -> bool {
        matches!(self.peek(), Some(Token::DocComment(_)))
    }

    fn expect(&mut self, expected: Token) -> Result<(), ParseError> {
        if self.check(&expected) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("Expected {:?}", expected)))
        }
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(SpannedToken {
                token: Token::Identifier(name),
                ..
            }) => Ok(name),
            _ => Err(self.error("Expected identifier")),
        }
    }

    fn expect_string_literal(&mut self) -> Result<String, ParseError> {
        match self.advance() {
            Some(SpannedToken {
                token: Token::StringLiteral(s),
                ..
            }) => Ok(s),
            _ => Err(self.error("Expected string literal")),
        }
    }

    fn expect_integer(&mut self) -> Result<i64, ParseError> {
        match self.advance() {
            Some(SpannedToken {
                token: Token::IntegerLiteral(v),
                ..
            }) => Ok(v),
            Some(SpannedToken {
                token: Token::HexLiteral(v),
                ..
            }) => Ok(v),
            _ => Err(self.error("Expected integer")),
        }
    }

    fn take_doc_comments(&mut self) -> Option<String> {
        if self.pending_doc_comments.is_empty() {
            None
        } else {
            let comments = std::mem::take(&mut self.pending_doc_comments);
            Some(comments.join("\n"))
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            span: self.tokens.get(self.pos).map(|t| t.span.clone()),
        }
    }
}

/// Parse FlatBuffers schema source to AST.
pub fn parse(source: &str) -> Result<FlatBuffersSchema, ParseError> {
    use super::lexer::tokenize;

    let tokens = tokenize(source).map_err(|e| ParseError {
        message: e.message,
        span: Some(e.span),
    })?;

    let mut parser = Parser::new(tokens);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_table() {
        let source = r#"
            namespace Example;

            table Person {
                name: string;
                age: int32;
            }
        "#;

        let schema = parse(source).unwrap();
        assert_eq!(schema.namespace, Some("Example".to_string()));
        assert_eq!(schema.tables.len(), 1);
        assert_eq!(schema.tables[0].name, "Person");
        assert_eq!(schema.tables[0].fields.len(), 2);
    }

    #[test]
    fn test_parse_enum() {
        let source = r#"
            enum Color : byte {
                Red = 0,
                Green,
                Blue = 5
            }
        "#;

        let schema = parse(source).unwrap();
        assert_eq!(schema.enums.len(), 1);
        assert_eq!(schema.enums[0].name, "Color");
        assert_eq!(schema.enums[0].values.len(), 3);
        assert_eq!(schema.enums[0].values[0].value, Some(0));
        assert_eq!(schema.enums[0].values[2].value, Some(5));
    }

    #[test]
    fn test_parse_with_metadata() {
        let source = r#"
            table User (validate: "required") {
                email: string (validate: "email");
                age: int32 = 18;
            }
        "#;

        let schema = parse(source).unwrap();
        assert_eq!(schema.tables[0].metadata.get("validate"), Some("required"));
        assert_eq!(
            schema.tables[0].fields[0].metadata.get("validate"),
            Some("email")
        );
        assert_eq!(
            schema.tables[0].fields[1].default_value,
            Some("18".to_string())
        );
    }
}
