//! The interface language: which one is in force, and the catalogs that turn
//! an English source string into it.
//!
//! English is the source text, so it has no catalog and is what a string a
//! catalog lacks falls back to. `--lang` outranks the Language setting, which
//! outranks the system's preferred languages. The webviews read the same
//! catalogs, handed over by `init_script` and `EVENT`, so the Rust and the
//! page halves of one screen cannot disagree about a word.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// The event that carries `payload()` to every webview when the language changes.
pub const EVENT: &str = "locale-changed";

/// The launch flag that pins the language, as `--lang fr` or `--lang=fr`.
pub const FLAG: &str = "--lang";

/// A language the interface is translated into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
    Es,
}

impl Lang {
    /// Every language, in the order the picker offers them.
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Fr, Lang::Es];

    /// The ISO 639-1 code, which is what the settings file stores.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::Es => "es",
        }
    }

    /// The language's name in that language: a picker offers it to someone who
    /// may not read the one in force.
    pub fn endonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::Es => "Español",
        }
    }

    /// The language's name in English, for the line that asks the model to
    /// speak it.
    pub fn english_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "French",
            Lang::Es => "Spanish",
        }
    }

    /// Reads a code or a locale tag (`fr`, `fr-CA`, `es_ES.UTF-8`), ignoring case.
    pub fn parse(tag: &str) -> Option<Self> {
        let primary = tag
            .trim()
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        Self::ALL.into_iter().find(|lang| lang.code() == primary)
    }

    fn catalog_json(self) -> &'static str {
        match self {
            Lang::En => "{}",
            Lang::Fr => include_str!("../../src/locales/fr.json"),
            Lang::Es => include_str!("../../src/locales/es.json"),
        }
    }

    fn index(self) -> u8 {
        self as u8
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The language in force.
pub fn current() -> Lang {
    let index = CURRENT.load(Ordering::Relaxed);
    Lang::ALL
        .into_iter()
        .find(|lang| lang.index() == index)
        .unwrap_or(Lang::En)
}

/// Put `lang` in force for every string translated from here on. Also the
/// language the model is asked to speak, so a reply matches the menus.
pub fn set(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
    fidget_core::director::set_reply_language(match lang {
        Lang::En => None,
        other => Some(other.english_name()),
    });
}

/// The language `--lang` pins, read once from this process's arguments.
pub fn flag() -> Option<Lang> {
    static FLAG_LANG: OnceLock<Option<Lang>> = OnceLock::new();
    *FLAG_LANG.get_or_init(|| match flag_value(std::env::args().skip(1)) {
        Some(value) => {
            let lang = Lang::parse(&value);
            if lang.is_none() {
                eprintln!("i18n: {FLAG} {value} is not one of en, fr, es; ignoring it");
            }
            lang
        }
        None => None,
    })
}

/// The value after `--lang`, in either spelling.
fn flag_value(args: impl Iterator<Item = String>) -> Option<String> {
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if arg == FLAG {
            return args.next();
        }
        if let Some(value) = arg
            .strip_prefix(FLAG)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Some(value.to_string());
        }
    }
    None
}

/// The first of the system's preferred languages that has a translation.
pub fn system() -> Lang {
    sys_locale::get_locales()
        .find_map(|tag| Lang::parse(&tag))
        .unwrap_or(Lang::En)
}

/// The language a stored setting means: `--lang` first, then the setting, then
/// the system. A blank or unknown setting is System.
pub fn resolve(setting: &str) -> Lang {
    flag()
        .or_else(|| Lang::parse(setting))
        .unwrap_or_else(system)
}

/// Put the language `setting` resolves to in force. True when that changed it.
pub fn apply(setting: &str) -> bool {
    let lang = resolve(setting);
    let changed = lang != current();
    set(lang);
    changed
}

fn catalog(lang: Lang) -> &'static HashMap<String, String> {
    static CATALOGS: [OnceLock<HashMap<String, String>>; 3] =
        [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    CATALOGS[lang.index() as usize].get_or_init(|| {
        serde_json::from_str(lang.catalog_json()).expect("a locale catalog is a JSON string map")
    })
}

/// `en` in `lang`, or `en` itself where the catalog has no entry.
pub fn translate(lang: Lang, en: &str) -> &str {
    catalog(lang).get(en).map(String::as_str).unwrap_or(en)
}

/// `en` in the language in force.
pub fn tr(en: &str) -> String {
    translate(current(), en).to_string()
}

/// `en` in the language in force, with each `{name}` filled from `args`.
/// Named, not positional, because a translation may reorder them.
pub fn trf(en: &str, args: &[(&str, &str)]) -> String {
    fill(translate(current(), en), args)
}

fn fill(template: &str, args: &[(&str, &str)]) -> String {
    args.iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

/// What a webview needs to speak the language in force: its code and catalog.
pub fn payload() -> serde_json::Value {
    let lang = current();
    serde_json::json!({ "lang": lang.code(), "strings": catalog(lang) })
}

/// The script every webview runs before its own, so the first paint is
/// already in the language in force.
pub fn init_script() -> String {
    format!("window.__FIDGET_LOCALE__ = {};", payload())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> impl Iterator<Item = String> {
        list.iter()
            .map(|arg| arg.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn a_locale_tag_reads_as_its_language() {
        assert_eq!(Lang::parse("fr"), Some(Lang::Fr));
        assert_eq!(Lang::parse("fr-CA"), Some(Lang::Fr));
        assert_eq!(Lang::parse("es_ES.UTF-8"), Some(Lang::Es));
        assert_eq!(Lang::parse("EN-us"), Some(Lang::En));
        assert_eq!(Lang::parse("de-DE"), None);
        assert_eq!(Lang::parse(""), None);
        assert_eq!(Lang::parse("C"), None);
    }

    #[test]
    fn the_flag_takes_either_spelling() {
        assert_eq!(flag_value(args(&["--lang", "fr"])), Some("fr".into()));
        assert_eq!(flag_value(args(&["--x", "--lang=es"])), Some("es".into()));
        assert_eq!(flag_value(args(&["--language", "fr"])), None);
        assert_eq!(flag_value(args(&["--lang"])), None);
    }

    #[test]
    fn a_missing_entry_falls_back_to_english() {
        assert_eq!(
            translate(Lang::Fr, "no catalog has this"),
            "no catalog has this"
        );
        assert_eq!(translate(Lang::En, "Settings…"), "Settings…");
        assert_eq!(translate(Lang::Fr, "Settings…"), "Réglages…");
        assert_eq!(translate(Lang::Es, "Settings…"), "Ajustes…");
    }

    #[test]
    fn placeholders_fill_by_name_in_any_order() {
        assert_eq!(fill("{b} then {a}", &[("a", "1"), ("b", "2")]), "2 then 1");
    }

    #[test]
    fn every_catalog_parses_and_keeps_its_placeholders() {
        let names = |text: &str| {
            let mut found: Vec<String> = text
                .split('{')
                .skip(1)
                .filter_map(|rest| rest.split_once('}').map(|(name, _)| name.to_string()))
                .filter(|name| name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
                .collect();
            found.sort();
            found
        };
        for lang in [Lang::Fr, Lang::Es] {
            for (en, translated) in catalog(lang) {
                assert_eq!(
                    names(en),
                    names(translated),
                    "{} drops or adds a placeholder in {en:?}",
                    lang.code()
                );
            }
        }
    }
}
