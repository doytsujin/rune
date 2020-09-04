use runestick::Span;

/// Compilation warning.
#[derive(Debug, Clone, Copy)]
pub enum Warning {
    /// Item identified by the span is not used.
    NotUsed {
        /// The span that is not used.
        span: Span,
        /// The context in which the value was not used.
        context: Option<Span>,
    },
    /// Warning that an unconditional let pattern will panic if it doesn't
    /// match.
    LetPatternMightPanic {
        /// The span of the pattern.
        span: Span,
        /// The context in which it is used.
        context: Option<Span>,
    },
    /// Encountered a template string without an expansion.
    TemplateWithoutExpansions {
        /// Span that caused the error.
        span: Span,
        /// The context in which it is used.
        context: Option<Span>,
    },
    /// Suggestion that call parameters could be removed.
    RemoveTupleCallParams {
        /// The span of the call.
        span: Span,
        /// The span of the variant being built.
        variant: Span,
        /// The context in which it is used.
        context: Option<Span>,
    },
    /// An unecessary semi-colon is used.
    UnecessarySemiColon {
        /// Span where the semi-colon is.
        span: Span,
    },
}
/// Compilation warnings.
#[derive(Debug, Clone, Default)]
pub struct Warnings {
    warnings: Vec<Warning>,
}

impl Warnings {
    /// Construct a new collection of compilation warnings.
    pub(crate) fn new() -> Self {
        Self {
            warnings: Vec::new(),
        }
    }

    /// Indicate if there are warnings or not.
    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Construct a warning indicating that the item identified by the span is
    /// not used.
    pub(crate) fn not_used(&mut self, span: Span, context: Option<Span>) {
        self.warnings.push(Warning::NotUsed { span, context });
    }

    /// Indicate that a pattern might panic.
    pub(crate) fn let_pattern_might_panic(&mut self, span: Span, context: Option<Span>) {
        self.warnings
            .push(Warning::LetPatternMightPanic { span, context });
    }

    /// Indicate that we encountered a template string without any expansion groups.
    pub(crate) fn template_without_expansions(&mut self, span: Span, context: Option<Span>) {
        self.warnings
            .push(Warning::TemplateWithoutExpansions { span, context });
    }

    /// Remove call parenthesis.
    pub(crate) fn remove_tuple_call_parens(
        &mut self,
        span: Span,
        variant: Span,
        context: Option<Span>,
    ) {
        self.warnings.push(Warning::RemoveTupleCallParams {
            span,
            variant,
            context,
        });
    }

    /// Indicate an unecessary semi colon.
    pub(crate) fn uneccessary_semi_colon(&mut self, span: Span) {
        self.warnings.push(Warning::UnecessarySemiColon { span });
    }
}

impl IntoIterator for Warnings {
    type IntoIter = std::vec::IntoIter<Warning>;
    type Item = Warning;

    fn into_iter(self) -> Self::IntoIter {
        self.warnings.into_iter()
    }
}