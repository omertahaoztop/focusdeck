use crate::config::CaptureWords;
use crate::i18n::{msg, t};
use crate::profile::{BoxSpec, Profile};
use chrono::{DateTime, FixedOffset};
use focusdeck_protocol::{Describe, Item};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

pub use focusdeck_protocol::DropParams as DropRequest;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilesResponse {
    pub profiles: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileView {
    pub name: String,
    pub capture_into: Option<String>,
    pub capture_refs: BTreeMap<String, String>,
    pub capture_label_aliases: BTreeMap<String, String>,
    pub capture_words: CaptureWords,
    pub boxes: Vec<BoxView>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoxView {
    pub id: String,
    pub title: String,
    pub plugin: String,
    pub row: u16,
    pub col: u16,
    pub width: u16,
    pub height: u16,
    pub refresh: u64,
    pub plan_into: Option<String>,
    pub describe: Option<Describe>,
    pub error: Option<String>,
}

impl BoxView {
    pub fn from_spec(spec: &BoxSpec, describe: Result<Describe, String>) -> BoxView {
        let (describe, error) = match describe {
            Ok(d) => (Some(d), None),
            Err(message) => (None, Some(message)),
        };
        BoxView {
            id: spec.id.clone(),
            title: spec.title.clone(),
            plugin: spec.plugin.clone(),
            row: spec.row,
            col: spec.col,
            width: spec.width,
            height: spec.height,
            refresh: spec.refresh.as_secs(),
            plan_into: spec.plan_into.clone(),
            describe,
            error,
        }
    }

    fn to_spec(&self) -> BoxSpec {
        BoxSpec {
            id: self.id.clone(),
            title: self.title.clone(),
            plugin: self.plugin.clone(),
            row: self.row,
            col: self.col,
            width: self.width,
            height: self.height,
            refresh: Duration::from_secs(self.refresh.max(1)),
            plan_into: self.plan_into.clone(),
            config: toml::Table::new(),
        }
    }
}

impl ProfileView {
    /// `describes` pairs with `p.boxes` by index; a missing entry becomes an error row.
    pub fn from_profile(
        p: &Profile,
        words: &CaptureWords,
        describes: Vec<Result<Describe, String>>,
    ) -> ProfileView {
        let mut describes = describes.into_iter();
        let boxes = p
            .boxes
            .iter()
            .map(|spec| {
                let describe = describes
                    .next()
                    .unwrap_or_else(|| Err(t(msg::REMOTE_DESCRIBE_UNAVAILABLE)));
                BoxView::from_spec(spec, describe)
            })
            .collect();
        ProfileView {
            name: p.name.clone(),
            capture_into: p.capture_into.clone(),
            capture_refs: p.capture_refs.clone(),
            capture_label_aliases: p.capture_label_aliases.clone(),
            capture_words: words.clone(),
            boxes,
        }
    }

    pub fn to_profile(&self) -> Profile {
        Profile {
            name: self.name.clone(),
            capture_into: self.capture_into.clone(),
            capture_refs: self.capture_refs.clone(),
            capture_label_aliases: self.capture_label_aliases.clone(),
            boxes: self.boxes.iter().map(BoxView::to_spec).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemsResponse {
    pub items: Vec<Item>,
    pub listed_at: DateTime<FixedOffset>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub item: String,
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResponse {
    pub item: Option<Item>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropResponse {
    pub item: Item,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureRequest {
    pub text: String,
    pub at: DateTime<FixedOffset>,
    pub lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureResponse {
    #[serde(rename = "box")]
    pub box_id: String,
    pub item: Item,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoxReminder {
    #[serde(rename = "box")]
    pub box_id: String,
    pub at: DateTime<FixedOffset>,
    pub item: Item,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemindersResponse {
    pub reminders: Vec<BoxReminder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Unauthorized,
    NotFound,
    Invalid,
    Unsupported,
    Plugin,
    InProgress,
    KeyReused,
    Internal,
}

impl ErrorCode {
    pub fn status(self) -> u16 {
        match self {
            ErrorCode::Unauthorized => 401,
            ErrorCode::NotFound => 404,
            ErrorCode::Invalid | ErrorCode::Unsupported | ErrorCode::KeyReused => 422,
            ErrorCode::Plugin => 502,
            ErrorCode::InProgress => 409,
            ErrorCode::Internal => 500,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: ApiErrorBody,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CaptureWords;
    use crate::profile::{BoxSpec, Profile};
    use std::collections::BTreeMap;
    use std::time::Duration;

    fn spec(config: toml::Table) -> BoxSpec {
        BoxSpec {
            id: "notes".into(),
            title: "Notes".into(),
            plugin: "local".into(),
            row: 1,
            col: 2,
            width: 3,
            height: 4,
            refresh: Duration::from_secs(30),
            plan_into: Some("today".into()),
            config,
        }
    }

    fn describe() -> Describe {
        Describe {
            name: "local".into(),
            version: "1".into(),
            protocol: 1,
            actions: vec!["done".into()],
            accepts_drop: true,
            reminders: false,
        }
    }

    #[test]
    fn box_view_never_carries_config() {
        let config: toml::Table = toml::from_str(
            r#"key_file = "~/x.key"
email = "a@b""#,
        )
        .unwrap();
        let view = BoxView::from_spec(&spec(config), Ok(describe()));
        let json = serde_json::to_string(&view).unwrap();
        for leak in ["config", "key_file", "a@b", "x.key"] {
            assert!(!json.contains(leak), "{leak} leaked: {json}");
        }
    }

    #[test]
    fn box_view_serializes_describe_and_error_as_null_when_absent() {
        let ok = serde_json::to_value(BoxView::from_spec(
            &spec(Default::default()),
            Ok(describe()),
        ))
        .unwrap();
        assert!(ok["describe"].is_object() && ok["error"].is_null());
        let err = serde_json::to_value(BoxView::from_spec(
            &spec(Default::default()),
            Err("boom".into()),
        ))
        .unwrap();
        assert!(err["describe"].is_null());
        assert_eq!(err["error"], "boom");
        assert_eq!(err["refresh"], 30);
    }

    #[test]
    fn error_codes_serialize_upper_snake_and_map_status() {
        assert_eq!(
            serde_json::to_string(&ErrorCode::InProgress).unwrap(),
            "\"IN_PROGRESS\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::KeyReused).unwrap(),
            "\"KEY_REUSED\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::NotFound).unwrap(),
            "\"NOT_FOUND\""
        );
        let expected = [
            (ErrorCode::Unauthorized, 401),
            (ErrorCode::NotFound, 404),
            (ErrorCode::Invalid, 422),
            (ErrorCode::Unsupported, 422),
            (ErrorCode::Plugin, 502),
            (ErrorCode::InProgress, 409),
            (ErrorCode::KeyReused, 422),
            (ErrorCode::Internal, 500),
        ];
        for (code, status) in expected {
            assert_eq!(code.status(), status);
        }
        let body = ApiError {
            error: ApiErrorBody {
                code: ErrorCode::Plugin,
                message: "x".into(),
            },
        };
        assert_eq!(
            serde_json::to_string(&body).unwrap(),
            r#"{"error":{"code":"PLUGIN","message":"x"}}"#
        );
    }

    #[test]
    fn zero_refresh_from_the_wire_becomes_one_second() {
        let mut view = BoxView::from_spec(&spec(Default::default()), Ok(describe()));
        view.refresh = 0;
        assert_eq!(view.to_spec().refresh, Duration::from_secs(1));
    }

    #[test]
    fn profile_view_round_trips_to_profile() {
        let profile = Profile {
            name: "pano".into(),
            capture_into: Some("notes".into()),
            capture_refs: BTreeMap::from([("r".to_string(), "redmine".to_string())]),
            capture_label_aliases: BTreeMap::from([("w".to_string(), "work".to_string())]),
            boxes: vec![spec(toml::from_str("a = 1").unwrap())],
        };
        let view =
            ProfileView::from_profile(&profile, &CaptureWords::default(), vec![Ok(describe())]);
        let json = serde_json::to_string(&view).unwrap();
        let back: ProfileView = serde_json::from_str(&json).unwrap();
        let mut expected = profile.clone();
        expected.boxes[0].config = toml::Table::new();
        assert_eq!(back.to_profile(), expected);
    }

    #[test]
    fn capture_and_reminder_use_box_key() {
        let item = Item {
            id: "1".into(),
            title: "t".into(),
            subtitle: None,
            url: None,
            due: None,
            overdue: false,
            source: None,
            description: None,
            labels: vec![],
            from: None,
            links: vec![],
        };
        let resp = CaptureResponse {
            box_id: "notes".into(),
            item,
        };
        assert_eq!(serde_json::to_value(&resp).unwrap()["box"], "notes");
        let action: ActionRequest =
            serde_json::from_str(r#"{"item":"1","action":"done"}"#).unwrap();
        assert!(action.args.is_null());
    }
}
