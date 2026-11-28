//! V2C's cloud API, used for one thing only: the charger's charging hours.
//! They are stored in the charger when the household saves them, and read
//! from it when someone opens the hours editor (the local API has neither).
//! The charger keeps them and follows them by itself, with or without
//! internet (AGENTS.md rule 7). Every other reading and control, and
//! switching the hours on and off, stay on the local API.
//!
//! Facts and sources are in this crate's `README.md` ("Charging hours").
//!
//! The requests this module can send are a closed list: [`CloudRequest`].
//! There is no way to reach the cloud's other endpoints (contracted power,
//! dynamic power control, current limits, ...) from here.

use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, HeaderValue};
use vesta_core::{ChargingWindow, DriverError, TimeOfDay, Weekday};

/// The cloud's production address.
pub(crate) const CLOUD_BASE: &str = "https://v2c.cloud/kong/v2c_service";
/// Largest answer we accept.
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
/// The charger's two timers, as the cloud numbers them (`timerId`). Not
/// documented: V2C's own example uses `1`, the fields the cloud reports
/// end in `_00` and `_01`, a community client says "0 or 1". With 0 and 1,
/// a wrong guess fails at the first write (nothing changes) instead of
/// changing one timer; the read-back after saving catches the rest.
pub(crate) const TIMER_IDS: [u8; 2] = [0, 1];

/// Every request VESTA may send to the cloud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CloudRequest {
    /// `POST /device/timer?deviceId=…&timerId=…` with
    /// `{"timeStart":"HH:MM","timeEnd":"HH:MM","daysOfWeek":"1234567"}`.
    Timer { slot: u8, window: ChargingWindow },
    /// `GET /device/reported?deviceId=…`: the charger's last reported
    /// state, for its timers. Read back after saving, and when someone asks
    /// to see the hours.
    Reported,
}

impl CloudRequest {
    pub(crate) fn path(&self) -> &'static str {
        match self {
            Self::Timer { .. } => "/device/timer",
            Self::Reported => "/device/reported",
        }
    }
}

/// `daysOfWeek`: digits 1 (Monday) … 7 (Sunday), in week order.
pub(crate) fn days_of_week(days: &[Weekday]) -> String {
    let mut sorted = days.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    sorted
        .iter()
        .map(|day| char::from(b'1' + day.index() as u8))
        .collect()
}

/// The body of a timer write.
pub(crate) fn timer_body(window: &ChargingWindow) -> serde_json::Value {
    serde_json::json!({
        "timeStart": window.start.to_string(),
        "timeEnd": window.end.to_string(),
        "daysOfWeek": days_of_week(&window.days),
    })
}

/// A JSON value as a whole number: ints and numeric strings.
fn whole(value: &serde_json::Value) -> Option<u16> {
    match value {
        serde_json::Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Days from a `daysofweek` value ("135", 135, …). `None` when it is not
/// days (e.g. "8": the timer is not set).
fn parse_days(value: &serde_json::Value) -> Option<Vec<Weekday>> {
    let text = match value {
        serde_json::Value::String(s) => s.trim().to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let mut days = Vec::new();
    for c in text.chars() {
        let digit = c.to_digit(10)?;
        let day = Weekday::ALL.get(usize::try_from(digit).ok()?.checked_sub(1)?)?;
        days.push(*day);
    }
    days.sort_unstable();
    days.dedup();
    (!days.is_empty()).then_some(days)
}

/// One timer as the cloud reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReportedTimer {
    Set(ChargingWindow),
    NotSet,
}

/// The charger's timers from `/device/reported` (fields `hourstart_00`,
/// `minutestart_00`, `hourend_00`, `minuteend_00`, `daysofweek_00`, and
/// `_01`; days "8" = not set). `None` when the answer does not say: the
/// fields are not documented and may be missing. Nothing else in the
/// answer is used.
pub(crate) fn reported_timers(body: &[u8]) -> Option<Vec<ReportedTimer>> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    // Some answers carry the JSON object as text inside a JSON string.
    let value = match value {
        serde_json::Value::String(text) => serde_json::from_str(&text).ok()?,
        other => other,
    };
    // Some answers wrap the object in a list or a `data` field.
    let object = match &value {
        serde_json::Value::Array(items) => items.first()?,
        other => other.get("data").unwrap_or(other),
    }
    .as_object()?;
    let mut timers = Vec::new();
    for slot in 0..TIMER_IDS.len() {
        let field = |name: &str| object.get(&format!("{name}_{slot:02}"));
        let days = field("daysofweek")?;
        if whole(days) == Some(8) {
            timers.push(ReportedTimer::NotSet);
            continue;
        }
        let time = |hour: &str, minute: &str| {
            let hour = u8::try_from(whole(field(hour)?)?).ok()?;
            let minute = u8::try_from(whole(field(minute)?)?).ok()?;
            TimeOfDay::new(hour, minute).ok()
        };
        timers.push(ReportedTimer::Set(ChargingWindow {
            start: time("hourstart", "minutestart")?,
            end: time("hourend", "minuteend")?,
            days: parse_days(days)?,
        }));
    }
    Some(timers)
}

/// The TLS set-up for the cloud (and, unused, for the local client: the
/// HTTP library needs one): ring, Mozilla's roots, TLS 1.2+.
pub(crate) fn tls_config() -> Result<rustls::ClientConfig, DriverError> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| DriverError::InvalidConfig(format!("cannot set up TLS: {e}")))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(config)
}

