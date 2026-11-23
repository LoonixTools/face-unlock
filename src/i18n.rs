//! Translations.
//!
//! Messages are written in English and looked up in the catalogs that build.rs
//! compiles in from po/. Nothing is needed at run time, not even libintl.

use std::fmt::Display;

include!(concat!(env!("OUT_DIR"), "/catalogs.rs"));

/// The locale user-facing text is shown in: the standard POSIX order, where
/// LC_ALL wins and LC_ALL=C really means English.
pub fn ui_locale() -> String {
    for var in ["FU_UI_LOCALE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && !v.is_empty()
        {
            return v;
        }
    }
    system_locale().unwrap_or_else(|| "C".into())
}

/// LANG from /etc/locale.conf, or from /etc/default/locale on Debian.
pub fn system_locale() -> Option<String> {
    ["/etc/locale.conf", "/etc/default/locale"].iter().find_map(|f| {
        let text = std::fs::read_to_string(f).ok()?;
        text.lines()
            .find_map(|l| l.strip_prefix("LANG="))
            .map(|v| v.trim().trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
    })
}

/// The catalog for a locale such as "de_DE.UTF-8", if there is one: one
/// for the language and region ("pt_BR") first, then one for the language.
fn catalog(locale: &str) -> Option<&'static [(&'static str, &'static str)]> {
    let full = locale.split(['.', '@']).next().unwrap_or("");
    let lang = full.split('_').next().unwrap_or("");
    [full, lang].into_iter().find_map(|l| CATALOGS.iter().find(|(c, _)| *c == l).map(|(_, c)| *c))
}

/// `msgid` in `locale`, or `msgid` itself when it has no translation.
pub fn tr_in<'a>(locale: &str, msgid: &'a str) -> &'a str {
    match catalog(locale) {
        Some(c) => match c.binary_search_by(|(id, _)| (*id).cmp(msgid)) {
            Ok(i) => c[i].1,
            Err(_) => msgid,
        },
        None => msgid,
    }
}

/// `msgid` in the UI locale, read once per process.
pub fn tr(msgid: &str) -> &str {
    static ENV: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    tr_in(ENV.get_or_init(ui_locale), msgid)
}

/// Fills a printf-style message: each `%d`, `%s` or `%i` takes the next
/// argument, `%%` is a percent sign. Anything else stays as it is, so a
/// translation with a stray % cannot break the text.
pub fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut args = args.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('%') => {
                chars.next();
                out.push('%');
            }
            Some('d' | 's' | 'i') => {
                chars.next();
                match args.next() {
                    Some(a) => out.push_str(&a.to_string()),
                    None => out.push('?'),
                }
            }
            _ => out.push('%'),
        }
    }
    out
}

/// `tr` and `fill` in one.
pub fn trf(msgid: &str, args: &[&dyn Display]) -> String {
    fill(tr(msgid), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_in_order_and_keeps_percent() {
        assert_eq!(fill("%d of %s", &[&3, &"faces"]), "3 of faces");
        assert_eq!(fill("100 % done", &[]), "100 % done");
    }

    #[test]
    fn regions_find_their_catalog() {
        assert_eq!(tr_in("de_DE.UTF-8", "Lock screen"), "Sperrbildschirm");
        assert_eq!(tr_in("C", "Lock screen"), "Lock screen");
        assert_eq!(tr_in("pt_PT.UTF-8", "Lock screen"), "Lock screen");
    }

    #[test]
    fn every_translation_keeps_its_placeholders() {
        fn marks(s: &str) -> Vec<char> {
            let mut out = Vec::new();
            let mut chars = s.chars();
            while let Some(c) = chars.next() {
                if c == '%'
                    && let Some(n @ ('d' | 's' | 'i')) = chars.next()
                {
                    out.push(n);
                }
            }
            out
        }
        for (lang, catalog) in CATALOGS {
            for (id, text) in *catalog {
                assert_eq!(marks(id), marks(text), "{lang}: {id:?}");
            }
        }
    }
}
