use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub last_path: Option<String>,
    pub scroll_ratio: f32,
    pub recent_ratios: Vec<(String, f32)>,
    pub theme_idx: usize,
    pub font_scale: f32,
    pub sidebar_open: bool,
    pub window_size: [f32; 2],
    pub window_pos: Option<[f32; 2]>,
    pub maximized: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            last_path: None,
            scroll_ratio: 0.0,
            recent_ratios: Vec::new(),
            theme_idx: 0,
            font_scale: 1.0,
            sidebar_open: false,
            window_size: [1120.0, 820.0],
            window_pos: None,
            maximized: false,
        }
    }
}

impl Session {
    pub fn ratio_for(&self, path: &Path) -> Option<f32> {
        let key = path.to_string_lossy();
        self.recent_ratios
            .iter()
            .find(|(p, _)| p.eq_ignore_ascii_case(&key))
            .map(|(_, ratio)| *ratio)
    }

    pub fn record_ratio(&mut self, path: &Path, ratio: f32) {
        let key = path.to_string_lossy().into_owned();
        let ratio = ratio.clamp(0.0, 1.0);
        self.recent_ratios
            .retain(|(p, _)| !p.eq_ignore_ascii_case(&key));
        self.recent_ratios.insert(0, (key, ratio));
        if self.recent_ratios.len() > 40 {
            self.recent_ratios.truncate(40);
        }
    }
}

pub fn session_path() -> Option<PathBuf> {
    if let Some(override_path) = std::env::var_os("SUDU_SESSION") {
        return Some(PathBuf::from(override_path));
    }
    if std::env::var_os("SUDU_NO_SESSION").is_some() {
        return None;
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return Some(PathBuf::from(local).join("sudu").join("session.json"));
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("session.json")))
}

pub fn load() -> Session {
    let Some(path) = session_path() else {
        return Session::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Session::default();
    };
    parse_json(&text).unwrap_or_default()
}

pub fn save(session: &Session) {
    let Some(path) = session_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, to_json(session));
}

pub fn to_json(session: &Session) -> String {
    let mut out = String::from("{\n");
    match &session.last_path {
        Some(path) => out.push_str(&format!("  \"last_path\": \"{}\",\n", escape_json(path))),
        None => out.push_str("  \"last_path\": null,\n"),
    }
    out.push_str(&format!("  \"scroll_ratio\": {:.6},\n", session.scroll_ratio.clamp(0.0, 1.0)));
    out.push_str(&format!("  \"theme_idx\": {},\n", session.theme_idx % 4));
    out.push_str(&format!("  \"font_scale\": {:.3},\n", session.font_scale.clamp(0.8, 1.6)));
    out.push_str(&format!("  \"sidebar_open\": {},\n", session.sidebar_open));
    out.push_str(&format!(
        "  \"window_size\": [{:.1}, {:.1}],\n",
        session.window_size[0].max(720.0),
        session.window_size[1].max(480.0)
    ));
    match session.window_pos {
        Some([x, y]) => out.push_str(&format!("  \"window_pos\": [{:.1}, {:.1}],\n", x, y)),
        None => out.push_str("  \"window_pos\": null,\n"),
    }
    out.push_str(&format!("  \"maximized\": {},\n", session.maximized));
    out.push_str("  \"recent_ratios\": [\n");
    for (i, (path, ratio)) in session.recent_ratios.iter().enumerate() {
        let comma = if i + 1 < session.recent_ratios.len() { "," } else { "" };
        out.push_str(&format!(
            "    [\"{}\", {:.6}]{}\n",
            escape_json(path),
            ratio.clamp(0.0, 1.0),
            comma
        ));
    }
    out.push_str("  ]\n}\n");
    out
}

pub fn parse_json(raw: &str) -> Option<Session> {
    let mut session = Session::default();
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return None;
    }
    let mut parser = JsonParser {
        bytes: trimmed.as_bytes(),
        src: trimmed,
        pos: 1,
    };
    loop {
        parser.skip_ws();
        if parser.peek() == Some(b'}') {
            break;
        }
        let key = parser.read_string()?;
        parser.skip_ws();
        if !parser.eat(b':') {
            return None;
        }
        parser.skip_ws();
        match key.as_str() {
            "last_path" => {
                if parser.eat_literal("null") {
                    session.last_path = None;
                } else {
                    session.last_path = Some(parser.read_string()?);
                }
            }
            "scroll_ratio" => {
                session.scroll_ratio = parser.read_number()?.clamp(0.0, 1.0);
            }
            "theme_idx" => {
                session.theme_idx = (parser.read_number()?.max(0.0) as usize) % 4;
            }
            "font_scale" => {
                session.font_scale = parser.read_number()?.clamp(0.8, 1.6);
            }
            "sidebar_open" => {
                session.sidebar_open = parser.read_bool()?;
            }
            "maximized" => {
                session.maximized = parser.read_bool()?;
            }
            "window_size" => {
                let pair = parser.read_num_pair()?;
                session.window_size = [pair[0].max(720.0), pair[1].max(480.0)];
            }
            "window_pos" => {
                if parser.eat_literal("null") {
                    session.window_pos = None;
                } else {
                    session.window_pos = Some(parser.read_num_pair()?);
                }
            }
            "recent_ratios" => {
                session.recent_ratios = parser.read_recent_ratios()?;
            }
            _ => {
                parser.skip_value()?;
            }
        }
        parser.skip_ws();
        let _ = parser.eat(b',');
    }
    Some(session)
}

