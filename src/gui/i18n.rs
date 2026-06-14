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

        // Only load the default locale initially
        Self::load_locale(&mut bundles, default_locale);

        Self {
            current_locale: default_locale.to_string(),
            bundles,
        }
    }

    fn load_locale(bundles: &mut HashMap<String, FluentBundle<FluentResource>>, locale: &str) {
        if bundles.contains_key(locale) {
            return; // Already loaded
        }

        let path = format!("{}/main.ftl", locale);

        let file =
            Localizations::get(&path).unwrap_or_else(|| panic!("Missing locale file: {}", path));

        let source = match file.data {
            Cow::Borrowed(bytes) => std::str::from_utf8(bytes).unwrap().to_string(),
            Cow::Owned(bytes) => String::from_utf8(bytes).unwrap(),
        };

        let resource = FluentResource::try_new(source).expect("Failed to parse Fluent resource");

        let langid: LanguageIdentifier = locale.parse().expect("Invalid language identifier");

        let mut bundle = FluentBundle::new(vec![langid]);

        bundle
            .add_resource(resource)
            .expect("Failed to add Fluent resource");

        bundles.insert(locale.to_string(), bundle);
    }

    pub fn set_locale(&mut self, locale: &str) {
        // Load the locale if it's not already loaded
        Self::load_locale(&mut self.bundles, locale);

        if self.bundles.contains_key(locale) {
            self.current_locale = locale.to_string();
        }
    }

    pub fn current_locale(&self) -> &str {
        &self.current_locale
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
