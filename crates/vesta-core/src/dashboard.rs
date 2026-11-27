//! `dashboard.json`: the declarative UI (see `dashboard.example.json`).
//!
//! Users never edit this file by hand; the UI writes it. The frontend renders
//! pages and widgets through `web/src/widgets/registry.ts`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::ConfigError;
use crate::event::EventKind;
use crate::id::Target;

/// `schema_version` understood by this build for `dashboard.json`.
pub const DASHBOARD_SCHEMA_VERSION: u32 = 1;

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

    /// Checks the schema version, unique page IDs and that every widget fits
    /// its page's grid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != DASHBOARD_SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedSchemaVersion {
                found: self.schema_version,
                supported: DASHBOARD_SCHEMA_VERSION,
            });
        }
        let mut seen = BTreeSet::new();
        for page in &self.pages {
            if !seen.insert(page.id.as_str()) {
                return Err(ConfigError::DuplicatePage(page.id.clone()));
            }
            page.validate()?;
        }
        Ok(())
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
        if self.grid_columns == 0 {
            return Err(invalid("grid_columns must be at least 1".to_owned()));
        }
        for (index, widget) in self.widgets.iter().enumerate() {
            if widget.col_span == 0 || widget.row_span == 0 {
                return Err(invalid(format!("widget {index} has an empty span")));
            }
            if widget.col_span > self.grid_columns {
                return Err(invalid(format!(
                    "widget {index} spans {} columns but the grid has {}",
                    widget.col_span, self.grid_columns
                )));
            }
        }
        Ok(())
    }
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
