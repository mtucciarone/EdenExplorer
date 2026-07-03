use fluent_bundle::{FluentBundle, FluentResource};
use rust_embed::RustEmbed;
use std::borrow::Cow;
use std::collections::HashMap;
use unic_langid::LanguageIdentifier;

#[derive(RustEmbed)]
#[folder = "locales/"]
struct Localizations;

pub struct I18n {
    current_locale: String,
    bundles: HashMap<String, FluentBundle<FluentResource>>,
}

impl I18n {
    pub fn new(default_locale: &str) -> Self {
        let mut bundles = HashMap::new();

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
        let bundle = match self.bundles.get(&self.current_locale) {
            Some(bundle) => bundle,
            None => return key.to_string(),
        };

        let message = match bundle.get_message(key) {
            Some(message) => message,
            None => return key.to_string(),
        };

        let pattern = match message.value() {
            Some(value) => value,
            None => return key.to_string(),
        };

        let mut errors = vec![];

        bundle
            .format_pattern(pattern, None, &mut errors)
            .to_string()
    }
}
