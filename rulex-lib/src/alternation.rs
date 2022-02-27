//! Implements [alternation](https://www.regular-expressions.info/alternation.html):
//! `('alt1' | 'alt2' | 'alt3')`.

use crate::{
    compile::{Compile, CompileResult, CompileState},
    options::CompileOptions,
    Rulex,
};

/// An [alternation](https://www.regular-expressions.info/alternation.html). This is a list of
/// alternatives. Each alternative is a [`Rulex`].
///
/// If an alternative consists of multiple expressions (e.g. `'a' | 'b' 'c'`), that alternative is
/// a [`Rulex::Group`]. Note that a group's parentheses are removed when compiling to a regex if
/// they aren't required. In other words, `'a' | ('b' 'c')` compiles to `a|bc`.
#[derive(Clone, PartialEq, Eq)]
pub struct Alternation<'i> {
    rules: Vec<Rulex<'i>>,
}

impl<'i> Alternation<'i> {
    fn two(a: Rulex<'i>, b: Rulex<'i>) -> Rulex<'i> {
        Rulex::Alternation(Alternation { rules: vec![a, b] })
    }

    pub fn new_rulex(rules: Vec<Rulex<'i>>) -> Rulex {
        rules
            .into_iter()
            .reduce(|a, b| match (a, b) {
                (Rulex::Alternation(mut a), Rulex::Alternation(b)) => {
                    a.rules.extend(b.rules);
                    Rulex::Alternation(a)
                }
                (Rulex::Alternation(mut a), b) => {
                    a.rules.push(b);
                    Rulex::Alternation(a)
                }
                (a, b) => Alternation::two(a, b),
            })
            .unwrap_or(Rulex::Literal(""))
    }
}

#[cfg(feature = "dbg")]
impl core::fmt::Debug for Alternation<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_tuple("Alternation");
        let mut d = &mut d;
        for rule in &self.rules {
            d = d.field(rule);
        }
        d.finish()
    }
}

impl Compile for Alternation<'_> {
    fn comp(
        &self,
        options: CompileOptions,
        state: &mut CompileState,
        buf: &mut String,
    ) -> CompileResult {
        for rule in &self.rules {
            rule.comp(options, state, buf)?;
            buf.push('|');
        }
        let _ = buf.pop();
        Ok(())
    }
}
