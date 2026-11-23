// Reading the .po files, for the build scripts of the command and of the
// PAM module. Included by them with include!, never compiled on its own.

/// One string of a .po file, with its escapes undone.
fn unquote(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or("");
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => {}
        }
    }
    out
}

/// The msgid and msgstr pairs of a .po file. Fuzzy and untranslated entries
/// are left out, so they fall back to English.
fn parse(src: &str) -> Vec<(String, String)> {
    #[derive(PartialEq)]
    enum Field {
        None,
        Id,
        Str,
    }
    let mut out = Vec::new();
    let (mut id, mut text, mut field, mut fuzzy) = (String::new(), String::new(), Field::None, false);
    let mut flush = |id: &mut String, text: &mut String, fuzzy: &mut bool| {
        if !id.is_empty() && !text.is_empty() && !*fuzzy {
            out.push((std::mem::take(id), std::mem::take(text)));
        }
        id.clear();
        text.clear();
        *fuzzy = false;
    };
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("#,") && line.contains("fuzzy") {
            fuzzy = true;
        } else if let Some(rest) = line.strip_prefix("msgid ") {
            if field == Field::Str {
                flush(&mut id, &mut text, &mut fuzzy);
            }
            id = unquote(rest);
            field = Field::Id;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            text = unquote(rest);
            field = Field::Str;
        } else if line.starts_with('"') {
            match field {
                Field::Id => id.push_str(&unquote(line)),
                Field::Str => text.push_str(&unquote(line)),
                Field::None => {}
            }
        } else if line.is_empty() && field == Field::Str {
            flush(&mut id, &mut text, &mut fuzzy);
            field = Field::None;
        }
    }
    if field == Field::Str {
        flush(&mut id, &mut text, &mut fuzzy);
    }
    out
}

