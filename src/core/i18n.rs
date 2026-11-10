use fluent_bundle::{FluentBundle, FluentResource};
use rust_embed::RustEmbed;
use std::borrow::Cow;
use std::collections::HashMap;
use unic_langid::LanguageIdentifier;

#[derive(RustEmbed)]
#[folder = "locales/"]
struct Localizations;

const FALLBACK_LOCALE: &str = "en-US";

pub struct I18n {
    current_locale: String,
    bundles: HashMap<String, FluentBundle<FluentResource>>,
}

impl I18n {
    pub fn new(default_locale: &str) -> Self {
        let mut bundles = HashMap::new();

        Self::load_locale(&mut bundles, FALLBACK_LOCALE);
        Self::load_locale(&mut bundles, default_locale);

        Self {
            current_locale: default_locale.to_string(),
            bundles,
        }
    }

    fn load_locale(bundles: &mut HashMap<String, FluentBundle<FluentResource>>, locale: &str) {
        if locale.is_empty() {
            return;
        }

        if bundles.contains_key(locale) {
            return;
        }

        let path = format!("{}/main.ftl", locale);

        let file = match Localizations::get(&path) {
            Some(f) => f,
            None => {
                eprintln!("Missing locale file: {}", path);
                return;
            }
        };

        let source = match file.data {
            Cow::Borrowed(bytes) => std::str::from_utf8(bytes).unwrap().to_string(),
            Cow::Owned(bytes) => String::from_utf8(bytes).unwrap(),
        };

        let resource = match FluentResource::try_new(source) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to parse Fluent resource for {}: {:?}", locale, e);
                return;
            }
        };

        let langid: LanguageIdentifier = match locale.parse() {
            Ok(id) => id,
            Err(e) => {
                eprintln!("Invalid language identifier '{}': {:?}", locale, e);
                return;
            }
        };

        let mut bundle = FluentBundle::new(vec![langid]);

        if let Err(e) = bundle.add_resource(resource) {
            eprintln!("Failed to add Fluent resource for {}: {:?}", locale, e);
            return;
        }

        bundles.insert(locale.to_string(), bundle);
    }

    pub fn set_locale(&mut self, locale: &str) {
        if locale.is_empty() {
            return;
        }

        Self::load_locale(&mut self.bundles, locale);

        if self.bundles.contains_key(locale) {
            self.current_locale = locale.to_string();
        }
    }

    pub fn tr(&self, key: &str) -> String {
        self.lookup(&self.current_locale, key)
            .or_else(|| self.lookup(FALLBACK_LOCALE, key))
            .unwrap_or_else(|| key.to_string())
    }

    fn lookup(&self, locale: &str, key: &str) -> Option<String> {
        let bundle = self.bundles.get(locale)?;
        let message = bundle.get_message(key)?;
        let pattern = message.value()?;

        let mut errors = vec![];

        Some(
            bundle
                .format_pattern(pattern, None, &mut errors)
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_from(locale: &str, source: &str) -> FluentBundle<FluentResource> {
        let mut bundle = FluentBundle::new(vec![locale.parse().unwrap()]);
        bundle
            .add_resource(FluentResource::try_new(source.to_string()).unwrap())
            .unwrap();
        bundle
    }

    /// Top-level message ids in a `.ftl` source (lines starting at column 0 with `id =`).
    fn message_keys(source: &str) -> std::collections::BTreeSet<String> {
        source
            .lines()
            .filter(|line| line.starts_with(|c: char| c.is_ascii_alphabetic()))
            .filter_map(|line| line.split_once('='))
            .map(|(key, _)| key.trim_end().to_string())
            .filter(|key| {
                key.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .collect()
    }

    fn embedded_source(locale: &str) -> String {
        let file = Localizations::get(&format!("{locale}/main.ftl"))
            .unwrap_or_else(|| panic!("missing locales/{locale}/main.ftl"));
        String::from_utf8(file.data.into_owned()).unwrap()
    }

    #[test]
    fn missing_key_in_locale_falls_back_to_en_us() {
        let mut bundles = HashMap::new();
        bundles.insert(
            FALLBACK_LOCALE.to_string(),
            bundle_from(
                FALLBACK_LOCALE,
                "only_english = Only in English\nshared = Shared",
            ),
        );
        bundles.insert("ja-JP".to_string(), bundle_from("ja-JP", "shared = 共有"));
        let i18n = I18n {
            current_locale: "ja-JP".to_string(),
            bundles,
        };

        assert_eq!(i18n.tr("shared"), "共有");
        assert_eq!(i18n.tr("only_english"), "Only in English");
        assert_eq!(i18n.tr("nowhere"), "nowhere");
    }

    #[test]
    fn every_en_us_key_exists_in_every_locale() {
        let english = message_keys(&embedded_source(FALLBACK_LOCALE));
        assert!(english.len() > 100, "key extraction found too few keys");

        let mut checked = 0;
        for path in Localizations::iter() {
            let Some(locale) = path.strip_suffix("/main.ftl") else {
                continue;
            };
            checked += 1;
            let keys = message_keys(&embedded_source(locale));
            let missing: Vec<&String> = english.difference(&keys).collect();
            assert!(missing.is_empty(), "{locale} is missing keys: {missing:?}");
        }
        assert!(checked >= 6, "expected all locales, found {checked}");
    }
}
