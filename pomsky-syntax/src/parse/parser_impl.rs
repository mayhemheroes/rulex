use std::{borrow::Borrow, collections::HashSet};

use crate::{error::*, exprs::*, lexer::Token, warning::ParseWarningKind, Span};

use super::{helper, Parser};

type PResult<T> = Result<T, ParseError>;

impl<'i> Parser<'i> {
    pub(super) fn parse_modified(&mut self) -> PResult<Rule<'i>> {
        let mut stmts = Vec::new();

        loop {
            let Some(stmt) = self.parse_mode_modifier()?.try_or_else(|| self.parse_let())? else {
                break;
            };
            stmts.push(stmt);
        }

        self.recursion_start()?;
        let mut rule = self.parse_or()?;
        self.recursion_end();

        // TODO: This should not be part of the parser
        if stmts.len() > 1 {
            let mut set = HashSet::new();
            for (stmt, _) in &stmts {
                if let Stmt::Let(l) = stmt {
                    if set.contains(l.name()) {
                        return Err(ParseErrorKind::LetBindingExists.at(l.name_span));
                    }
                    set.insert(l.name());
                }
            }
        }

        let span_end = rule.span();
        for (stmt, span) in stmts.into_iter().rev() {
            rule = Rule::StmtExpr(Box::new(StmtExpr::new(stmt, rule, span.join(span_end))));
        }

