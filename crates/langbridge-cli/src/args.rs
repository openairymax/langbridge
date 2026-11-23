//! 轻量参数解析，支持 `--key value`、`--key=value` 与无值开关。

#[derive(Debug, Clone, PartialEq)]
pub enum FlagValue {
    Str(String),
    Bool,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedArgs {
    pub positional: Vec<String>,
    pub flags: Vec<(String, FlagValue)>,
}

impl ParsedArgs {
    fn set(&mut self, key: String, value: FlagValue) {
        if let Some(slot) = self.flags.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.flags.push((key, value));
        }
    }

    pub fn get(&self, name: &str) -> Option<&FlagValue> {
        self.flags.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }

    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn flag_true(&self, name: &str) -> bool {
        matches!(self.get(name), Some(FlagValue::Bool))
    }

    pub fn str(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(FlagValue::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn num(&self, name: &str) -> Option<f64> {
        self.str(name).and_then(js_number)
    }
}

pub fn parse_args(argv: &[String]) -> ParsedArgs {
    let mut out = ParsedArgs::default();
    let mut i = 0usize;
    while i < argv.len() {
        let a = &argv[i];
        if !a.starts_with("--") {
            out.positional.push(a.clone());
            i += 1;
            continue;
        }
        if let Some(eq) = a.find('=') {
            if eq > 0 {
                out.set(
                    a[2..eq].to_string(),
                    FlagValue::Str(a[eq + 1..].to_string()),
                );
                i += 1;
                continue;
            }
        }
        let next = argv.get(i + 1);
        if next.is_some_and(|n| !n.starts_with('-')) {
            out.set(a[2..].to_string(), FlagValue::Str(next.unwrap().clone()));
            i += 2;
        } else {
            out.set(a[2..].to_string(), FlagValue::Bool);
            i += 1;
        }
    }
    out
}

pub fn js_number(raw: &str) -> Option<f64> {
    let t = raw.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    let n: f64 = t.parse().ok()?;
    if n.is_finite() {
        Some(n)
    } else {
        None
    }
}

pub fn js_parse_int(raw: &str) -> Option<i64> {
    let t = raw.trim();
    let mut chars = t.chars().peekable();
    let mut sign = 1i64;
    if let Some(&c) = chars.peek() {
        if c == '+' || c == '-' {
            if c == '-' {
                sign = -1;
            }
            chars.next();
        }
    }
    let mut digits = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            digits.push(c);
            chars.next();
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    digits.parse::<i64>().ok().map(|n| n * sign)
}
