use crate::Span;

#[derive(Clone, PartialEq, Eq)]
pub struct Range {
    pub start: Vec<u8>,
    pub end: Vec<u8>,
    pub radix: u8,
    pub span: Span,
}

impl Range {
    pub(crate) fn new(start: Vec<u8>, end: Vec<u8>, radix: u8, span: Span) -> Self {
        Range { start, end, radix, span }
    }

    #[cfg(feature = "dbg")]
    pub(super) fn pretty_print(&self, buf: &mut crate::PrettyPrinter) {
        fn hex(n: u8) -> char {
            match n {
                0..=9 => (n + b'0') as char,
                _ => (n + (b'A' - 10)) as char,
            }
        }

        buf.push_str("range '");
        buf.extend(self.start.iter().map(|&n| hex(n)));
        buf.push_str("'-'");
        buf.extend(self.end.iter().map(|&n| hex(n)));
        buf.push('\'');
    }
}
