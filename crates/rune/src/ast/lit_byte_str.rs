use crate::ast;
use crate::error::ParseError;
use crate::parser::Parser;
use crate::traits::{Parse, Resolve};
use runestick::{Source, Span};
use std::borrow::Cow;

/// A string literal.
#[derive(Debug, Clone)]
pub struct LitByteStr {
    /// The token corresponding to the literal.
    token: ast::Token,
    /// If the string literal is escaped.
    escaped: bool,
}

impl LitByteStr {
    /// Access the span of the expression.
    pub fn span(&self) -> Span {
        self.token.span
    }
}

impl LitByteStr {
    fn parse_escaped(&self, span: Span, source: &str) -> Result<Vec<u8>, ParseError> {
        let mut buffer = Vec::with_capacity(source.len());

        let mut it = source
            .char_indices()
            .map(|(n, c)| (span.start + n, c))
            .peekable();

        while let Some((n, c)) = it.next() {
            buffer.push(match c {
                '\\' => ast::utils::parse_byte_escape(span.with_start(n), &mut it)?,
                c => c as u8,
            });
        }

        Ok(buffer)
    }
}

impl<'a> Resolve<'a> for LitByteStr {
    type Output = Cow<'a, [u8]>;

    fn resolve(&self, source: &'a Source) -> Result<Cow<'a, [u8]>, ParseError> {
        let span = self.token.span.trim_start(2).trim_end(1);
        let string = source
            .source(span)
            .ok_or_else(|| ParseError::BadSlice { span })?;

        Ok(if self.escaped {
            Cow::Owned(self.parse_escaped(span, string)?)
        } else {
            Cow::Borrowed(string.as_bytes())
        })
    }
}

/// Parse a string literal.
///
/// # Examples
///
/// ```rust
/// use rune::{parse_all, ast};
///
/// let s = parse_all::<ast::LitByteStr>("b\"hello world\"").unwrap();
/// let s = parse_all::<ast::LitByteStr>("b\"hello\\nworld\"").unwrap();
/// ```
impl Parse for LitByteStr {
    fn parse(parser: &mut Parser<'_>) -> Result<Self, ParseError> {
        let token = parser.token_next()?;

        match token.kind {
            ast::Kind::LitByteStr { escaped } => Ok(Self { token, escaped }),
            _ => Err(ParseError::ExpectedString {
                actual: token.kind,
                span: token.span,
            }),
        }
    }
}
