//! The settings files: one `Key=Value` per line, `#` starts a comment, quotes
//! around a value are dropped, the last line for a key wins.
//!
//! Every part reads them with these rules: the window, the daemon, the agent.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyValue {
    values: HashMap<String, String>,
}

impl KeyValue {
    pub fn load(path: &Path) -> KeyValue {
        std::fs::read_to_string(path).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    pub fn parse(text: &str) -> KeyValue {
        let mut values = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            // A comment can follow a value.
            let value = value.split('#').next().unwrap_or("").trim();
            let value = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
                &value[1..value.len() - 1]
            } else {
                value
            };
            values.insert(key.to_string(), value.to_string());
        }
        KeyValue { values }
    }

    /// The value, or `fallback` when it is missing or empty.
    pub fn get<'a>(&'a self, key: &str, fallback: &'a str) -> &'a str {
        match self.values.get(key) {
            Some(v) if !v.is_empty() => v,
            _ => fallback,
        }
    }

    pub fn raw(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn boolean(&self, key: &str, fallback: bool) -> bool {
        parse_bool(self.get(key, ""), fallback)
    }

    pub fn integer(&self, key: &str, fallback: i64, min: i64, max: i64) -> i64 {
        self.get(key, "").parse::<i64>().map(|v| v.clamp(min, max)).unwrap_or(fallback)
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.values.insert(key.into(), value.into());
    }
}

pub fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().to_lowercase().as_str() {
        "yes" | "y" | "true" | "1" | "on" | "enabled" => true,
        "no" | "n" | "false" | "0" | "off" | "disabled" => false,
        _ => fallback,
    }
}

/// `text` with `key` set to `value`: the first line for the key (also a
/// commented out one) takes the value, later ones go, and a key that was not
/// there is added at the end.
pub fn with_value(text: &str, key: &str, value: &str) -> String {
    let matches = |line: &str, commented: bool| {
        let mut l = line.trim_start();
        if commented && let Some(rest) = l.strip_prefix('#') {
            l = rest.trim_start();
        }
        l.strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    let mut out = String::with_capacity(text.len() + key.len() + value.len() + 2);
    let mut done = false;
    for line in text.lines() {
        if !done && matches(line, true) {
            out.push_str(&format!("{key}={value}\n"));
            done = true;
            continue;
        }
        if done && matches(line, false) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !done {
        out.push_str(&format!("{key}={value}\n"));
    }
    out
}

/// Writes `key=value` into the file at `path`, which is made with a short
/// header if it is not there. Replaced, not rewritten in place: the agent
/// watches the folder and picks up the new file the moment it lands.
pub fn set_in_file(path: &Path, key: &str, value: &str, header: &str, mode: u32) -> std::io::Result<()> {
    let old = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            format!("# {header}\n#\n# Written by `face-unlock`. Nothing here needs editing by hand:\n# every option is in its window.\n")
        }
        Err(e) => return Err(e),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mode = std::fs::metadata(path).map(|m| m.permissions().mode() & 0o7777).unwrap_or(mode);
    replace_file(path, with_value(&old, key, value).as_bytes(), mode)
}

/// Writes `content` next to `path` and moves it over, with `mode`.
pub fn replace_file(path: &Path, content: &[u8], mode: u32) -> std::io::Result<()> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode_compat(mode).open(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode))?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

trait ModeCompat {
    fn mode_compat(&mut self, mode: u32) -> &mut Self;
}

impl ModeCompat for std::fs::OpenOptions {
    fn mode_compat(&mut self, mode: u32) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.mode(mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_like_the_old_shell_reader() {
        let kv = KeyValue::parse("# c\nA = 1\nB=\"two words\" # note\nA=3\n=x\nC=\n");
        assert_eq!(kv.get("A", ""), "3");
        assert_eq!(kv.get("B", ""), "two words");
        assert_eq!(kv.get("C", "fallback"), "fallback");
        assert!(!kv.boolean("C", false));
        assert_eq!(kv.integer("A", 0, 0, 2), 2);
    }

    #[test]
    fn sets_the_first_line_and_drops_the_rest() {
        let out = with_value("# head\n#Liveness=off\nCamera=auto\nLiveness=heavy\n", "Liveness", "light");
        assert_eq!(out, "# head\nLiveness=light\nCamera=auto\n");
        assert_eq!(with_value("A=1\n", "B", "2"), "A=1\nB=2\n");
    }

    #[test]
    fn writes_a_new_file_with_a_header() {
        let dir = std::env::temp_dir().join(format!("fu-kv-{}", std::process::id()));
        let path = dir.join("sub/config");
        set_in_file(&path, "Enabled", "yes", "Face Unlock", 0o644).unwrap();
        set_in_file(&path, "Enabled", "no", "Face Unlock", 0o644).unwrap();
        let kv = KeyValue::load(&path);
        assert_eq!(kv.get("Enabled", ""), "no");
        assert!(std::fs::read_to_string(&path).unwrap().starts_with("# Face Unlock"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
