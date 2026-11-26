// SPDX-License-Identifier: GPL-3.0-only
//! Calculator: a small recursive-descent parser for everyday maths.
//!
//! `+ - * / ^ %` (with `x`, `×`, `÷` and `**` as aliases), brackets, implicit
//! multiplication by a bracket or constant (`2(3+4)`, `2pi`), the functions
//! `sqrt cbrt abs ln log log2 exp sin cos tan asin acos atan round floor ceil`
//! (trigonometry in radians), and the constants `pi`, `π`, `tau` and `e`.
//! `%` is modulo between two numbers and a percentage after the last one
//! (`200 * 15%` is 30).

/// Evaluate `expr`, or `None` if it isn't a complete, finite expression.
#[must_use]
pub fn eval(expr: &str) -> Option<f64> {
    let tokens = tokenize(expr)?;
    let mut p = Parser { tokens, pos: 0 };
    let v = p.sum()?;
    (p.pos == p.tokens.len() && v.is_finite()).then_some(v)
}

/// Whether `expr` looks like a calculation rather than an app name, so the
/// result can be shown without the `=` prefix: it must start like a number
/// and contain an operator or a function call.
#[must_use]
pub fn looks_like_maths(expr: &str) -> bool {
    let t = expr.trim_start();
    let starts = t
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, '(' | '-' | '.' | '√'));
    starts
        && tokenize(t).is_some_and(|tokens| {
            tokens
                .iter()
                .any(|t| matches!(t, Tok::Op(_) | Tok::Func(_) | Tok::LParen))
        })
}

