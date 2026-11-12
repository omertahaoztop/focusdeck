use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, TimeZone};
use focusdeck_protocol::{Item, Link};

use crate::i18n::{msg, t};

#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub title: String,
    pub description: Option<String>,
    pub from: Option<String>,
    pub labels: Vec<String>,
    pub links: Vec<Link>,
    pub due: Option<DateTime<FixedOffset>>,
}

impl Capture {
    pub fn into_item(self) -> Item {
        Item {
            id: String::new(),
            title: self.title,
            subtitle: None,
            url: None,
            due: self.due,
            overdue: false,
            source: None,
            description: self.description,
            labels: self.labels,
            from: self.from,
            links: self.links,
        }
    }
}

pub fn parse(
    text: &str,
    now: DateTime<FixedOffset>,
    refs: &BTreeMap<String, String>,
    aliases: &BTreeMap<String, String>,
) -> Result<Capture, String> {
    let (head, description) = split_description(text);
    let mut words: Vec<&str> = head.split_whitespace().collect();
    let due = take_trailing_time(&mut words, now);

    let mut title: Vec<&str> = Vec::new();
    let mut from = None;
    let mut labels = Vec::new();
    let mut links = Vec::new();
    for word in words {
        if from.is_none()
            && let Some(name) = word.strip_prefix('@').map(|n| n.trim_end_matches(':'))
            && !name.is_empty()
        {
            from = Some(name.to_owned());
        } else if let Some(tag) = word.strip_prefix('#')
            && !tag.is_empty()
            && !tag.bytes().all(|b| b.is_ascii_digit())
        {
            labels.push(aliases.get(tag).map_or(tag, String::as_str).to_owned());
        } else if let Some(link) = ref_link(word, refs) {
            links.push(link);
        } else {
            title.push(word);
        }
    }

    if title.is_empty() {
        return Err(t(msg::CAPTURE_EMPTY_TITLE));
    }
    Ok(Capture {
        title: title.join(" "),
        description,
        from,
        labels,
        links,
        due,
    })
}

fn split_description(text: &str) -> (&str, Option<String>) {
    let mut search_from = 0;
    while let Some(i) = text[search_from..].find("//") {
        let at = search_from + i;
        if at == 0 || text[..at].ends_with(char::is_whitespace) {
            let desc = text[at + 2..].trim();
            let desc = (!desc.is_empty()).then(|| desc.to_owned());
            return (&text[..at], desc);
        }
        search_from = at + 2;
    }
    (text, None)
}

fn ref_link(word: &str, refs: &BTreeMap<String, String>) -> Option<Link> {
    refs.iter().find_map(|(prefix, template)| {
        let digits = word.strip_prefix(prefix.as_str())?;
        (digits.len() >= 2 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| Link {
            label: word.to_owned(),
            url: template.replace("{}", digits),
        })
    })
}

enum Spec {
    After(Duration),
    Clock { hour: u32, minute: u32 },
}

enum Day {
    Tomorrow,
    Weekday(u32),
}

fn take_trailing_time(
    words: &mut Vec<&str>,
    now: DateTime<FixedOffset>,
) -> Option<DateTime<FixedOffset>> {
    let spec = parse_spec(words.last()?)?;
    let (day, consumed) = match (&spec, words.len().checked_sub(2).map(|i| words[i])) {
        (Spec::Clock { .. }, Some(w)) => parse_day(w).map_or((None, 1), |d| (Some(d), 2)),
        _ => (None, 1),
    };
    let due = resolve(&spec, day, now)?;
    words.truncate(words.len() - consumed);
    Some(due)
}

