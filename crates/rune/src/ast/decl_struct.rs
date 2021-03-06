use crate::ast;
use crate::{IntoTokens, MacroContext, Parse, ParseError, Parser, TokenStream};
use runestick::Span;

/// A struct declaration.
#[derive(Debug, Clone)]
pub struct DeclStruct {
    /// The `struct` keyword.
    pub struct_: ast::Struct,
    /// The identifier of the struct declaration.
    pub ident: ast::Ident,
    /// The body of the struct.
    pub body: DeclStructBody,
}

impl DeclStruct {
    /// Get the span for the declaration.
    pub fn span(&self) -> Span {
        let start = self.struct_.span();

        match &self.body {
            DeclStructBody::EmptyBody(..) => start,
            DeclStructBody::TupleBody(body) => start.join(body.span()),
            DeclStructBody::StructBody(body) => start.join(body.span()),
        }
    }

    /// Indicates if the declaration needs a semi-colon or not.
    pub fn needs_semi_colon(&self) -> bool {
        matches!(&self.body, DeclStructBody::EmptyBody(..))
    }
}

/// Parse implementation for a struct.
///
/// # Examples
///
/// ```rust
/// use rune::{parse_all, ast};
///
/// parse_all::<ast::DeclStruct>("struct Foo").unwrap();
/// parse_all::<ast::DeclStruct>("struct Foo ( a, b, c )").unwrap();
/// parse_all::<ast::DeclStruct>("struct Foo { a, b, c }").unwrap();
/// ```
impl Parse for DeclStruct {
    fn parse(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        Ok(Self {
            struct_: parser.parse()?,
            ident: parser.parse()?,
            body: parser.parse()?,
        })
    }
}

/// A struct declaration.
#[derive(Debug, Clone)]
pub enum DeclStructBody {
    /// An empty struct declaration.
    EmptyBody(EmptyBody),
    /// A tuple struct body.
    TupleBody(TupleBody),
    /// A regular struct body.
    StructBody(StructBody),
}

/// Parse implementation for a struct body.
///
/// # Examples
///
/// ```rust
/// use rune::{parse_all, ast};
///
/// parse_all::<ast::DeclStructBody>("").unwrap();
/// parse_all::<ast::DeclStructBody>("( a, b, c )").unwrap();
/// parse_all::<ast::DeclStructBody>("{ a, b, c }").unwrap();
/// ```
impl Parse for DeclStructBody {
    fn parse(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        let token = parser.token_peek()?;

        Ok(match token.map(|t| t.kind) {
            Some(ast::Kind::Open(ast::Delimiter::Parenthesis)) => Self::TupleBody(parser.parse()?),
            Some(ast::Kind::Open(ast::Delimiter::Brace)) => Self::StructBody(parser.parse()?),
            _ => Self::EmptyBody(parser.parse()?),
        })
    }
}

impl IntoTokens for &DeclStructBody {
    fn into_tokens(self, context: &mut MacroContext, stream: &mut TokenStream) {
        match self {
            DeclStructBody::EmptyBody(..) => (),
            DeclStructBody::TupleBody(body) => {
                body.into_tokens(context, stream);
            }
            DeclStructBody::StructBody(body) => {
                body.into_tokens(context, stream);
            }
        }
    }
}

/// A variant declaration that is empty..
#[derive(Debug, Clone)]
pub struct EmptyBody {}

/// Parse implementation for an empty struct body.
///
/// # Examples
///
/// ```rust
/// use rune::{parse_all, ast};
///
/// parse_all::<ast::EmptyBody>("Foo").unwrap();
/// ```
impl Parse for EmptyBody {
    fn parse(_: &mut Parser<'_>) -> Result<Self, ParseError> {
        Ok(Self {})
    }
}

/// A variant declaration.
#[derive(Debug, Clone)]
pub struct TupleBody {
    /// The opening paren.
    pub open: ast::OpenParen,
    /// Fields in the variant.
    pub fields: Vec<(ast::Ident, Option<ast::Comma>)>,
    /// The close paren.
    pub close: ast::CloseParen,
}

impl TupleBody {
    /// Get the span for the tuple body.
    pub fn span(&self) -> Span {
        self.open.span().join(self.close.span())
    }
}

/// Parse implementation for a struct body.
///
/// # Examples
///
/// ```rust
/// use rune::{parse_all, ast};
///
/// parse_all::<ast::TupleBody>("( a, b, c )").unwrap();
/// ```
impl Parse for TupleBody {
    fn parse(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        let open = parser.parse()?;

        let mut fields = Vec::new();

        while !parser.peek::<ast::CloseParen>()? {
            let field = parser.parse()?;

            let comma = if parser.peek::<ast::Comma>()? {
                Some(parser.parse()?)
            } else {
                None
            };

            let done = comma.is_none();

            fields.push((field, comma));

            if done {
                break;
            }
        }

        let close = parser.parse()?;

        Ok(Self {
            open,
            fields,
            close,
        })
    }
}

impl IntoTokens for &TupleBody {
    fn into_tokens(self, context: &mut MacroContext, stream: &mut TokenStream) {
        self.open.into_tokens(context, stream);

        for (field, comma) in &self.fields {
            field.into_tokens(context, stream);
            comma.into_tokens(context, stream);
        }

        self.close.into_tokens(context, stream);
    }
}

/// A variant declaration.
#[derive(Debug, Clone)]
pub struct StructBody {
    /// The opening brace.
    pub open: ast::OpenBrace,
    /// Fields in the variant.
    pub fields: Vec<(ast::Ident, Option<ast::Comma>)>,
    /// The close brace.
    pub close: ast::CloseBrace,
}

impl StructBody {
    /// Get the span for the tuple body.
    pub fn span(&self) -> Span {
        self.open.span().join(self.close.span())
    }
}

/// Parse implementation for a struct body.
///
/// # Examples
///
/// ```rust
/// use rune::{parse_all, ast};
///
/// parse_all::<ast::StructBody>("{ a, b, c }").unwrap();
/// ```
impl Parse for StructBody {
    fn parse(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        let open = parser.parse()?;

        let mut fields = Vec::new();

        while !parser.peek::<ast::CloseBrace>()? {
            let field = parser.parse()?;

            let comma = if parser.peek::<ast::Comma>()? {
                Some(parser.parse()?)
            } else {
                None
            };

            let done = comma.is_none();

            fields.push((field, comma));

            if done {
                break;
            }
        }

        let close = parser.parse()?;

        Ok(Self {
            open,
            fields,
            close,
        })
    }
}

impl IntoTokens for &StructBody {
    fn into_tokens(self, context: &mut MacroContext, stream: &mut TokenStream) {
        self.open.into_tokens(context, stream);

        for (field, comma) in &self.fields {
            field.into_tokens(context, stream);
            comma.into_tokens(context, stream);
        }

        self.close.into_tokens(context, stream);
    }
}