        Ok(rule)
    }

    fn parse_mode_modifier(&mut self) -> PResult<Option<(Stmt<'i>, Span)>> {
        let stmt = if self.consume_reserved("enable") {
            Stmt::Enable(BooleanSetting::Lazy)
        } else if self.consume_reserved("disable") {
            Stmt::Disable(BooleanSetting::Lazy)
        } else {
            return Ok(None);
        };

        let span_start = self.last_span();
        self.expect_reserved("lazy")?;
        self.expect(Token::Semicolon)?;
        let span_end = self.last_span();

        Ok(Some((stmt, span_start.join(span_end))))
    }

    fn parse_let(&mut self) -> PResult<Option<(Stmt<'i>, Span)>> {
        if self.consume_reserved("let") {
            let span_start = self.last_span();
            let name_span = self.span();
            let name = self.expect_as(Token::Identifier).map_err(|e| {
                if self.is(Token::ReservedName) {
                    ParseErrorKind::KeywordAfterLet(self.source_at(self.span()).to_owned())
                        .at(e.span)
                } else {
                    e
                }
            })?;

            self.expect(Token::Equals)?;

            self.recursion_start()?;
            let rule = self.parse_or()?;
            self.recursion_end();

            self.expect(Token::Semicolon)
                .map_err(|p| ParseErrorKind::Expected("expression or `;`").at(p.span))?;
            let span_end = self.last_span();

            Ok(Some((Stmt::Let(Let::new(name, rule, name_span)), span_start.join(span_end))))
        } else {
            Ok(None)
        }
    }

    fn parse_or(&mut self) -> PResult<Rule<'i>> {
        let mut span = self.span();
        let leading_pipe = self.consume(Token::Pipe);

        let mut alts = Vec::new();
        if let Some(first_alt) = self.parse_sequence()? {
            alts.push(first_alt);

            while self.consume(Token::Pipe) {
                if let Some(next_alt) = self.parse_sequence()? {
                    span = span.join(next_alt.span());
                    alts.push(next_alt);
                } else {
                    return Err(ParseErrorKind::LonePipe.at(self.last_span()));
                }
            }

            if alts.len() == 1 {
                Ok(alts.pop().unwrap())
            } else {
                Ok(Alternation::new_expr(alts))
            }
        } else if leading_pipe {
            Err(ParseErrorKind::LonePipe.at(span))
        } else {
            Ok(Alternation::new_expr(alts))
        }
    }

    fn parse_sequence(&mut self) -> PResult<Option<Rule<'i>>> {
        let mut fixes = Vec::new();
        while let Some(fix) = self.parse_fixes()? {
            fixes.push(fix);
        }

        Ok(if fixes.is_empty() {
            None
        } else if fixes.len() == 1 {
            Some(fixes.pop().unwrap())
        } else {
            let start = fixes.first().map(Rule::span).unwrap_or_default();
            let end = fixes.last().map(Rule::span).unwrap_or_default();
            let span = start.join(end);

            Some(Rule::Group(Group::new(fixes, GroupKind::Implicit, span)))
        })
    }

    fn parse_fixes(&mut self) -> PResult<Option<Rule<'i>>> {
        let mut nots_span = self.span();
        let mut nots = 0usize;
        while self.consume(Token::Not) {
            nots += 1;
            nots_span = nots_span.join(self.last_span());
        }

        let Some(mut rule) = self.parse_lookaround()?.try_or_else(|| self.parse_repeated())? else {
            if nots == 0 {
               return Ok(None);
            } else {
               return Err(ParseErrorKind::Expected("expression").at(self.span()));
            }
        };

        match nots {
            0 => {}
            1 => rule.negate().map_err(|k| k.at(nots_span))?,
            _ => return Err(ParseErrorKind::UnallowedMultiNot(nots).at(nots_span)),
        }

        Ok(Some(rule))
    }

    fn parse_lookaround(&mut self) -> PResult<Option<Rule<'i>>> {
        let kind = if self.consume(Token::LookAhead) {
            LookaroundKind::Ahead
        } else if self.consume(Token::LookBehind) {
            LookaroundKind::Behind
        } else {
            return Ok(None);
        };
        let start_span = self.last_span();

        self.recursion_start()?;
        let rule = self.parse_modified()?;
        self.recursion_end();

        let span = rule.span();
        Ok(Some(Rule::Lookaround(Box::new(Lookaround::new(rule, kind, start_span.join(span))))))
    }

    /// Parse an atom expression with possibly multiple repetitions, e.g. `E
    /// {3,} lazy ?`.
    fn parse_repeated(&mut self) -> PResult<Option<Rule<'i>>> {
        if let Some(mut rule) = self.parse_atom()? {
            if let Some((kind, quantifier, span)) = self.parse_repetition()? {
                let span = rule.span().join(span);
                rule = Rule::Repetition(Box::new(Repetition::new(rule, kind, quantifier, span)));
            }

            Ok(Some(rule))
        } else {
            Ok(None)
        }
    }

    /// Parse a repetition that can follow an atom: `+`, `?`, `*`, `{x}`,
    /// `{x,}`, `{,x}` or `{x,y}` optionally followed by the `greedy` or
    /// `lazy` keyword. `x` and `y` are number literals.
    fn parse_repetition(&mut self) -> PResult<Option<(RepetitionKind, Quantifier, Span)>> {
        let start = self.span();

        let kind = if self.consume(Token::Plus) {
            RepetitionKind::one_inf()
        } else if self.consume(Token::Star) {
            RepetitionKind::zero_inf()
        } else if self.consume(Token::QuestionMark) {
            RepetitionKind::zero_one()
        } else if let Some(kind) = self.parse_repetition_braces()? {
            kind
        } else {
            return Ok(None);
        };

        let quantifier = if self.consume_reserved("greedy") {
            Quantifier::Greedy
        } else if self.consume_reserved("lazy") {
            Quantifier::Lazy
        } else {
            Quantifier::Default
        };

        let multi_span = self.span();
        if self.consume(Token::Plus) || self.consume(Token::Star) {
            return Err(ParseErrorKind::Repetition(RepetitionError::Multi).at(multi_span));
        } else if self.consume(Token::QuestionMark) {
            return Err(ParseErrorKind::Repetition(RepetitionError::QmSuffix).at(multi_span));
        } else if self.parse_repetition_braces()?.is_some() {
            return Err(ParseErrorKind::Repetition(RepetitionError::Multi)
                .at(multi_span.join(self.last_span())));
        }

        let end = self.last_span();
        Ok(Some((kind, quantifier, start.join(end))))
    }

    /// Parse `{2}`, `{2,}`, `{,2}` or `{2,5}`.
    fn parse_repetition_braces(&mut self) -> PResult<Option<RepetitionKind>> {
        if self.consume(Token::OpenBrace) {
            let num_start = self.span();

            // Both numbers and the comma are parsed optionally, then we check that one
            // of the allowed syntaxes is used: There must be at least one number, and if
            // there are two numbers, the comma is required. It also checks that the
            // numbers are in increasing order.
            let lower = self.consume_number::<u32>()?;
            let comma = self.consume(Token::Comma);
            let upper = self.consume_number::<u32>()?;

            let num_end = self.last_span();
            let num_span = num_start.join(num_end);

            let kind = match (lower, comma, upper) {
                (lower, true, upper) => (lower.unwrap_or(0), upper)
                    .try_into()
                    .map_err(|e| ParseErrorKind::Repetition(e).at(num_span))?,

                (Some(_), false, Some(_)) => {
                    return Err(ParseErrorKind::Expected("`}` or `,`").at(num_end))
                }
                (Some(rep), false, None) | (None, false, Some(rep)) => RepetitionKind::fixed(rep),
                (None, false, None) => {
                    return Err(ParseErrorKind::Expected("number").at(self.span()))
                }
            };

            self.expect(Token::CloseBrace)?;

            Ok(Some(kind))
        } else {
            Ok(None)
        }
    }

    fn parse_atom(&mut self) -> PResult<Option<Rule<'i>>> {
        Ok(self
            .parse_group()?
            .try_or_else(|| self.parse_string())?
            .try_or_else(|| self.parse_char_set())?
            .or_else(|| self.parse_boundary())
            .try_or_else(|| self.parse_reference())?
            .try_or_else(|| self.parse_code_point_rule())?
            .try_or_else(|| self.parse_range())?
            .try_or_else(|| self.parse_regex())?
            .or_else(|| self.parse_variable())
            .or_else(|| self.parse_dot()))
    }

    /// Parses a (possibly capturing) group, e.g. `(E E | E)` or `:name(E)`.
    fn parse_group(&mut self) -> PResult<Option<Rule<'i>>> {
        let (kind, start_span) = self.parse_group_kind();
        if !kind.is_normal() {
            self.expect(Token::OpenParen)?;
        } else if !self.consume(Token::OpenParen) {
            return Ok(None);
        }

        self.recursion_start()?;
        let rule = self.parse_modified()?;
        self.recursion_end();

        self.expect(Token::CloseParen)
            .map_err(|p| ParseErrorKind::Expected("`)` or an expression").at(p.span))?;
        let span = start_span.join(self.last_span());

        let rule = Rule::Group(Group::new(vec![rule], kind, span));
        Ok(Some(rule))
    }

    /// Parses `:name` or just `:`. Returns the span of the colon with the name.
    fn parse_group_kind(&mut self) -> (GroupKind<'i>, Span) {
        if self.consume_reserved("atomic") {
            let span = self.last_span();
            (GroupKind::Atomic, span)
        } else if self.consume(Token::Colon) {
            let span = self.last_span();
            // TODO: Better diagnostic for `:let(`
            let name = self.consume_as(Token::Identifier);
            (GroupKind::Capturing(Capture::new(name)), span)
        } else {
            (GroupKind::Normal, self.span().start())
        }
    }

    /// Parses a string literal.
    fn parse_string(&mut self) -> PResult<Option<Rule<'i>>> {
        if let Some(s) = self.consume_as(Token::String) {
            let span = self.last_span();
            let content = helper::parse_quoted_text(s).map_err(|k| k.at(span))?;
            Ok(Some(Rule::Literal(Literal::new(content, span))))
        } else {
            Ok(None)
        }
    }

    /// Parses a char set, surrounded by `[` `]`. This was previously called a
    /// "char class", but that name is ambiguous and is being phased out.
    ///
    /// This function does _not_ parse exclamation marks in front of a char
    /// class, because negation is handled separately.
    fn parse_char_set(&mut self) -> PResult<Option<Rule<'i>>> {
        if self.consume(Token::OpenBracket) {
            let start_span = self.last_span();

            if self.consume(Token::Caret) {
                return Err(
                    ParseErrorKind::CharClass(CharClassError::CaretInGroup).at(self.last_span())
                );
            }

            let inner = self.parse_char_set_inner()?;

            self.expect(Token::CloseBracket).map_err(|p| {
                ParseErrorKind::Expected(
                    "character class, string, code point, Unicode property or `]`",
                )
                .at(p.span)
            })?;
            let span = start_span.join(self.last_span());

            if let CharGroup::Items(v) = &inner {
                if v.is_empty() {
                    return Err(ParseErrorKind::CharClass(CharClassError::Empty).at(span));
                }
            }

            Ok(Some(Rule::CharClass(CharClass::new(inner, span))))
        } else {
            Ok(None)
        }
    }

    /// Parses a char group, i.e. the contents of a char set. This is a sequence
    /// of characters, character classes, character ranges or Unicode
    /// properties. Some of them can be negated.
    fn parse_char_set_inner(&mut self) -> PResult<CharGroup> {
        let span_start = self.span();

        let mut items = Vec::new();
        loop {
            let mut nots_span = self.span();
            let mut nots = 0usize;
            while self.consume(Token::Not) {
                nots += 1;
                nots_span = nots_span.join(self.last_span());
            }

            let item = if let Some(group) = self.parse_char_group_chars_or_range()? {
                if nots > 0 {
                    return Err(ParseErrorKind::UnallowedNot.at(nots_span));
                }
                group
            } else if let Some(group) = self.parse_char_group_ident(nots % 2 != 0)? {
                if nots > 1 {
                    return Err(ParseErrorKind::UnallowedMultiNot(nots).at(nots_span));
                }
                group
            } else {
                break;
            };
            items.push(item);
        }

        let mut iter = items.into_iter();
        let mut group = iter.next().unwrap_or_else(|| CharGroup::Items(vec![]));

        for item in iter {
            group
                .add(item)
                .map_err(|e| ParseErrorKind::CharClass(e).at(span_start.join(self.last_span())))?;
        }
        Ok(group)
    }

    /// Parses an identifier or dot in a char set
    fn parse_char_group_ident(&mut self, negative: bool) -> PResult<Option<CharGroup>> {
        if self.consume(Token::Dot) || self.consume(Token::Identifier) {
            let span = self.last_span();

            let (item, warning) = CharGroup::try_from_group_name(self.source_at(span), negative)
                .map_err(|e| e.at(span))?;

            if let Some(warning) = warning {
                self.add_warning(ParseWarningKind::Deprecation(warning).at(span));
            }
            Ok(Some(item))
        } else if let Some(name) = self.consume_as(Token::ReservedName) {
            Err(ParseErrorKind::UnexpectedKeyword(name.to_owned()).at(self.last_span()))
        } else {
            Ok(None)
        }
    }

    /// Parses a string literal or a character range in a char set, e.g. `"axd"`
    /// or `'0'-'7'`.
    fn parse_char_group_chars_or_range(&mut self) -> PResult<Option<CharGroup>> {
        let span1 = self.span();
        let Some(first) = self.parse_string_or_char()? else {
            return Ok(None);
        };

        if self.consume(Token::Dash) {
            let span2 = self.span();
            let Some(last) = self.parse_string_or_char()? else {
                return Err(ParseErrorKind::Expected("code point or character").at(self.span()));
            };

            let first = first.to_char().map_err(|e| e.at(span1))?;
            let last = last.to_char().map_err(|e| e.at(span2))?;

            let group = CharGroup::try_from_range(first, last).ok_or_else(|| {
                ParseErrorKind::CharClass(CharClassError::DescendingRange(first, last))
                    .at(span1.join(span2))
            })?;
            Ok(Some(group))
        } else {
            let group = match first {
                StringOrChar::String(s) => CharGroup::from_chars(
                    helper::parse_quoted_text(s).map_err(|k| k.at(span1))?.borrow(),
                ),
                StringOrChar::Char(c) => CharGroup::from_char(c),
            };
            Ok(Some(group))
        }
    }

    fn parse_string_or_char(&mut self) -> PResult<Option<StringOrChar<'i>>> {
        let res = if let Some(s) = self.consume_as(Token::String) {
            StringOrChar::String(s)
        } else if let Some((c, _)) = self.parse_code_point()? {
            StringOrChar::Char(c)
        } else if let Some(c) = self.parse_special_char() {
            StringOrChar::Char(c)
        } else {
            return Ok(None);
        };
        Ok(Some(res))
    }

    fn parse_code_point(&mut self) -> PResult<Option<(char, Span)>> {
        if let Some(cp) = self.consume_as(Token::CodePoint) {
            let span = self.last_span();
            let hex = &cp[2..];
            if hex.len() > 6 {
                Err(ParseErrorKind::CodePoint(CodePointError::Invalid).at(span))
            } else {
                u32::from_str_radix(hex, 16)
                    .ok()
                    .and_then(|n| char::try_from(n).ok())
                    .map(|c| Some((c, span)))
                    .ok_or_else(|| ParseErrorKind::CodePoint(CodePointError::Invalid).at(span))
            }
        } else {
            if self.is(Token::Identifier) {
                let span = self.span();
                let str = self.source_at(span);

                if let Some(rest) = str.strip_prefix('U') {
                    if let Ok(n) = u32::from_str_radix(rest, 16) {
                        self.advance();

                        if let Ok(c) = char::try_from(n) {
                            return Ok(Some((c, span)));
                        }
                        return Err(ParseErrorKind::CodePoint(CodePointError::Invalid).at(span));
                    }
                }
            }

            Ok(None)
        }
    }

    fn parse_code_point_rule(&mut self) -> PResult<Option<Rule<'i>>> {
        if let Some((c, span)) = self.parse_code_point()? {
            Ok(Some(Rule::CharClass(CharClass::new(CharGroup::from_char(c), span))))
        } else {
            Ok(None)
        }
    }

    fn parse_special_char(&mut self) -> Option<char> {
        if let Some((Token::Identifier, string)) = self.peek() {
            let c = match string {
                "n" => '\n',
                "r" => '\r',
                "t" => '\t',
                "a" => '\u{07}',
                "e" => '\u{1B}',
                "f" => '\u{0C}',
                _ => return None,
            };
            self.advance();
            Some(c)
        } else {
            None
        }
    }

    /// Parses a boundary. For start and end, there are two syntaxes: `^`, `$`
    /// (new) and `<%`, `%>` (deprecated). Word boundaries are `%`.
    ///
    /// The deprecated syntax issues a warning.
    ///
    /// This function does _not_ parse negated negated word boundaries (`!%`),
    /// since negation is handled elsewhere. It also does _not_ parse the
    /// `Start` and `End` global variables.
    fn parse_boundary(&mut self) -> Option<Rule<'i>> {
        let span = self.span();
        let kind = if self.consume(Token::Caret) {
            BoundaryKind::Start
        } else if self.consume(Token::Dollar) {
            BoundaryKind::End
        } else if self.consume(Token::BWord) {
            BoundaryKind::Word
        } else {
            return None;
        };
        Some(Rule::Boundary(Boundary::new(kind, span)))
    }

    /// Parses a reference. Supported syntaxes are `::name`, `::3`, `::+3` and
    /// `::-3`.
    fn parse_reference(&mut self) -> PResult<Option<Rule<'i>>> {
        if self.consume(Token::DoubleColon) {
            let start_span = self.last_span();

            let target = if self.consume(Token::Plus) {
                let num = self.expect_number::<i32>()?;
                ReferenceTarget::Relative(num)
            } else if self.consume(Token::Dash) {
                let num = self.expect_number::<i32>()?;
                // negating from positive to negative can't overflow, luckily
                ReferenceTarget::Relative(-num)
            } else if let Some(num) = self.consume_number::<u32>()? {
                ReferenceTarget::Number(num)
            } else {
                // TODO: Better diagnostic for `::let`
                let name = self
                    .expect_as(Token::Identifier)
                    .map_err(|p| ParseErrorKind::Expected("number of group name").at(p.span))?;
                ReferenceTarget::Named(name)
            };

            let span = start_span.join(self.last_span());
            Ok(Some(Rule::Reference(Reference::new(target, span))))
        } else {
            Ok(None)
        }
    }

    fn parse_range(&mut self) -> PResult<Option<Rule<'i>>> {
        if self.consume_reserved("range") {
            let span_start = self.last_span();

            let first = self.expect_as(Token::String)?;
            let span_1 = self.last_span();
            self.expect(Token::Dash)?;
            let second = self.expect_as(Token::String)?;
            let span_2 = self.last_span();

            let radix = if self.consume_reserved("base") {
                let n = self.expect_number()?;
                let span = self.last_span();
                if n > 36 {
                    return Err(ParseErrorKind::Number(NumberError::TooLarge).at(span));
                } else if n < 2 {
                    return Err(ParseErrorKind::Number(NumberError::TooSmall).at(span));
                }
                n
            } else {
                10u8
            };

            let span = span_start.join(self.last_span());

            let start = helper::parse_number(helper::strip_first_last(first), radix)
                .map_err(|k| ParseErrorKind::from(k).at(span_1))?;
            let end = helper::parse_number(helper::strip_first_last(second), radix)
                .map_err(|k| ParseErrorKind::from(k).at(span_2))?;

            if start.len() > end.len() || (start.len() == end.len() && start > end) {
                return Err(ParseErrorKind::RangeIsNotIncreasing.at(span_1.join(span_2)));
            }

            Ok(Some(Rule::Range(Range::new(start, end, radix, span))))
        } else {
            Ok(None)
        }
    }

    /// Parses an unescaped regex expression (`regex "[test]"`)
    fn parse_regex(&mut self) -> PResult<Option<Rule<'i>>> {
        if self.consume_reserved("regex") {
            let span_start = self.last_span();
            let lit = self.expect_as(Token::String)?;
            let span_end = self.last_span();

            let content = helper::parse_quoted_text(lit).map_err(|k| k.at(span_end))?;

            let span = span_start.join(span_end);
            Ok(Some(Rule::Regex(Regex::new(content, span))))
        } else {
            Ok(None)
        }
    }

    /// Parses a variable (usage site).
    fn parse_variable(&mut self) -> Option<Rule<'i>> {
        self.consume_as(Token::Identifier)
            .map(|ident| Rule::Variable(Variable::new(ident, self.last_span())))
    }

    /// Parses the dot
    fn parse_dot(&mut self) -> Option<Rule<'i>> {
        if self.consume(Token::Dot) {
            Some(Rule::CharClass(CharClass::new(CharGroup::Dot, self.last_span())))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy)]
enum StringOrChar<'i> {
    String(&'i str),
    Char(char),
}

impl StringOrChar<'_> {
    fn to_char(self) -> Result<char, ParseErrorKind> {
        Err(ParseErrorKind::CharString(match self {
            StringOrChar::Char(c) => return Ok(c),
            StringOrChar::String(s) => {
                let s = helper::parse_quoted_text(s)?;
                let mut iter = s.chars();
                match iter.next() {
                    Some(c) if matches!(iter.next(), None) => return Ok(c),
                    Some(_) => CharStringError::TooManyCodePoints,
                    _ => CharStringError::Empty,
                }
            }
        }))
    }
}

trait TryOptionExt<T> {
    fn try_or_else<E>(self, f: impl FnMut() -> Result<Option<T>, E>) -> Result<Option<T>, E>;
}

impl<T> TryOptionExt<T> for Option<T> {
    #[inline(always)]
    fn try_or_else<E>(self, mut f: impl FnMut() -> Result<Option<T>, E>) -> Result<Option<T>, E> {
        match self {
            Some(val) => Ok(Some(val)),
            None => f(),
        }
    }
}