fn parse_spec(word: &str) -> Option<Spec> {
    if let Some((h, m)) = word.split_once(':') {
        let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        if !(1..=2).contains(&h.len()) || m.len() != 2 || !digits(h) || !digits(m) {
            return None;
        }
        let (hour, minute) = (h.parse().ok()?, m.parse().ok()?);
        return (hour <= 23 && minute <= 59).then_some(Spec::Clock { hour, minute });
    }
    let (digits, per_unit_minutes) = match (word.strip_suffix("dk"), word.strip_suffix('s')) {
        (Some(d), _) => (d, 1),
        (None, Some(d)) => (d, 60),
        _ => return None,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes = i64::from(digits.parse::<u32>().ok()?) * per_unit_minutes;
    Some(Spec::After(Duration::try_minutes(minutes)?))
}

fn parse_day(word: &str) -> Option<Day> {
    Some(match word.to_lowercase().as_str() {
        "yarın" | "yarin" => Day::Tomorrow,
        "pzt" | "pts" => Day::Weekday(0),
        "sal" => Day::Weekday(1),
        "çar" | "car" => Day::Weekday(2),
        "per" | "prş" | "prs" => Day::Weekday(3),
        "cum" => Day::Weekday(4),
        "cmt" => Day::Weekday(5),
        "paz" => Day::Weekday(6),
        _ => return None,
    })
}

fn resolve(
    spec: &Spec,
    day: Option<Day>,
    now: DateTime<FixedOffset>,
) -> Option<DateTime<FixedOffset>> {
    let (hour, minute) = match *spec {
        Spec::After(d) => return now.checked_add_signed(d),
        Spec::Clock { hour, minute } => (hour, minute),
    };
    let today = now.date_naive();
    let at = |date: NaiveDate| {
        now.timezone()
            .from_local_datetime(&date.and_hms_opt(hour, minute, 0)?)
            .single()
    };
    let add_days = |date: NaiveDate, n: u64| date.checked_add_days(chrono::Days::new(n));
    match day {
        None => {
            let candidate = at(today)?;
            if candidate > now {
                Some(candidate)
            } else {
                at(add_days(today, 1)?)
            }
        }
        Some(Day::Tomorrow) => at(add_days(today, 1)?),
        Some(Day::Weekday(target)) => {
            let ahead = (target + 7 - today.weekday().num_days_from_monday()) % 7;
            let candidate = at(add_days(today, u64::from(ahead))?)?;
            if candidate > now {
                Some(candidate)
            } else {
                at(add_days(today, u64::from(ahead) + 7)?)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-12T10:00:00+03:00").unwrap()
    }

    fn at(s: &str) -> Option<DateTime<FixedOffset>> {
        Some(DateTime::parse_from_rfc3339(s).unwrap())
    }

    fn p(text: &str) -> Result<Capture, String> {
        let refs = BTreeMap::from([(
            "R".to_owned(),
            "https://redmine.example.com/issues/{}".to_owned(),
        )]);
        let aliases = BTreeMap::from([("tel".to_owned(), "telefon".to_owned())]);
        parse(text, now(), &refs, &aliases)
    }

    #[test]
    fn plain_title() {
        let c = p("Ali'ye VPN logu at").unwrap();
        assert_eq!(c.title, "Ali'ye VPN logu at");
        assert_eq!(c.due, None);
    }

    #[test]
    fn relative_durations() {
        let c = p("log at 30dk").unwrap();
        assert_eq!(c.due, at("2026-11-12T10:30:00+03:00"));
        assert_eq!(c.title, "log at");
        assert_eq!(p("log at 2s").unwrap().due, at("2026-11-12T12:00:00+03:00"));
    }

    #[test]
    fn clock_today() {
        let c = p("log at 14:30").unwrap();
        assert_eq!(c.due, at("2026-11-12T14:30:00+03:00"));
        assert_eq!(c.title, "log at");
    }

    #[test]
    fn past_time_rolls_to_tomorrow() {
        assert_eq!(
            p("log at 09:15").unwrap().due,
            at("2026-11-13T09:15:00+03:00")
        );
        assert_eq!(
            p("log at 23:59").unwrap().due,
            at("2026-11-12T23:59:00+03:00")
        );
    }

    #[test]
    fn tomorrow_and_weekday() {
        for t in ["log at yarın 09:00", "log at yarin 09:00"] {
            let c = p(t).unwrap();
            assert_eq!(c.due, at("2026-11-13T09:00:00+03:00"));
            assert_eq!(c.title, "log at");
        }
        assert_eq!(
            p("log at pzt 09:00").unwrap().due,
            at("2026-11-16T09:00:00+03:00")
        );
        assert_eq!(
            p("log at ÇAR 09:00").unwrap().due,
            at("2026-11-18T09:00:00+03:00")
        );
        assert_eq!(
            p("log at car 09:00").unwrap().due,
            at("2026-11-18T09:00:00+03:00")
        );
    }

    #[test]
    fn same_weekday_past_rolls_a_week() {
        assert_eq!(
            p("x per 09:00").unwrap().due,
            at("2026-11-19T09:00:00+03:00")
        );
        assert_eq!(
            p("x per 14:00").unwrap().due,
            at("2026-11-12T14:00:00+03:00")
        );
    }

    #[test]
    fn from_marks_first_at_only() {
        let c = p("@ali: log at @veli").unwrap();
        assert_eq!(c.from.as_deref(), Some("ali"));
        assert_eq!(c.title, "log at @veli");
        assert_eq!(p("log @ali").unwrap().from.as_deref(), Some("ali"));
    }

    #[test]
    fn labels_use_aliases() {
        let c = p("log #tel #mail").unwrap();
        assert_eq!(c.labels, ["telefon", "mail"]);
        assert_eq!(c.title, "log");
    }

    #[test]
    fn numeric_hash_stays_in_title() {
        let c = p("bak #123").unwrap();
        assert_eq!(c.title, "bak #123");
        assert!(c.labels.is_empty());
    }

    #[test]
    fn ref_becomes_link() {
        let c = p("S3 R45539 bak").unwrap();
        assert_eq!(
            c.links,
            [Link {
                label: "R45539".into(),
                url: "https://redmine.example.com/issues/45539".into()
            }]
        );
        assert_eq!(c.title, "S3 bak");
    }

    #[test]
    fn short_ref_stays_in_title() {
        let c = p("S3 R4 bak").unwrap();
        assert!(c.links.is_empty());
        assert_eq!(c.title, "S3 R4 bak");
    }

    #[test]
    fn description_after_double_slash() {
        let c = p("başlık // ayrıntı burada").unwrap();
        assert_eq!(c.title, "başlık");
        assert_eq!(c.description.as_deref(), Some("ayrıntı burada"));
    }

    #[test]
    fn url_double_slash_is_not_description() {
        let c = p("bak https://x.y/z").unwrap();
        assert_eq!(c.title, "bak https://x.y/z");
        assert_eq!(c.description, None);
    }

    #[test]
    fn time_before_description_is_parsed() {
        let c = p("log 14:30 // ayrıntı").unwrap();
        assert_eq!(c.due, at("2026-11-12T14:30:00+03:00"));
        assert_eq!(c.description.as_deref(), Some("ayrıntı"));
    }

    #[test]
    fn empty_title_is_error() {
        assert_eq!(p("@ali 14:30").unwrap_err(), "title is empty");
        assert_eq!(p("   ").unwrap_err(), "title is empty");
    }

    #[test]
    fn invalid_clock_stays_in_title() {
        let c = p("log 25:00").unwrap();
        assert_eq!(c.due, None);
        assert_eq!(c.title, "log 25:00");
    }

    #[test]
    fn time_only_at_end() {
        let c = p("14:30 log").unwrap();
        assert_eq!(c.due, None);
        assert_eq!(c.title, "14:30 log");
    }

    #[test]
    fn into_item_maps_fields() {
        let item = p("@ali log #tel R45539 14:30 // not").unwrap().into_item();
        assert_eq!(item.id, "");
        assert!(!item.overdue);
        assert_eq!(item.title, "log");
        assert_eq!(item.from.as_deref(), Some("ali"));
        assert_eq!(item.labels, ["telefon"]);
        assert_eq!(item.links.len(), 1);
        assert_eq!(item.description.as_deref(), Some("not"));
        assert_eq!(item.due, at("2026-11-12T14:30:00+03:00"));
    }
}
