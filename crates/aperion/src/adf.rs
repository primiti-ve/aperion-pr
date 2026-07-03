use std::fmt;
use std::fs;
use std::path::Path;

use thiserror::Error;

pub const SUPPORTED_ADF_VERSIONS: &[u32] = &[1];
pub const ADFB_MAGIC: &[u8; 4] = b"ADFB";
const CUSTOM_TAG: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdfEncoding {
    Text,
    Binary,
}

impl fmt::Display for AdfEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => write!(f, "ADFT"),
            Self::Binary => write!(f, "ADFB"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdfCheckSummary {
    pub version: u32,
    pub material_imports: usize,
    pub edits: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdfDocument {
    pub version: u32,
    pub statements: Vec<AdfStatement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdfStatement {
    MaterialImport(MaterialImport),
    Edit(Edit),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialImport {
    pub alias: Option<String>,
    pub path: String,
    pub syntax: MaterialImportSyntax,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialImportSyntax {
    Use,
    LegacyAtMaterial,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub operation: Operation,
    pub primitive: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Union,
    Difference,
    Intersection,
    Paint,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    String(String),
    Ident(String),
    Bool(bool),
    Vector(Vec<Value>),
}

impl fmt::Display for AdfDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ADF v{}", self.version)?;

        for (index, statement) in self.statements.iter().enumerate() {
            match statement {
                AdfStatement::MaterialImport(import) => {
                    write!(f, "  {:>2}. material import ", index + 1)?;

                    if let Some(alias) = &import.alias {
                        write!(f, "{alias} ")?;
                    }

                    writeln!(f, "from {:?}", import.path)?;
                }
                AdfStatement::Edit(edit) => {
                    writeln!(
                        f,
                        "  {:>2}. {:?} {}",
                        index + 1,
                        edit.operation,
                        edit.primitive
                    )?;

                    for property in &edit.properties {
                        writeln!(f, "      {} = {}", property.key, property.value)?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Union => "union",
            Self::Difference => "difference",
            Self::Intersection => "intersection",
            Self::Paint => "paint",
        };

        write!(f, "{name}")
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(number) => write!(f, "{number}"),
            Self::String(value) => write!(f, "{value:?}"),
            Self::Ident(value) => write!(f, "{value}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Vector(values) => {
                write!(f, "[")?;

                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }

                    write!(f, "{value}")?;
                }

                write!(f, "]")
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum AdfError {
    #[error("could not read {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("ADF text is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Binary(String),
}

pub fn parse_file(path: &Path) -> Result<AdfDocument, AdfError> {
    let source = fs::read_to_string(path).map_err(|source| AdfError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;

    parse(&source)
}

pub fn parse(source: &str) -> Result<AdfDocument, AdfError> {
    let tokens = Lexer::new(source).lex()?;
    Parser::new(tokens).parse_document()
}

pub fn check(source: &str) -> Result<AdfCheckSummary, AdfError> {
    CheckingParser::new(Lexer::new(source))?.check_document()
}

pub fn detect_encoding(bytes: &[u8]) -> AdfEncoding {
    if bytes.starts_with(ADFB_MAGIC) {
        AdfEncoding::Binary
    } else {
        AdfEncoding::Text
    }
}

pub fn check_bytes(bytes: &[u8]) -> Result<(AdfEncoding, AdfCheckSummary), AdfError> {
    match detect_encoding(bytes) {
        AdfEncoding::Text => Ok((AdfEncoding::Text, check(std::str::from_utf8(bytes)?)?)),
        AdfEncoding::Binary => Ok((AdfEncoding::Binary, check_binary(bytes)?)),
    }
}

pub fn parse_bytes(bytes: &[u8]) -> Result<(AdfEncoding, AdfDocument), AdfError> {
    match detect_encoding(bytes) {
        AdfEncoding::Text => Ok((AdfEncoding::Text, parse(std::str::from_utf8(bytes)?)?)),
        AdfEncoding::Binary => Ok((AdfEncoding::Binary, parse_binary(bytes)?)),
    }
}

pub fn parse_binary_file(path: &Path) -> Result<AdfDocument, AdfError> {
    let bytes = fs::read(path).map_err(|source| AdfError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;

    parse_binary(&bytes)
}

pub fn parse_binary(bytes: &[u8]) -> Result<AdfDocument, AdfError> {
    BinaryReader::new(bytes).parse_document()
}

pub fn check_binary(bytes: &[u8]) -> Result<AdfCheckSummary, AdfError> {
    BinaryReader::new(bytes).check_document()
}

pub fn encode_binary(document: &AdfDocument) -> Result<Vec<u8>, AdfError> {
    BinaryWriter::new().encode_document(document)
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(f64),
    String(String),
    Plus,
    Minus,
    Caret,
    Question,
    At,
    Colon,
    Comma,
    Semicolon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    line: usize,
    column: usize,
}

struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    lookahead: Option<char>,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        let mut chars = source.chars();
        let lookahead = chars.next();

        Self {
            chars,
            lookahead,
            line: 1,
            column: 1,
        }
    }

    fn lex(mut self) -> Result<Vec<Token>, AdfError> {
        let mut tokens = Vec::new();

        loop {
            let token = self.next_token()?;
            let is_eof = matches!(token.kind, TokenKind::Eof);
            tokens.push(token);

            if is_eof {
                return Ok(tokens);
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, AdfError> {
        self.skip_ignored();

        let line = self.line;
        let column = self.column;

        let Some(current) = self.lookahead else {
            return Ok(Token {
                kind: TokenKind::Eof,
                line,
                column,
            });
        };

        let kind = match current {
            '+' => {
                self.bump();
                TokenKind::Plus
            }
            '-' => {
                self.bump();
                TokenKind::Minus
            }
            '^' => {
                self.bump();
                TokenKind::Caret
            }
            '?' => {
                self.bump();
                TokenKind::Question
            }
            '@' => {
                self.bump();
                TokenKind::At
            }
            ':' => {
                self.bump();
                TokenKind::Colon
            }
            ',' => {
                self.bump();
                TokenKind::Comma
            }
            ';' => {
                self.bump();
                TokenKind::Semicolon
            }
            '(' => {
                self.bump();
                TokenKind::LParen
            }
            ')' => {
                self.bump();
                TokenKind::RParen
            }
            '[' => {
                self.bump();
                TokenKind::LBracket
            }
            ']' => {
                self.bump();
                TokenKind::RBracket
            }
            '"' => TokenKind::String(self.lex_string()?),
            '.' | '0'..='9' => TokenKind::Number(self.lex_number()?),
            _ if is_ident_start(current) => TokenKind::Ident(self.lex_ident()),
            _ => {
                return Err(AdfError::Parse(format!(
                    "unexpected character {:?} at line {}, column {}",
                    current, line, column
                )));
            }
        };

        Ok(Token { kind, line, column })
    }

    fn skip_ignored(&mut self) {
        loop {
            while matches!(self.lookahead, Some(ch) if ch.is_whitespace()) {
                self.bump();
            }

            if self.lookahead == Some('/') && self.peek_char() == Some('/') {
                while !matches!(self.lookahead, None | Some('\n')) {
                    self.bump();
                }
                continue;
            }

            if self.lookahead == Some('#') {
                while !matches!(self.lookahead, None | Some('\n')) {
                    self.bump();
                }
                continue;
            }

            break;
        }
    }

    fn lex_string(&mut self) -> Result<String, AdfError> {
        let start_line = self.line;
        let start_column = self.column;
        let mut result = String::new();
        self.bump();

        while let Some(current) = self.lookahead {
            match current {
                '"' => {
                    self.bump();
                    return Ok(result);
                }
                '\\' => {
                    self.bump();
                    let Some(escaped) = self.lookahead else {
                        return Err(AdfError::Parse(format!(
                            "unterminated string starting at line {}, column {}",
                            start_line, start_column
                        )));
                    };

                    let resolved = match escaped {
                        '"' => '"',
                        '\\' => '\\',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        other => {
                            return Err(AdfError::Parse(format!(
                                "unsupported escape sequence \\{} at line {}, column {}",
                                other, self.line, self.column
                            )));
                        }
                    };

                    result.push(resolved);
                    self.bump();
                }
                _ => {
                    result.push(current);
                    self.bump();
                }
            }
        }

        Err(AdfError::Parse(format!(
            "unterminated string starting at line {}, column {}",
            start_line, start_column
        )))
    }

    fn lex_number(&mut self) -> Result<f64, AdfError> {
        let start_line = self.line;
        let start_column = self.column;
        let mut raw = String::new();

        if self.lookahead == Some('.') {
            raw.push('.');
            self.bump();
        }

        while let Some(current) = self.lookahead {
            if current.is_ascii_digit() || matches!(current, '.' | 'e' | 'E' | '+' | '-') {
                raw.push(current);
                self.bump();
            } else {
                break;
            }
        }

        raw.parse::<f64>().map_err(|_| {
            AdfError::Parse(format!(
                "invalid number {:?} at line {}, column {}",
                raw, start_line, start_column
            ))
        })
    }

    fn lex_ident(&mut self) -> String {
        let mut ident = String::new();

        while let Some(current) = self.lookahead {
            if is_ident_continue(current) {
                ident.push(current);
                self.bump();
            } else {
                break;
            }
        }

        ident
    }

    fn bump(&mut self) {
        if let Some(current) = self.lookahead {
            if current == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }

        self.lookahead = self.chars.next();
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.clone().next()
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '\\')
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_document(&mut self) -> Result<AdfDocument, AdfError> {
        self.expect_keyword("adf")?;
        let version = self.expect_u32()?;

        if !SUPPORTED_ADF_VERSIONS.contains(&version) {
            return Err(self.error_here(&format!(
                "unsupported ADF version {} (supported: {})",
                version,
                supported_versions_string()
            )));
        }

        self.expect(TokenKind::Semicolon, "expected ';' after ADF version")?;

        let mut statements = Vec::new();

        while !self.is_eof() {
            statements.push(self.parse_statement()?);
        }

        Ok(AdfDocument { version, statements })
    }

    fn parse_statement(&mut self) -> Result<AdfStatement, AdfError> {
        match &self.current().kind {
            TokenKind::At => self.parse_legacy_material_import().map(AdfStatement::MaterialImport),
            TokenKind::Ident(ident) if ident == "use" => {
                self.parse_use_material_import().map(AdfStatement::MaterialImport)
            }
            TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Question => {
                self.parse_edit().map(AdfStatement::Edit)
            }
            _ => Err(self.error_here("expected a material import or edit statement")),
        }
    }

    fn parse_use_material_import(&mut self) -> Result<MaterialImport, AdfError> {
        self.expect_keyword("use")?;
        self.expect_keyword("material")?;
        let alias = self.expect_ident()?;
        self.expect_keyword("from")?;
        let path = self.expect_string()?;
        self.expect(TokenKind::Semicolon, "expected ';' after material import")?;

        Ok(MaterialImport {
            alias: Some(alias),
            path,
            syntax: MaterialImportSyntax::Use,
        })
    }

    fn parse_legacy_material_import(&mut self) -> Result<MaterialImport, AdfError> {
        self.expect(TokenKind::At, "expected '@material'")?;
        self.expect_keyword("material")?;
        let path = self.expect_string()?;
        self.expect(TokenKind::Semicolon, "expected ';' after material import")?;

        Ok(MaterialImport {
            alias: None,
            path,
            syntax: MaterialImportSyntax::LegacyAtMaterial,
        })
    }

    fn parse_edit(&mut self) -> Result<Edit, AdfError> {
        let operation = match &self.current().kind {
            TokenKind::Plus => Operation::Union,
            TokenKind::Minus => Operation::Difference,
            TokenKind::Caret => Operation::Intersection,
            TokenKind::Question => Operation::Paint,
            _ => return Err(self.error_here("expected an edit operation")),
        };
        self.bump();

        let primitive = self.expect_ident()?;
        self.expect(TokenKind::LParen, "expected '(' after primitive name")?;

        let mut properties = self.parse_property_list(TokenKind::RParen, true)?;
        self.expect(TokenKind::RParen, "expected ')' after primitive properties")?;

        while !matches!(self.current().kind, TokenKind::Semicolon | TokenKind::Eof) {
            if matches!(self.current().kind, TokenKind::Comma) {
                self.bump();
                continue;
            }

            properties.push(self.parse_property(false)?);
        }

        self.expect(TokenKind::Semicolon, "expected ';' after edit")?;

        Ok(Edit {
            operation,
            primitive,
            properties,
        })
    }

    fn parse_property_list(
        &mut self,
        end: TokenKind,
        require_colon: bool,
    ) -> Result<Vec<Property>, AdfError> {
        let mut properties = Vec::new();

        while !self.same_variant(&self.current().kind, &end) {
            if matches!(self.current().kind, TokenKind::Comma) {
                self.bump();
                continue;
            }

            properties.push(self.parse_property(require_colon)?);
        }

        Ok(properties)
    }

    fn parse_property(&mut self, require_colon: bool) -> Result<Property, AdfError> {
        let key = self.expect_ident()?;

        if matches!(self.current().kind, TokenKind::Colon) {
            self.bump();
        } else if require_colon {
            return Err(self.error_here(&format!("expected ':' after property {key}")));
        }

        let value = self.parse_value()?;
        Ok(Property { key, value })
    }

    fn parse_value(&mut self) -> Result<Value, AdfError> {
        match &self.current().kind {
            TokenKind::Minus => {
                self.bump();

                match self.current().kind.clone() {
                    TokenKind::Number(number) => {
                        self.bump();
                        Ok(Value::Number(-number))
                    }
                    _ => Err(self.error_here("expected a number after '-'")),
                }
            }
            TokenKind::Number(number) => {
                let number = *number;
                self.bump();
                Ok(Value::Number(number))
            }
            TokenKind::String(value) => {
                let value = value.clone();
                self.bump();
                Ok(Value::String(value))
            }
            TokenKind::Ident(value) if value == "true" => {
                self.bump();
                Ok(Value::Bool(true))
            }
            TokenKind::Ident(value) if value == "false" => {
                self.bump();
                Ok(Value::Bool(false))
            }
            TokenKind::Ident(value) => {
                let value = value.clone();
                self.bump();
                Ok(Value::Ident(value))
            }
            TokenKind::LBracket => self.parse_vector(),
            _ => Err(self.error_here("expected a value")),
        }
    }

    fn parse_vector(&mut self) -> Result<Value, AdfError> {
        self.expect(TokenKind::LBracket, "expected '[' to start a vector")?;
        let mut values = Vec::new();

        while !matches!(self.current().kind, TokenKind::RBracket | TokenKind::Eof) {
            if matches!(self.current().kind, TokenKind::Comma) {
                self.bump();
                continue;
            }

            values.push(self.parse_value()?);
        }

        self.expect(TokenKind::RBracket, "expected ']' after vector")?;
        Ok(Value::Vector(values))
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<(), AdfError> {
        match &self.current().kind {
            TokenKind::Ident(actual) if actual == expected => {
                self.bump();
                Ok(())
            }
            _ => Err(self.error_here(&format!("expected keyword {expected:?}"))),
        }
    }

    fn expect_ident(&mut self) -> Result<String, AdfError> {
        match &self.current().kind {
            TokenKind::Ident(value) => {
                let value = value.clone();
                self.bump();
                Ok(value)
            }
            _ => Err(self.error_here("expected an identifier")),
        }
    }

    fn expect_string(&mut self) -> Result<String, AdfError> {
        match &self.current().kind {
            TokenKind::String(value) => {
                let value = value.clone();
                self.bump();
                Ok(value)
            }
            _ => Err(self.error_here("expected a string literal")),
        }
    }

    fn expect_u32(&mut self) -> Result<u32, AdfError> {
        match &self.current().kind {
            TokenKind::Number(number) if number.fract() == 0.0 && *number >= 0.0 => {
                let value = *number as u32;
                self.bump();
                Ok(value)
            }
            _ => Err(self.error_here("expected an unsigned integer")),
        }
    }

    fn expect(&mut self, expected: TokenKind, message: &str) -> Result<(), AdfError> {
        if self.same_variant(&self.current().kind, &expected) {
            self.bump();
            Ok(())
        } else {
            Err(self.error_here(message))
        }
    }

    fn same_variant(&self, left: &TokenKind, right: &TokenKind) -> bool {
        std::mem::discriminant(left) == std::mem::discriminant(right)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    fn bump(&mut self) {
        if !self.is_eof() {
            self.index += 1;
        }
    }

    fn is_eof(&self) -> bool {
        matches!(self.current().kind, TokenKind::Eof)
    }

    fn error_here(&self, message: &str) -> AdfError {
        let token = self.current();
        AdfError::Parse(format!(
            "{message} at line {}, column {}",
            token.line, token.column
        ))
    }
}

fn supported_versions_string() -> String {
    SUPPORTED_ADF_VERSIONS
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

struct CheckingParser<'a> {
    lexer: Lexer<'a>,
    current: Token,
}

impl<'a> CheckingParser<'a> {
    fn new(mut lexer: Lexer<'a>) -> Result<Self, AdfError> {
        let current = lexer.next_token()?;
        Ok(Self { lexer, current })
    }

    fn check_document(&mut self) -> Result<AdfCheckSummary, AdfError> {
        self.expect_keyword("adf")?;
        let version = self.expect_u32()?;

        if !SUPPORTED_ADF_VERSIONS.contains(&version) {
            return Err(self.error_here(&format!(
                "unsupported ADF version {} (supported: {})",
                version,
                supported_versions_string()
            )));
        }

        self.expect(TokenKind::Semicolon, "expected ';' after ADF version")?;

        let mut material_imports = 0usize;
        let mut edits = 0usize;

        while !self.is_eof() {
            match &self.current.kind {
                TokenKind::At => {
                    self.check_legacy_material_import()?;
                    material_imports += 1;
                }
                TokenKind::Ident(ident) if ident == "use" => {
                    self.check_use_material_import()?;
                    material_imports += 1;
                }
                TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Question => {
                    self.check_edit()?;
                    edits += 1;
                }
                _ => return Err(self.error_here("expected a material import or edit statement")),
            }
        }

        Ok(AdfCheckSummary {
            version,
            material_imports,
            edits,
        })
    }

    fn check_use_material_import(&mut self) -> Result<(), AdfError> {
        self.expect_keyword("use")?;
        self.expect_keyword("material")?;
        self.expect_ident()?;
        self.expect_keyword("from")?;
        self.expect_string()?;
        self.expect(TokenKind::Semicolon, "expected ';' after material import")
    }

    fn check_legacy_material_import(&mut self) -> Result<(), AdfError> {
        self.expect(TokenKind::At, "expected '@material'")?;
        self.expect_keyword("material")?;
        self.expect_string()?;
        self.expect(TokenKind::Semicolon, "expected ';' after material import")
    }

    fn check_edit(&mut self) -> Result<(), AdfError> {
        match self.current.kind {
            TokenKind::Plus | TokenKind::Minus | TokenKind::Caret | TokenKind::Question => {
                self.bump()?;
            }
            _ => return Err(self.error_here("expected an edit operation")),
        }

        self.expect_ident()?;
        self.expect(TokenKind::LParen, "expected '(' after primitive name")?;
        self.check_property_list(TokenKind::RParen, true)?;
        self.expect(TokenKind::RParen, "expected ')' after primitive properties")?;

        while !matches!(self.current.kind, TokenKind::Semicolon | TokenKind::Eof) {
            if matches!(self.current.kind, TokenKind::Comma) {
                self.bump()?;
                continue;
            }

            self.check_property(false)?;
        }

        self.expect(TokenKind::Semicolon, "expected ';' after edit")
    }

    fn check_property_list(
        &mut self,
        end: TokenKind,
        require_colon: bool,
    ) -> Result<(), AdfError> {
        while !same_variant(&self.current.kind, &end) {
            if matches!(self.current.kind, TokenKind::Comma) {
                self.bump()?;
                continue;
            }

            self.check_property(require_colon)?;
        }

        Ok(())
    }

    fn check_property(&mut self, require_colon: bool) -> Result<(), AdfError> {
        let key = self.expect_ident()?;

        if matches!(self.current.kind, TokenKind::Colon) {
            self.bump()?;
        } else if require_colon {
            return Err(self.error_here(&format!("expected ':' after property {key}")));
        }

        self.check_value()
    }

    fn check_value(&mut self) -> Result<(), AdfError> {
        match self.current.kind.clone() {
            TokenKind::Minus => {
                self.bump()?;

                match self.current.kind {
                    TokenKind::Number(_) => self.bump(),
                    _ => Err(self.error_here("expected a number after '-'")),
                }
            }
            TokenKind::Number(_) | TokenKind::String(_) => self.bump(),
            TokenKind::Ident(ref value) if value == "true" || value == "false" => self.bump(),
            TokenKind::Ident(_) => self.bump(),
            TokenKind::LBracket => self.check_vector(),
            _ => Err(self.error_here("expected a value")),
        }
    }

    fn check_vector(&mut self) -> Result<(), AdfError> {
        self.expect(TokenKind::LBracket, "expected '[' to start a vector")?;

        while !matches!(self.current.kind, TokenKind::RBracket | TokenKind::Eof) {
            if matches!(self.current.kind, TokenKind::Comma) {
                self.bump()?;
                continue;
            }

            self.check_value()?;
        }

        self.expect(TokenKind::RBracket, "expected ']' after vector")
    }

    fn expect_keyword(&mut self, expected: &str) -> Result<(), AdfError> {
        match &self.current.kind {
            TokenKind::Ident(actual) if actual == expected => self.bump(),
            _ => Err(self.error_here(&format!("expected keyword {expected:?}"))),
        }
    }

    fn expect_ident(&mut self) -> Result<String, AdfError> {
        match &self.current.kind {
            TokenKind::Ident(value) => {
                let value = value.clone();
                self.bump()?;
                Ok(value)
            }
            _ => Err(self.error_here("expected an identifier")),
        }
    }

    fn expect_string(&mut self) -> Result<String, AdfError> {
        match &self.current.kind {
            TokenKind::String(value) => {
                let value = value.clone();
                self.bump()?;
                Ok(value)
            }
            _ => Err(self.error_here("expected a string literal")),
        }
    }

    fn expect_u32(&mut self) -> Result<u32, AdfError> {
        match self.current.kind {
            TokenKind::Number(number) if number.fract() == 0.0 && number >= 0.0 => {
                let value = number as u32;
                self.bump()?;
                Ok(value)
            }
            _ => Err(self.error_here("expected an unsigned integer")),
        }
    }

    fn expect(&mut self, expected: TokenKind, message: &str) -> Result<(), AdfError> {
        if same_variant(&self.current.kind, &expected) {
            self.bump()
        } else {
            Err(self.error_here(message))
        }
    }

    fn bump(&mut self) -> Result<(), AdfError> {
        self.current = self.lexer.next_token()?;
        Ok(())
    }

    fn is_eof(&self) -> bool {
        matches!(self.current.kind, TokenKind::Eof)
    }

    fn error_here(&self, message: &str) -> AdfError {
        AdfError::Parse(format!(
            "{message} at line {}, column {}",
            self.current.line, self.current.column
        ))
    }
}

fn same_variant(left: &TokenKind, right: &TokenKind) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

fn primitive_tag_to_name(tag: u8) -> Option<&'static str> {
    match tag {
        1 => Some("sphere"),
        2 => Some("box"),
        3 => Some("cylinder"),
        4 => Some("capsule"),
        5 => Some("cone"),
        6 => Some("torus"),
        7 => Some("plane"),
        8 => Some("ellipsoid"),
        9 => Some("cube"),
        _ => None,
    }
}

fn primitive_name_to_tag(name: &str) -> Option<u8> {
    match name {
        "sphere" => Some(1),
        "box" => Some(2),
        "cylinder" => Some(3),
        "capsule" => Some(4),
        "cone" => Some(5),
        "torus" => Some(6),
        "plane" => Some(7),
        "ellipsoid" => Some(8),
        "cube" => Some(9),
        _ => None,
    }
}

fn property_tag_to_name(tag: u8) -> Option<&'static str> {
    match tag {
        1 => Some("radius"),
        2 => Some("size"),
        3 => Some("at"),
        4 => Some("mat"),
        5 => Some("k"),
        6 => Some("rot"),
        7 => Some("scale"),
        8 => Some("height"),
        9 => Some("width"),
        10 => Some("depth"),
        11 => Some("center"),
        12 => Some("axis"),
        13 => Some("normal"),
        14 => Some("inner_radius"),
        15 => Some("outer_radius"),
        _ => None,
    }
}

fn property_name_to_tag(name: &str) -> Option<u8> {
    match name {
        "radius" => Some(1),
        "size" => Some(2),
        "at" => Some(3),
        "mat" => Some(4),
        "k" => Some(5),
        "rot" => Some(6),
        "scale" => Some(7),
        "height" => Some(8),
        "width" => Some(9),
        "depth" => Some(10),
        "center" => Some(11),
        "axis" => Some(12),
        "normal" => Some(13),
        "inner_radius" => Some(14),
        "outer_radius" => Some(15),
        _ => None,
    }
}

struct BinaryReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> BinaryReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn parse_document(mut self) -> Result<AdfDocument, AdfError> {
        let (version, statement_count) = self.read_header()?;
        let mut statements = Vec::with_capacity(statement_count as usize);

        for _ in 0..statement_count {
            statements.push(self.read_statement()?);
        }

        self.ensure_exhausted()?;

        Ok(AdfDocument { version, statements })
    }

    fn check_document(mut self) -> Result<AdfCheckSummary, AdfError> {
        let (version, statement_count) = self.read_header()?;
        let mut material_imports = 0usize;
        let mut edits = 0usize;

        for _ in 0..statement_count {
            match self.read_u8()? {
                0 => {
                    self.skip_material_import()?;
                    material_imports += 1;
                }
                1 => {
                    self.skip_edit()?;
                    edits += 1;
                }
                tag => {
                    return Err(AdfError::Binary(format!(
                        "unknown ADFB statement tag {} at byte {}",
                        tag,
                        self.offset.saturating_sub(1)
                    )));
                }
            }
        }

        self.ensure_exhausted()?;

        Ok(AdfCheckSummary {
            version,
            material_imports,
            edits,
        })
    }

    fn read_header(&mut self) -> Result<(u32, u32), AdfError> {
        let magic = self.read_exact(4)?;

        if magic != ADFB_MAGIC {
            return Err(AdfError::Binary(format!(
                "invalid ADFB magic {:?}, expected {:?}",
                magic, ADFB_MAGIC
            )));
        }

        let version = self.read_u32()?;

        if !SUPPORTED_ADF_VERSIONS.contains(&version) {
            return Err(AdfError::Binary(format!(
                "unsupported ADF version {} (supported: {})",
                version,
                supported_versions_string()
            )));
        }

        let statement_count = self.read_u32()?;
        Ok((version, statement_count))
    }

    fn read_statement(&mut self) -> Result<AdfStatement, AdfError> {
        match self.read_u8()? {
            0 => self.read_material_import().map(AdfStatement::MaterialImport),
            1 => self.read_edit().map(AdfStatement::Edit),
            tag => Err(AdfError::Binary(format!(
                "unknown ADFB statement tag {} at byte {}",
                tag,
                self.offset.saturating_sub(1)
            ))),
        }
    }

    fn read_material_import(&mut self) -> Result<MaterialImport, AdfError> {
        let syntax = match self.read_u8()? {
            0 => MaterialImportSyntax::Use,
            1 => MaterialImportSyntax::LegacyAtMaterial,
            tag => {
                return Err(AdfError::Binary(format!(
                    "unknown material import syntax tag {} at byte {}",
                    tag,
                    self.offset.saturating_sub(1)
                )));
            }
        };

        let alias = match self.read_u8()? {
            0 => None,
            1 => Some(self.read_string()?),
            flag => {
                return Err(AdfError::Binary(format!(
                    "invalid alias flag {} at byte {}",
                    flag,
                    self.offset.saturating_sub(1)
                )));
            }
        };

        let path = self.read_string()?;

        Ok(MaterialImport { alias, path, syntax })
    }

    fn read_edit(&mut self) -> Result<Edit, AdfError> {
        let operation = match self.read_u8()? {
            0 => Operation::Union,
            1 => Operation::Difference,
            2 => Operation::Intersection,
            3 => Operation::Paint,
            tag => {
                return Err(AdfError::Binary(format!(
                    "unknown operation tag {} at byte {}",
                    tag,
                    self.offset.saturating_sub(1)
                )));
            }
        };

        let primitive = self.read_tagged_name(
            "primitive",
            primitive_tag_to_name,
        )?;
        let property_count = self.read_u32()? as usize;
        let mut properties = Vec::with_capacity(property_count);

        for _ in 0..property_count {
            properties.push(self.read_property()?);
        }

        Ok(Edit {
            operation,
            primitive,
            properties,
        })
    }

    fn read_property(&mut self) -> Result<Property, AdfError> {
        let key = self.read_tagged_name("property", property_tag_to_name)?;
        let value = self.read_value()?;
        Ok(Property { key, value })
    }

    fn read_value(&mut self) -> Result<Value, AdfError> {
        match self.read_u8()? {
            0 => Ok(Value::Number(self.read_f64()?)),
            1 => Ok(Value::String(self.read_string()?)),
            2 => Ok(Value::Ident(self.read_string()?)),
            3 => match self.read_u8()? {
                0 => Ok(Value::Bool(false)),
                1 => Ok(Value::Bool(true)),
                flag => Err(AdfError::Binary(format!(
                    "invalid boolean value {} at byte {}",
                    flag,
                    self.offset.saturating_sub(1)
                ))),
            },
            4 => {
                let value_count = self.read_u32()? as usize;
                let mut values = Vec::with_capacity(value_count);

                for _ in 0..value_count {
                    values.push(self.read_value()?);
                }

                Ok(Value::Vector(values))
            }
            tag => Err(AdfError::Binary(format!(
                "unknown value tag {} at byte {}",
                tag,
                self.offset.saturating_sub(1)
            ))),
        }
    }

    fn skip_material_import(&mut self) -> Result<(), AdfError> {
        match self.read_u8()? {
            0 | 1 => {}
            tag => {
                return Err(AdfError::Binary(format!(
                    "unknown material import syntax tag {} at byte {}",
                    tag,
                    self.offset.saturating_sub(1)
                )));
            }
        }

        match self.read_u8()? {
            0 => {}
            1 => {
                self.skip_string()?;
            }
            flag => {
                return Err(AdfError::Binary(format!(
                    "invalid alias flag {} at byte {}",
                    flag,
                    self.offset.saturating_sub(1)
                )));
            }
        }

        self.skip_string()
    }

    fn skip_edit(&mut self) -> Result<(), AdfError> {
        match self.read_u8()? {
            0..=3 => {}
            tag => {
                return Err(AdfError::Binary(format!(
                    "unknown operation tag {} at byte {}",
                    tag,
                    self.offset.saturating_sub(1)
                )));
            }
        }

        self.skip_tagged_name("primitive", primitive_tag_to_name)?;
        let property_count = self.read_u32()? as usize;

        for _ in 0..property_count {
            self.skip_tagged_name("property", property_tag_to_name)?;
            self.skip_value()?;
        }

        Ok(())
    }

    fn skip_value(&mut self) -> Result<(), AdfError> {
        match self.read_u8()? {
            0 => {
                self.read_f64()?;
                Ok(())
            }
            1 | 2 => self.skip_string(),
            3 => match self.read_u8()? {
                0 | 1 => Ok(()),
                flag => Err(AdfError::Binary(format!(
                    "invalid boolean value {} at byte {}",
                    flag,
                    self.offset.saturating_sub(1)
                ))),
            },
            4 => {
                let value_count = self.read_u32()? as usize;

                for _ in 0..value_count {
                    self.skip_value()?;
                }

                Ok(())
            }
            tag => Err(AdfError::Binary(format!(
                "unknown value tag {} at byte {}",
                tag,
                self.offset.saturating_sub(1)
            ))),
        }
    }

    fn skip_string(&mut self) -> Result<(), AdfError> {
        let length = self.read_u32()? as usize;
        self.read_exact(length)?;
        Ok(())
    }

    fn read_tagged_name(
        &mut self,
        kind: &str,
        known_name: fn(u8) -> Option<&'static str>,
    ) -> Result<String, AdfError> {
        let tag = self.read_u8()?;

        if tag == CUSTOM_TAG {
            return self.read_string();
        }

        known_name(tag)
            .map(str::to_string)
            .ok_or_else(|| {
                AdfError::Binary(format!(
                    "unknown {} tag {} at byte {}",
                    kind,
                    tag,
                    self.offset.saturating_sub(1)
                ))
            })
    }

    fn skip_tagged_name(
        &mut self,
        kind: &str,
        known_name: fn(u8) -> Option<&'static str>,
    ) -> Result<(), AdfError> {
        let tag = self.read_u8()?;

        if tag == CUSTOM_TAG {
            return self.skip_string();
        }

        if known_name(tag).is_some() {
            Ok(())
        } else {
            Err(AdfError::Binary(format!(
                "unknown {} tag {} at byte {}",
                kind,
                tag,
                self.offset.saturating_sub(1)
            )))
        }
    }

    fn read_string(&mut self) -> Result<String, AdfError> {
        let length = self.read_u32()? as usize;
        let bytes = self.read_exact(length)?;
        let value = std::str::from_utf8(bytes)?;
        Ok(value.to_string())
    }

    fn read_u8(&mut self) -> Result<u8, AdfError> {
        let bytes = self.read_exact(1)?;
        Ok(bytes[0])
    }

    fn read_u32(&mut self) -> Result<u32, AdfError> {
        let bytes = self.read_exact(4)?;
        let mut array = [0u8; 4];
        array.copy_from_slice(bytes);
        Ok(u32::from_le_bytes(array))
    }

    fn read_f64(&mut self) -> Result<f64, AdfError> {
        let bytes = self.read_exact(8)?;
        let mut array = [0u8; 8];
        array.copy_from_slice(bytes);
        Ok(f64::from_le_bytes(array))
    }

    fn read_exact(&mut self, length: usize) -> Result<&'a [u8], AdfError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| AdfError::Binary("ADFB offset overflow".to_string()))?;

        if end > self.bytes.len() {
            return Err(AdfError::Binary(format!(
                "unexpected end of ADFB input at byte {}, needed {} more bytes",
                self.offset,
                end - self.bytes.len()
            )));
        }

        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn ensure_exhausted(&self) -> Result<(), AdfError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(AdfError::Binary(format!(
                "ADFB has {} trailing bytes",
                self.bytes.len() - self.offset
            )))
        }
    }
}

struct BinaryWriter {
    bytes: Vec<u8>,
}

impl BinaryWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn encode_document(mut self, document: &AdfDocument) -> Result<Vec<u8>, AdfError> {
        if !SUPPORTED_ADF_VERSIONS.contains(&document.version) {
            return Err(AdfError::Binary(format!(
                "cannot encode unsupported ADF version {} (supported: {})",
                document.version,
                supported_versions_string()
            )));
        }

        self.bytes.extend_from_slice(ADFB_MAGIC);
        self.write_u32(document.version);
        self.write_len(document.statements.len(), "statement count")?;

        for statement in &document.statements {
            self.write_statement(statement)?;
        }

        Ok(self.bytes)
    }

    fn write_statement(&mut self, statement: &AdfStatement) -> Result<(), AdfError> {
        match statement {
            AdfStatement::MaterialImport(import) => {
                self.write_u8(0);
                self.write_material_import(import)
            }
            AdfStatement::Edit(edit) => {
                self.write_u8(1);
                self.write_edit(edit)
            }
        }
    }

    fn write_material_import(&mut self, import: &MaterialImport) -> Result<(), AdfError> {
        self.write_u8(match import.syntax {
            MaterialImportSyntax::Use => 0,
            MaterialImportSyntax::LegacyAtMaterial => 1,
        });

        match &import.alias {
            Some(alias) => {
                self.write_u8(1);
                self.write_string(alias)?;
            }
            None => self.write_u8(0),
        }

        self.write_string(&import.path)
    }

    fn write_edit(&mut self, edit: &Edit) -> Result<(), AdfError> {
        self.write_u8(match edit.operation {
            Operation::Union => 0,
            Operation::Difference => 1,
            Operation::Intersection => 2,
            Operation::Paint => 3,
        });

        self.write_tagged_name(&edit.primitive, primitive_name_to_tag)?;
        self.write_len(edit.properties.len(), "property count")?;

        for property in &edit.properties {
            self.write_property(property)?;
        }

        Ok(())
    }

    fn write_property(&mut self, property: &Property) -> Result<(), AdfError> {
        self.write_tagged_name(&property.key, property_name_to_tag)?;
        self.write_value(&property.value)
    }

    fn write_value(&mut self, value: &Value) -> Result<(), AdfError> {
        match value {
            Value::Number(number) => {
                self.write_u8(0);
                self.write_f64(*number);
            }
            Value::String(value) => {
                self.write_u8(1);
                self.write_string(value)?;
            }
            Value::Ident(value) => {
                self.write_u8(2);
                self.write_string(value)?;
            }
            Value::Bool(value) => {
                self.write_u8(3);
                self.write_u8(u8::from(*value));
            }
            Value::Vector(values) => {
                self.write_u8(4);
                self.write_len(values.len(), "vector length")?;

                for value in values {
                    self.write_value(value)?;
                }
            }
        }

        Ok(())
    }

    fn write_len(&mut self, value: usize, what: &str) -> Result<(), AdfError> {
        let value = u32::try_from(value).map_err(|_| {
            AdfError::Binary(format!("{what} exceeds u32::MAX in ADFB encoding"))
        })?;
        self.write_u32(value);
        Ok(())
    }

    fn write_string(&mut self, value: &str) -> Result<(), AdfError> {
        self.write_len(value.len(), "string length")?;
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn write_tagged_name(
        &mut self,
        value: &str,
        known_tag: fn(&str) -> Option<u8>,
    ) -> Result<(), AdfError> {
        match known_tag(value) {
            Some(tag) => {
                self.write_u8(tag);
                Ok(())
            }
            None => {
                self.write_u8(CUSTOM_TAG);
                self.write_string(value)
            }
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn write_f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_document() {
        let source = r#"
            adf 1;
            use material body from "body.amat";
            + sphere(radius: 1.0, at: [0, 2, 0], mat: body, k: 0.2);
            ? sphere(radius: 0.35)
                at [0.25, 2.1, 0.6]
                mat paint_red;
        "#;

        let document = parse(source).expect("parser should accept the sample");

        assert_eq!(document.version, 1);
        assert_eq!(document.statements.len(), 3);
    }

    #[test]
    fn parses_legacy_material_import() {
        let source = r#"
            adf 1;
            @material "^.amat";
        "#;

        let document = parse(source).expect("legacy import should parse");
        assert_eq!(document.statements.len(), 1);
    }

    #[test]
    fn rejects_unsupported_version() {
        let source = r#"
            adf 999;
            @material "^.amat";
        "#;

        let error = parse(source).expect_err("unsupported version should fail");
        assert!(error
            .to_string()
            .contains("unsupported ADF version 999"));
    }

    #[test]
    fn binary_roundtrip_preserves_document() {
        let source = r#"
            adf 1;
            use material body from "body.amat";
            + sphere(radius: 1.0, at: [0, 2, 0], mat: body, k: 0.2);
            ? sphere(radius: 0.35)
                at [0.25, 2.1, 0.6]
                mat paint_red;
        "#;

        let document = parse(source).expect("text ADF should parse");
        let bytes = encode_binary(&document).expect("document should encode");
        let decoded = parse_binary(&bytes).expect("binary ADF should decode");

        assert_eq!(document, decoded);
    }

    #[test]
    fn check_bytes_detects_binary_adfb() {
        let source = r#"
            adf 1;
            use material body from "body.amat";
            + sphere(radius: 1.0, at: [0, 2, 0], mat: body, k: 0.2);
        "#;

        let document = parse(source).expect("text ADF should parse");
        let bytes = encode_binary(&document).expect("document should encode");
        let (encoding, summary) = check_bytes(&bytes).expect("binary ADF should validate");

        assert_eq!(encoding, AdfEncoding::Binary);
        assert_eq!(summary.version, 1);
        assert_eq!(summary.material_imports, 1);
        assert_eq!(summary.edits, 1);
    }
}