fn escape_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    src: &'a str,
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_literal(&mut self, lit: &str) -> bool {
        if self.src[self.pos..].starts_with(lit) {
            self.pos += lit.len();
            true
        } else {
            false
        }
    }

    fn read_string(&mut self) -> Option<String> {
        self.skip_ws();
        if !self.eat(b'"') {
            return None;
        }
        let mut out = String::new();
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                self.pos += 1;
                return Some(out);
            }
            if b == b'\\' {
                self.pos += 1;
                let esc = *self.bytes.get(self.pos)?;
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = self.src.get(self.pos..self.pos + 4)?;
                        self.pos += 4;
                        let code = u32::from_str_radix(hex, 16).ok()?;
                        if let Some(ch) = char::from_u32(code) {
                            out.push(ch);
                        }
                    }
                    other => out.push(other as char),
                }
            } else {
                let ch = self.src[self.pos..].chars().next()?;
                out.push(ch);
                self.pos += ch.len_utf8();
            }
        }
        None
    }

    fn read_number(&mut self) -> Option<f32> {
        self.skip_ws();
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_digit() || self.bytes[self.pos] == b'.')
        {
            self.pos += 1;
        }
        self.src[start..self.pos].parse::<f32>().ok()
    }

    fn read_bool(&mut self) -> Option<bool> {
        self.skip_ws();
        if self.eat_literal("true") {
            Some(true)
        } else if self.eat_literal("false") {
            Some(false)
        } else {
            None
        }
    }

    fn read_num_pair(&mut self) -> Option<[f32; 2]> {
        self.skip_ws();
        if !self.eat(b'[') {
            return None;
        }
        let a = self.read_number()?;
        self.skip_ws();
        if !self.eat(b',') {
            return None;
        }
        let b = self.read_number()?;
        self.skip_ws();
        if !self.eat(b']') {
            return None;
        }
        Some([a, b])
    }

    fn read_recent_ratios(&mut self) -> Option<Vec<(String, f32)>> {
        self.skip_ws();
        if !self.eat(b'[') {
            return None;
        }
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            if self.eat(b']') {
                return Some(out);
            }
            if !self.eat(b'[') {
                return None;
            }
            let path = self.read_string()?;
            self.skip_ws();
            if !self.eat(b',') {
                return None;
            }
            let ratio = self.read_number()?;
            self.skip_ws();
            if !self.eat(b']') {
                return None;
            }
            out.push((path, ratio));
            self.skip_ws();
            let _ = self.eat(b',');
        }
    }

    fn skip_value(&mut self) -> Option<()> {
        self.skip_ws();
        match self.peek()? {
            b'"' => {
                let _ = self.read_string()?;
                Some(())
            }
            b'{' => {
                self.pos += 1;
                let mut depth = 1usize;
                while self.pos < self.bytes.len() && depth > 0 {
                    match self.bytes[self.pos] {
                        b'"' => {
                            let _ = self.read_string()?;
                            continue;
                        }
                        b'{' => depth += 1,
                        b'}' => depth -= 1,
                        _ => {}
                    }
                    self.pos += 1;
                }
                Some(())
            }
            b'[' => {
                self.pos += 1;
                let mut depth = 1usize;
                while self.pos < self.bytes.len() && depth > 0 {
                    match self.bytes[self.pos] {
                        b'"' => {
                            let _ = self.read_string()?;
                            continue;
                        }
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        _ => {}
                    }
                    self.pos += 1;
                }
                Some(())
            }
            _ => {
                while self.pos < self.bytes.len()
                    && !matches!(self.bytes[self.pos], b',' | b'}' | b']')
                {
                    self.pos += 1;
                }
                Some(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_json_roundtrip_handles_cjk_paths() {
        let mut s = Session::default();
        s.last_path = Some(String::from(r"D:\文档\中文\测试.md"));
        s.scroll_ratio = 0.425;
        s.theme_idx = 2;
        s.font_scale = 1.15;
        s.sidebar_open = true;
        s.window_size = [1280.0, 900.0];
        s.window_pos = Some([120.0, 60.0]);
        s.maximized = false;
        s.record_ratio(Path::new(r"D:\文档\中文\测试.md"), 0.425);
        let json = to_json(&s);
        let parsed = parse_json(&json).expect("parse session");
        assert_eq!(parsed.last_path, s.last_path);
        assert_eq!(parsed.theme_idx, 2);
        assert!(parsed.sidebar_open);
        assert_eq!(parsed.window_pos, Some([120.0, 60.0]));
        assert_eq!(parsed.recent_ratios.len(), 1);
    }
}
