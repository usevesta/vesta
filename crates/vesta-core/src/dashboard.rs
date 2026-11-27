//! `dashboard.json`: the declarative UI (see `dashboard.example.json`).
//!
//! Users never edit this file by hand; the UI writes it. The frontend renders
//! pages and widgets through `web/src/widgets/registry.ts`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::ConfigError;
use crate::event::EventKind;
use crate::id::{Target, validate_id};

/// `schema_version` understood by this build for `dashboard.json`.
pub const DASHBOARD_SCHEMA_VERSION: u32 = 1;

/// Limits enforced by [`Dashboard::validate`]. Generous for a wall panel,
/// tight enough that a broken client cannot store something unusable.
pub const MAX_PAGES: usize = 32;
pub const MAX_WIDGETS_PER_PAGE: usize = 64;
pub const MAX_GRID_COLUMNS: u8 = 12;
pub const MAX_ROW_SPAN: u8 = 6;
/// Page titles, in characters.
pub const MAX_TITLE_CHARS: usize = 64;
/// Widget `type` strings (registry keys such as `light-control`), in bytes.
pub const MAX_WIDGET_TYPE_LEN: usize = 64;

/// A JSON object with arbitrary values (widget `props`).
pub type JsonObject = serde_json::Map<String, serde_json::Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Dashboard {
    pub schema_version: u32,
    pub theme: Theme,
    pub kiosk: Kiosk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doorbell_overlay: Option<DoorbellOverlay>,
    pub pages: Vec<Page>,
}

impl Dashboard {
    /// Parses and validates a `dashboard.json` document.
    pub fn from_json(json: &str) -> Result<Self, ConfigError> {
        let dashboard: Self = serde_json::from_str(json)?;
        dashboard.validate()?;
        Ok(dashboard)
    }

    /// Checks invariants serde cannot express: the schema version, unique,
    /// well-formed page IDs (lowercase `snake_case`), non-empty titles, sane
    /// grids and that every widget fits its page. Targets are well-formed by
    /// construction ([`Target`] only parses `<device_id>/<endpoint_id>`);
    /// whether they exist is not checked here (a device may be offline or
    /// gone, and the UI shows it as unavailable).
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != DASHBOARD_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: DASHBOARD_SCHEMA_VERSION,
            });
        }
        if self.pages.len() > MAX_PAGES {
            return Err(ConfigError::TooManyPages {
                found: self.pages.len(),
                max: MAX_PAGES,
            });
        }
        let mut seen = BTreeSet::new();
        for page in &self.pages {
            validate_id(&page.id).map_err(|source| ConfigError::InvalidPageId {
                page: page.id.clone(),
                source,
            })?;
            if !seen.insert(page.id.as_str()) {
                return Err(ConfigError::DuplicatePage(page.id.clone()));
            }
            page.validate()?;
        }
        Ok(())
    }

    /// Every endpoint the dashboard refers to: widget targets, wake events
    /// and the doorbell overlay.
    pub fn targets(&self) -> impl Iterator<Item = &Target> {
        let widgets = self
            .pages
            .iter()
            .flat_map(|page| page.widgets.iter().filter_map(|w| w.target.as_ref()));
        let wake = self.kiosk.wake_on.iter().map(|event| &event.target);
        let overlay = self.doorbell_overlay.iter().flat_map(|overlay| {
            [
                Some(&overlay.doorbell_target),
                overlay.camera_target.as_ref(),
                overlay.open_target.as_ref(),
            ]
            .into_iter()
            .flatten()
        });
        widgets.chain(wake).chain(overlay)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Theme {
    /// Name of a CSS-variable theme preset, e.g. `glass-dark`.
    pub preset: String,
    /// Follow the system light/dark setting.
    pub auto_dark_mode: bool,
    pub touch_scale: TouchScale,
}

/// Size of touch targets and text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TouchScale {
    Normal,
    Large,
    ExtraLarge,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Kiosk {
    /// Idle time before the screensaver starts, in seconds.
    pub screensaver_timeout_sec: u32,
    /// Events that wake the panel.
    #[serde(default)]
    pub wake_on: Vec<EventRef>,
}

/// A reference to an event on an endpoint, e.g. to wake the panel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct EventRef {
    pub target: Target,
    pub event: EventKind,
}

/// Full-screen overlay shown when the doorbell rings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct DoorbellOverlay {
    pub doorbell_target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_target: Option<Target>,
    /// Endpoint the overlay's "open" button controls (a sensitive command).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_target: Option<Target>,
    pub auto_dismiss_sec: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(optional_fields)]
pub struct Page {
    pub id: String,
    pub title: String,
    /// Semantic icon name from the frontend's icon registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub grid_columns: u8,
    pub widgets: Vec<Widget>,
}

impl Page {
    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |reason: String| ConfigError::InvalidLayout {
            page: self.id.clone(),
            reason,
        };
        let title = self.title.trim();
        if title.is_empty() {
            return Err(invalid("title is empty".to_owned()));
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(invalid(format!(
                "title is longer than {MAX_TITLE_CHARS} characters"
            )));
        }
        if self.grid_columns == 0 || self.grid_columns > MAX_GRID_COLUMNS {
            return Err(invalid(format!(
                "grid_columns must be between 1 and {MAX_GRID_COLUMNS}"
            )));
        }
        if self.widgets.len() > MAX_WIDGETS_PER_PAGE {
            return Err(invalid(format!(
                "{} widgets, at most {MAX_WIDGETS_PER_PAGE} are allowed",
                self.widgets.len()
            )));
        }
        for (index, widget) in self.widgets.iter().enumerate() {
            if !is_widget_type(&widget.widget_type) {
                return Err(invalid(format!(
                    "widget {index} has an invalid type (lowercase letters, digits and `-`)"
                )));
            }
            if widget.col_span == 0 || widget.row_span == 0 {
                return Err(invalid(format!("widget {index} has an empty span")));
            }
            if widget.col_span > self.grid_columns {
                return Err(invalid(format!(
                    "widget {index} spans {} columns but the grid has {}",
                    widget.col_span, self.grid_columns
                )));
            }
            if widget.row_span > MAX_ROW_SPAN {
                return Err(invalid(format!(
                    "widget {index} spans {} rows, at most {MAX_ROW_SPAN} are allowed",
                    widget.row_span
                )));
            }
        }
        Ok(())
    }
}

/// Registry keys are kebab-case: `light-control`, `clock-weather`. Types
/// this build does not know are allowed (a newer frontend may add them).
fn is_widget_type(widget_type: &str) -> bool {
    !widget_type.is_empty()
        && widget_type.len() <= MAX_WIDGET_TYPE_LEN
        && widget_type
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Widget {
    /// Key in the frontend widget registry, e.g. `light-control`.
    #[serde(rename = "type")]
    pub widget_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub target: Option<Target>,
    pub col_span: u8,
    pub row_span: u8,
    /// Widget-specific settings, interpreted by the widget.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub props: JsonObject,
}
