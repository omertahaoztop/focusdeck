pub mod build;
mod check;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

pub use check::check_locales;

pub type Sources = &'static [(&'static str, &'static str)];

pub const CORE_ENV: [&str; 3] = ["LC_ALL", "LC_MESSAGES", "LANG"];
pub const PLUGIN_ENV: [&str; 4] = ["FOCUSDECK_LANG", "LC_ALL", "LC_MESSAGES", "LANG"];

const FALLBACK: &str = "en";

pub fn normalize(raw: &str) -> Option<String> {
    let base = raw.split(['.', '@']).next()?.trim().replace('_', "-");
    if base.is_empty() || base.eq_ignore_ascii_case("c") || base.eq_ignore_ascii_case("posix") {
        return None;
    }
    base.parse::<LanguageIdentifier>()
        .ok()
        .map(|id| id.to_string())
}

pub fn requested(
    config_language: Option<&str>,
    get: impl Fn(&str) -> Option<String>,
    vars: &[&str],
) -> Option<String> {
    config_language.and_then(normalize).or_else(|| {
        vars.iter()
            .find_map(|var| get(var).as_deref().and_then(normalize))
    })
}

pub enum Arg<'a> {
    Str(&'a str),
    Num(i64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Self {
        Arg::Str(value)
    }
}

impl From<i64> for Arg<'_> {
    fn from(value: i64) -> Self {
        Arg::Num(value)
    }
}

impl From<i32> for Arg<'_> {
    fn from(value: i32) -> Self {
        Arg::Num(i64::from(value))
    }
}

impl From<usize> for Arg<'_> {
    fn from(value: usize) -> Self {
        Arg::Num(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

pub struct Localizer {
    chain: Vec<(String, FluentBundle<FluentResource>)>,
}

impl Localizer {
    pub fn new(requested: Option<&str>, sources: Sources) -> Localizer {
        let mut tags: Vec<String> = Vec::new();
        if let Some(tag) = requested.and_then(normalize)
            && let Ok(id) = tag.parse::<LanguageIdentifier>()
        {
            tags.push(id.language.as_str().to_owned());
            tags.insert(0, tag);
        }
        tags.dedup();
        tags.retain(|tag| tag != FALLBACK);
        tags.push(FALLBACK.to_owned());

        let chain = tags
            .into_iter()
            .filter_map(|tag| {
                let text = match sources.iter().find(|(label, _)| *label == tag) {
                    Some((_, text)) => text,
                    None if tag == FALLBACK => "",
                    None => return None,
                };
                let bundle = bundle(&tag, text);
                Some((tag, bundle))
            })
            .collect();
        Localizer { chain }
    }

    pub fn lang(&self) -> &str {
        &self.chain[0].0
    }

    pub fn t(&self, id: &str) -> String {
        self.format(id, None)
    }

    pub fn t_args(&self, id: &str, args: &[(&str, Arg<'_>)]) -> String {
        let mut fluent_args = FluentArgs::new();
        for (name, arg) in args {
            match arg {
                Arg::Str(value) => fluent_args.set(*name, FluentValue::from(*value)),
                Arg::Num(value) => fluent_args.set(*name, FluentValue::from(*value)),
            }
        }
        self.format(id, Some(&fluent_args))
    }

    pub fn words(&self, id: &str) -> Vec<String> {
        self.t(id)
            .split_whitespace()
            .map(str::to_lowercase)
            .collect()
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        for (_, bundle) in &self.chain {
            let Some(pattern) = bundle.get_message(id).and_then(|m| m.value()) else {
                continue;
            };
            let mut errors = Vec::new();
            return bundle
                .format_pattern(pattern, args, &mut errors)
                .into_owned();
        }
        id.to_owned()
    }
}

fn bundle(tag: &str, text: &str) -> FluentBundle<FluentResource> {
    let langid = tag
        .parse::<LanguageIdentifier>()
        .unwrap_or_else(|_| LanguageIdentifier::from_bytes(FALLBACK.as_bytes()).expect("en"));
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    bundle.set_use_isolating(false);
    let resource = match FluentResource::try_new(text.to_owned()) {
        Ok(resource) => resource,
        Err((partial, _)) => partial,
    };
    bundle.add_resource_overriding(resource);
    bundle
}

#[macro_export]
macro_rules! localized {
    () => {
        include!(concat!(env!("OUT_DIR"), "/focusdeck_i18n.rs"));

        static __FOCUSDECK_I18N_STATE: ::std::sync::OnceLock<(
            ::std::option::Option<::std::string::String>,
            $crate::Localizer,
        )> = ::std::sync::OnceLock::new();
        static __FOCUSDECK_I18N_ENGLISH: ::std::sync::LazyLock<$crate::Localizer> =
            ::std::sync::LazyLock::new(|| localizer(::std::option::Option::None));

        #[allow(dead_code)]
        pub fn init(requested: ::std::option::Option<::std::string::String>) {
            let requested = requested.and_then(|raw| $crate::normalize(&raw));
            let localizer = $crate::Localizer::new(requested.as_deref(), SOURCES);
            let _ = __FOCUSDECK_I18N_STATE.set((requested, localizer));
        }

        #[allow(dead_code)]
        pub fn requested() -> ::std::option::Option<&'static str> {
            __FOCUSDECK_I18N_STATE
                .get()
                .and_then(|(requested, _)| requested.as_deref())
        }

        #[allow(dead_code)]
        pub fn current() -> &'static $crate::Localizer {
            match __FOCUSDECK_I18N_STATE.get() {
                ::std::option::Option::Some((_, localizer)) => localizer,
                ::std::option::Option::None => &__FOCUSDECK_I18N_ENGLISH,
            }
        }

        #[allow(dead_code)]
        pub fn localizer(requested: ::std::option::Option<&str>) -> $crate::Localizer {
            $crate::Localizer::new(requested, SOURCES)
        }

        #[allow(dead_code)]
        pub fn t(id: &str) -> ::std::string::String {
            current().t(id)
        }

        #[allow(dead_code)]
        pub fn t_args(id: &str, args: &[(&str, $crate::Arg<'_>)]) -> ::std::string::String {
            current().t_args(id, args)
        }
    };
}