/// Format a result for display and the clipboard: no trailing zeros, and
/// rounding noise from binary floats removed (`0.1 + 0.2` is `0.3`).
#[must_use]
pub fn format(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.abs() >= 1e15 || v.abs() < 1e-9 {
        return format!("{v:e}");
    }
    let s = format!("{v:.12}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Op(char),
    Func(&'static str),
    LParen,
    RParen,
}

const FUNCS: [&str; 16] = [
    "sqrt", "cbrt", "abs", "ln", "log2", "log", "exp", "asin", "acos", "atan", "sin", "cos", "tan",
    "round", "floor", "ceil",
];

fn tokenize(s: &str) -> Option<Vec<Tok>> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            // a comma is a decimal point too (2,5 in pt-BR, es, fr)
            '0'..='9' | '.' | ',' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || matches!(chars[i], '.' | ','))
                {
                    i += 1;
                }
                // exponent: 1e3, 2.5E-4
                if i + 1 < chars.len()
                    && matches!(chars[i], 'e' | 'E')
                    && (chars[i + 1].is_ascii_digit()
                        || (matches!(chars[i + 1], '+' | '-')
                            && chars.get(i + 2).is_some_and(char::is_ascii_digit)))
                {
                    i += 2;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let n: String = chars[start..i]
                    .iter()
                    .map(|&c| if c == ',' { '.' } else { c })
                    .collect();
                out.push(Tok::Num(n.parse().ok()?));
            }
            '+' | '-' | '/' | '^' | '%' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            '*' => {
                if chars.get(i + 1) == Some(&'*') {
                    out.push(Tok::Op('^'));
                    i += 2;
                } else {
                    out.push(Tok::Op('*'));
                    i += 1;
                }
            }
            '×' | '·' => {
                out.push(Tok::Op('*'));
                i += 1;
            }
            '÷' => {
                out.push(Tok::Op('/'));
                i += 1;
            }
            '−' => {
                out.push(Tok::Op('-'));
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            'π' => {
                out.push(Tok::Num(std::f64::consts::PI));
                i += 1;
            }
            '√' => {
                out.push(Tok::Func("sqrt"));
                i += 1;
            }
            c if c.is_alphabetic() => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric()) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect::<String>().to_lowercase();
                match word.as_str() {
                    "pi" => out.push(Tok::Num(std::f64::consts::PI)),
                    "tau" => out.push(Tok::Num(std::f64::consts::TAU)),
                    "e" => out.push(Tok::Num(std::f64::consts::E)),
                    "x" => out.push(Tok::Op('*')),
                    "mod" => out.push(Tok::Op('%')),
                    w => out.push(Tok::Func(FUNCS.iter().find(|f| **f == w)?)),
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

struct Parser {
    tokens: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    /// sum := product (('+' | '-') product)*
    fn sum(&mut self) -> Option<f64> {
        let mut v = self.product()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let rhs = self.product()?;
            v = if op == '+' { v + rhs } else { v - rhs };
        }
        Some(v)
    }

    /// product := unary (('*' | '/' | '%') unary | implicit unary)*
    fn product(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            match self.peek().cloned() {
                Some(Tok::Op(op @ ('*' | '/'))) => {
                    self.pos += 1;
                    let rhs = self.unary()?;
                    v = if op == '*' { v * rhs } else { v / rhs };
                }
                Some(Tok::Op('%')) => {
                    self.pos += 1;
                    // a trailing % (or one before a closing bracket or +/-)
                    // is a percentage of the value it follows
                    match self.peek() {
                        None | Some(Tok::RParen | Tok::Op('+' | '-')) => v /= 100.0,
                        _ => {
                            let rhs = self.unary()?;
                            v %= rhs;
                        }
                    }
                }
                // 2(3+4), 2pi, 3 sqrt(4)
                Some(Tok::LParen | Tok::Func(_) | Tok::Num(_)) => {
                    let rhs = self.unary()?;
                    v *= rhs;
                }
                _ => return Some(v),
            }
        }
    }

    /// unary := ('-' | '+') unary | power
    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Some(-self.unary()?)
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// power := atom ('^' unary)?   (right-associative)
    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.pos += 1;
            let exp = self.unary()?;
            return Some(base.powf(exp));
        }
        Some(base)
    }

    /// atom := number | '(' sum ')' | func atom
    fn atom(&mut self) -> Option<f64> {
        match self.next()? {
            Tok::Num(n) => Some(n),
            Tok::LParen => {
                let v = self.sum()?;
                // a missing closing bracket at the very end is forgiven
                match self.peek() {
                    Some(Tok::RParen) => self.pos += 1,
                    None => {}
                    _ => return None,
                }
                Some(v)
            }
            Tok::Func(f) => {
                let x = self.power()?;
                Some(match f {
                    "sqrt" => x.sqrt(),
                    "cbrt" => x.cbrt(),
                    "abs" => x.abs(),
                    "ln" => x.ln(),
                    "log" => x.log10(),
                    "log2" => x.log2(),
                    "exp" => x.exp(),
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "tan" => x.tan(),
                    "asin" => x.asin(),
                    "acos" => x.acos(),
                    "atan" => x.atan(),
                    "round" => x.round(),
                    "floor" => x.floor(),
                    "ceil" => x.ceil(),
                    _ => return None,
                })
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> String {
        eval(s).map_or_else(|| "none".into(), format)
    }

    #[test]
    fn arithmetic() {
        assert_eq!(ev("2+3*4"), "14");
        assert_eq!(ev("(2+3)*4"), "20");
        assert_eq!(ev("2^3^2"), "512");
        assert_eq!(ev("-2^2"), "-4");
        assert_eq!(ev("10/4"), "2.5");
        assert_eq!(ev("0.1+0.2"), "0.3");
        assert_eq!(ev("7 % 3"), "1");
        assert_eq!(ev("200 * 15%"), "30");
        assert_eq!(ev("3 x 4"), "12");
        assert_eq!(ev("6÷3×2"), "4");
        assert_eq!(ev("1e3 + 1"), "1001");
        assert_eq!(ev("2**10"), "1024");
        assert_eq!(ev("2,5*2"), "5");
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(ev("sqrt(16)"), "4");
        assert_eq!(ev("√9"), "3");
        assert_eq!(ev("2pi"), format(std::f64::consts::TAU));
        assert_eq!(ev("2(3+4)"), "14");
        assert_eq!(ev("log(1000)"), "3");
        assert_eq!(ev("round(2.6)"), "3");
        assert_eq!(ev("(1+2"), "3");
    }

    #[test]
    fn rejects() {
        assert_eq!(ev("1/0"), "none");
        assert_eq!(ev("2+"), "none");
        assert_eq!(ev("firefox"), "none");
        assert_eq!(ev(""), "none");
    }

    #[test]
    fn maths_detection() {
        assert!(looks_like_maths("2+2"));
        assert!(looks_like_maths("(3"));
        assert!(!looks_like_maths("7zip"));
        assert!(!looks_like_maths("42"));
        assert!(!looks_like_maths("firefox"));
    }
}