/// The cloud, for one charger's owner (their API key).
pub(crate) struct Cloud {
    base: String,
    key: HeaderValue,
    client: reqwest::Client,
}

impl std::fmt::Debug for Cloud {
    /// Never the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cloud").finish_non_exhaustive()
    }
}

impl Cloud {
    pub(crate) fn new(base: &str, api_key: &str, timeout: Duration) -> Result<Self, DriverError> {
        let mut key = HeaderValue::from_str(api_key.trim())
            .map_err(|_| DriverError::InvalidConfig("cloud_api_key: not a valid key".to_owned()))?;
        key.set_sensitive(true);
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .https_only(base.starts_with("https://"))
            .timeout(timeout)
            .connect_timeout(timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("vesta")
            .build()
            .map_err(|e| {
                DriverError::InvalidConfig(format!("cannot create the HTTP client: {e}"))
            })?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            key,
            client,
        })
    }

    /// Sends one request and returns the answer's body.
    pub(crate) async fn send(
        &self,
        device_id: &str,
        request: &CloudRequest,
    ) -> Result<Vec<u8>, DriverError> {
        let mut url = reqwest::Url::parse(&format!("{}{}", self.base, request.path()))
            .map_err(|e| DriverError::InvalidConfig(format!("cloud address: {e}")))?;
        url.query_pairs_mut().append_pair("deviceId", device_id);
        let builder = match request {
            CloudRequest::Timer { slot, window } => {
                url.query_pairs_mut()
                    .append_pair("timerId", &slot.to_string());
                self.client
                    .post(url)
                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                    .body(timer_body(window).to_string())
            }
            CloudRequest::Reported => self.client.get(url),
        };
        let mut response = builder
            .header("apikey", self.key.clone())
            .send()
            .await
            .map_err(cloud_error)?;
        let status = response.status().as_u16();
        let path = request.path();
        match status {
            200..=299 => {}
            401 | 403 => {
                return Err(DriverError::Protocol(format!(
                    "{path}: the cloud did not accept the API key (HTTP {status})"
                )));
            }
            429 => {
                return Err(DriverError::Protocol(format!(
                    "{path}: the cloud's daily limit is reached (HTTP 429); try again tomorrow"
                )));
            }
            _ => {
                return Err(DriverError::Protocol(format!(
                    "{path}: the cloud answered HTTP {status}"
                )));
            }
        }
        if let Some(remaining) = response
            .headers()
            .get("ratelimit-remaining")
            .and_then(|value| value.to_str().ok())
        {
            tracing::debug!(path, remaining, "cloud calls left today");
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(cloud_error)? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(DriverError::Protocol(format!(
                    "{path}: the cloud answered too much"
                )));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// Never `Offline`: that would say the charger is not responding, while
/// it is the internet (or the cloud) that is not there.
fn cloud_error(error: reqwest::Error) -> DriverError {
    if error.is_timeout() {
        DriverError::Protocol("the cloud did not answer in time".to_owned())
    } else if error.is_connect() {
        DriverError::Protocol("cannot reach the cloud (no internet?)".to_owned())
    } else {
        // Without the URL: it carries the charger's ID.
        DriverError::Protocol(format!("cloud request failed: {}", error.without_url()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn window(start: &str, end: &str, days: &[Weekday]) -> ChargingWindow {
        ChargingWindow {
            start: start.parse().unwrap(),
            end: end.parse().unwrap(),
            days: days.to_vec(),
        }
    }

    #[test]
    fn timer_writes_have_the_documented_shape() {
        use Weekday::*;
        let night = window("22:00", "06:05", &[Wednesday, Monday, Tuesday]);
        assert_eq!(
            timer_body(&night),
            json!({ "timeStart": "22:00", "timeEnd": "06:05", "daysOfWeek": "123" })
        );
        assert_eq!(days_of_week(&Weekday::ALL), "1234567");
        assert_eq!(days_of_week(&[Sunday, Saturday, Sunday]), "67");
        assert_eq!(
            CloudRequest::Timer {
                slot: 0,
                window: night
            }
            .path(),
            "/device/timer"
        );
        assert_eq!(CloudRequest::Reported.path(), "/device/reported");
    }

    #[test]
    fn reads_the_timers_the_cloud_reports() {
        use Weekday::*;
        let body = json!({
            "programed": 1,
            "hourstart_00": 22, "minutestart_00": 0, "hourend_00": "6", "minuteend_00": 30,
            "daysofweek_00": "15",
            "hourstart_01": 0, "minutestart_01": 0, "hourend_01": 0, "minuteend_01": 0,
            "daysofweek_01": "8",
            "SomethingElse": true
        });
        assert_eq!(
            reported_timers(body.to_string().as_bytes()),
            Some(vec![
                ReportedTimer::Set(window("22:00", "06:30", &[Monday, Friday])),
                ReportedTimer::NotSet,
            ])
        );
        // Numbers for days, wrapped in `data` or a list.
        let wrapped = json!({ "data": {
            "hourstart_00": 1, "minutestart_00": 0, "hourend_00": 2, "minuteend_00": 0,
            "daysofweek_00": 67, "daysofweek_01": 8 } });
        assert_eq!(
            reported_timers(wrapped.to_string().as_bytes()),
            Some(vec![
                ReportedTimer::Set(window("01:00", "02:00", &[Saturday, Sunday])),
                ReportedTimer::NotSet,
            ])
        );
        let listed = json!([{ "daysofweek_00": "8", "daysofweek_01": "8" }]);
        assert_eq!(
            reported_timers(listed.to_string().as_bytes()),
            Some(vec![ReportedTimer::NotSet, ReportedTimer::NotSet])
        );
        // The object as text inside a JSON string.
        let text = json!(json!({ "daysofweek_00": "8", "daysofweek_01": "8" }).to_string());
        assert_eq!(
            reported_timers(text.to_string().as_bytes()),
            Some(vec![ReportedTimer::NotSet, ReportedTimer::NotSet])
        );
    }

    #[test]
    fn says_nothing_when_the_answer_does_not_say() {
        for body in [
            json!({}),
            json!({ "daysofweek_00": "1" }),
            json!({ "hourstart_00": 25, "minutestart_00": 0, "hourend_00": 2, "minuteend_00": 0,
                    "daysofweek_00": "1", "daysofweek_01": "8" }),
            json!({ "hourstart_00": 1, "minutestart_00": 0, "hourend_00": 2, "minuteend_00": 0,
                    "daysofweek_00": "19", "daysofweek_01": "8" }),
            json!("OK"),
        ] {
            assert_eq!(reported_timers(body.to_string().as_bytes()), None, "{body}");
        }
        assert_eq!(reported_timers(b"not json"), None);
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        let cloud = Cloud::new(CLOUD_BASE, "secret-key-123", Duration::from_secs(1)).unwrap();
        assert!(!format!("{cloud:?}").contains("secret"));
        assert!(Cloud::new(CLOUD_BASE, "bad\nkey", Duration::from_secs(1)).is_err());
    }
}
