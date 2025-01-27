use std::collections::{HashMap, HashSet};

use pomsky_syntax::exprs::Rule;

use crate::{error::CompileError, exprs::repetition::RegexQuantifier, regex::Regex};

pub(crate) type CompileResult<'i> = Result<Regex<'i>, CompileError>;

#[derive(Clone)]
pub(crate) struct CompileState<'c, 'i> {
    pub(crate) next_idx: u32,
    pub(crate) used_names: HashMap<String, u32>,
    pub(crate) groups_count: u32,

    pub(crate) default_quantifier: RegexQuantifier,
    pub(crate) variables: Vec<(&'i str, &'c Rule<'i>)>,
    pub(crate) current_vars: HashSet<usize>,
}

impl<'c, 'i> CompileState<'c, 'i> {
    pub(crate) fn new(
        default_quantifier: RegexQuantifier,
        used_names: HashMap<String, u32>,
        groups_count: u32,
        variables: Vec<(&'i str, &'c Rule<'i>)>,
    ) -> Self {
        CompileState {
            next_idx: 1,
            used_names,
            groups_count,
            default_quantifier,
            variables,
            current_vars: Default::default(),
        }
    }
}
