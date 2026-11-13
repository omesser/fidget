//! The shell's reply-language preference, separate from UI translation.
//! --lang overrides the saved setting; System uses the machine's locale.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

pub const FLAG: &str = "--lang";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::Fr, Lang::Es];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::Es => "es",
        }
    }

    pub fn endonym(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
            Lang::Es => "Español",
        }
    }

    pub fn english_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "French",
            Lang::Es => "Spanish",
        }
    }

    pub fn parse(tag: &str) -> Option<Self> {
        let primary = tag
            .trim()
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        Self::ALL.into_iter().find(|lang| lang.code() == primary)
    }

    fn index(self) -> u8 {
        self as u8
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Lang {
    let index = CURRENT.load(Ordering::Relaxed);
    Lang::ALL
        .into_iter()
        .find(|lang| lang.index() == index)
        .unwrap_or(Lang::En)
}

/// The language currently requested by the shell, as a per-wake core argument.
pub fn reply_language() -> Option<&'static str> {
    match current() {
        Lang::En => None,
        other => Some(other.english_name()),
    }
}

pub fn flag() -> Option<Lang> {
    static FLAG_LANG: OnceLock<Option<Lang>> = OnceLock::new();
    *FLAG_LANG.get_or_init(|| match flag_value(std::env::args().skip(1)) {
        Some(value) => {
            let parsed = Lang::parse(&value);
            if parsed.is_none() {
                eprintln!("language: --lang {value} must be en, fr, or es; ignoring it");
            }
            parsed
        }
        None => None,
    })
}

fn flag_value(args: impl Iterator<Item = String>) -> Option<String> {
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == FLAG {
            return args.next();
        }
        if let Some(value) = arg.strip_prefix("--lang=") {
            return Some(value.to_string());
        }
    }
    None
}

fn system() -> Lang {
    sys_locale::get_locales()
        .find_map(|tag| Lang::parse(&tag))
        .unwrap_or(Lang::En)
}

pub fn resolve(setting: &str) -> Lang {
    flag()
        .or_else(|| Lang::parse(setting))
        .unwrap_or_else(system)
}

/// The only mutable language selection lives in the Shell, never fidget-core.
pub fn apply(setting: &str) {
    CURRENT.store(resolve(setting).index(), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_and_names() {
        assert_eq!(Lang::parse("fr-CA"), Some(Lang::Fr));
        assert_eq!(Lang::parse("es_ES.UTF-8"), Some(Lang::Es));
        assert_eq!(Lang::parse("EN-gb"), Some(Lang::En));
        assert_eq!(Lang::parse("de"), None);
        assert_eq!(Lang::Fr.english_name(), "French");
    }

    #[test]
    fn language_flag_accepts_both_spellings() {
        assert_eq!(
            flag_value(["--lang=es".to_string()].into_iter()),
            Some("es".to_string())
        );
        assert_eq!(
            flag_value(["--lang".to_string(), "fr".to_string()].into_iter()),
            Some("fr".to_string())
        );
    }
}
