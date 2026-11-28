//! `config.example.json` and `dashboard.example.json` must always parse.

// Integration-test crate: helpers outside #[test] fns may panic too.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use vesta_core::*;

const CONFIG_EXAMPLE: &str = include_str!("../../../config.example.json");
const DASHBOARD_EXAMPLE: &str = include_str!("../../../dashboard.example.json");

#[test]
fn config_example_parses_and_validates() {
    let config = HubConfig::from_json(CONFIG_EXAMPLE).unwrap();
    assert_eq!(config.schema_version, CONFIG_SCHEMA_VERSION);
    assert!(config.hub.bind_address.is_loopback());
    assert!(config.hub.go2rtc.api_bind_address.is_loopback());
    assert!(matches!(
        config.notifications,
        Some(NotificationsConfig::Ntfy { .. })
    ));
    assert_eq!(config.devices.len(), 5);
    let charger = config.device(&DeviceId::new("car_charger").unwrap()).unwrap();
    assert_eq!(charger.driver, "v2c");

    let strike = config
        .device(&DeviceId::new("front_door_strike").unwrap())
        .unwrap();
    assert_eq!(
        strike.endpoints[&EndpointId::main()].endpoint_type,
        Some(EndpointType::Door)
    );
    assert!(strike.config.is_object());

    // Round-trips without losing anything (the Hub rewrites it after UI edits).
    let original: Value = serde_json::from_str(CONFIG_EXAMPLE).unwrap();
    assert_eq!(serde_json::to_value(&config).unwrap(), original);
}

#[test]
fn debug_output_redacts_driver_config_and_topics() {
    let config = HubConfig::from_json(CONFIG_EXAMPLE).unwrap();
    let debug = format!("{config:?}");
    for secret in [
        "CHANGE_ME",
        "192.168.",
        "local_key",
        "password",
        "your-private-topic",
    ] {
        assert!(!debug.contains(secret), "Debug output leaks `{secret}`");
    }
    assert!(debug.contains("front_door_strike"));
}

#[test]
fn minimal_config_uses_local_defaults() {
    let config = HubConfig::from_json(r#"{ "schema_version": 1, "home_name": "Home" }"#).unwrap();
    assert_eq!(config.hub, HubSettings::default());
    assert!(config.devices.is_empty());
    assert!(config.notifications.is_none());
}

#[test]
fn config_validation() {
    let base = json!({
        "schema_version": 1,
        "home_name": "Home",
        "devices": [
            { "id": "lamp", "name": "Lamp", "driver": "example", "config": {} }
        ]
    });

    let mut wrong_version = base.clone();
    wrong_version["schema_version"] = json!(2);
    assert!(matches!(
        HubConfig::from_json(&wrong_version.to_string()),
        Err(ConfigError::UnsupportedSchemaVersion { found: 2, .. })
    ));

    let mut on_lan = base.clone();
    on_lan["hub"] = json!({ "bind_address": "0.0.0.0" });
    assert!(matches!(
        HubConfig::from_json(&on_lan.to_string()),
        Err(ConfigError::NotLoopback { .. })
    ));

    let mut go2rtc_on_lan = base.clone();
    go2rtc_on_lan["hub"] = json!({ "go2rtc": { "api_bind_address": "192.0.2.10" } });
    assert!(matches!(
        HubConfig::from_json(&go2rtc_on_lan.to_string()),
        Err(ConfigError::NotLoopback { .. })
    ));

    let mut duplicate = base.clone();
    duplicate["devices"] = json!([
        { "id": "lamp", "name": "Lamp", "driver": "example" },
        { "id": "lamp", "name": "Lamp 2", "driver": "example" }
    ]);
    assert!(matches!(
        HubConfig::from_json(&duplicate.to_string()),
        Err(ConfigError::DuplicateDevice(_))
    ));

    let mut bad_id = base;
    bad_id["devices"][0]["id"] = json!("Living Room Lamp");
    assert!(matches!(
        HubConfig::from_json(&bad_id.to_string()),
        Err(ConfigError::Json(_))
    ));
}

#[test]
fn endpoint_overrides_change_presentation_only() {
    let mut endpoint = Endpoint {
        id: EndpointId::main(),
        name: "Relay".to_owned(),
        endpoint_type: EndpointType::Switch,
        room: None,
        capabilities: Capabilities {
            on_off: Some(OnOffCapability::default()),
            power_meter: Some(PowerMeterCapability {
                features: PowerMeterFeatures {
                    role: MeterRole::Load,
                    bidirectional: false,
                },
                state: None,
            }),
            ..Capabilities::default()
        },
    };
    let capabilities_before = endpoint.capabilities.kinds();

    let overrides: EndpointOverride = serde_json::from_value(json!({
        "type": "light",
        "name": "Porch light",
        "room": "porch",
        "role": "solar"
    }))
    .unwrap();
    overrides.apply_to(&mut endpoint);

    assert_eq!(endpoint.endpoint_type, EndpointType::Light);
    assert_eq!(endpoint.name, "Porch light");
    assert_eq!(endpoint.room, Some(RoomId::new("porch").unwrap()));
    assert_eq!(
        endpoint
            .capabilities
            .power_meter
            .as_ref()
            .unwrap()
            .features
            .role,
        MeterRole::Solar
    );
    assert_eq!(endpoint.capabilities.kinds(), capabilities_before);
}

#[test]
fn dashboard_example_parses_and_round_trips() {
    let dashboard = Dashboard::from_json(DASHBOARD_EXAMPLE).unwrap();
    assert_eq!(dashboard.schema_version, DASHBOARD_SCHEMA_VERSION);
    assert_eq!(dashboard.theme.touch_scale, TouchScale::Large);
    assert_eq!(
        dashboard.kiosk.wake_on,
        vec![
            EventRef {
                target: "front_doorbell/main".parse().unwrap(),
                event: EventKind::DoorbellPressed,
            },
            EventRef {
                target: "front_doorbell/main".parse().unwrap(),
                event: EventKind::OccupancyDetected,
            },
        ]
    );
    let overlay = dashboard.doorbell_overlay.as_ref().unwrap();
    assert_eq!(
        overlay
            .open_target
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("front_door_strike/main")
    );

    let home = &dashboard.pages[0];
    assert_eq!(home.grid_columns, 4);
    assert!(home.widgets[0].target.is_none());
    assert_eq!(home.widgets[0].widget_type, "clock-weather");
    let fan = home
        .widgets
        .iter()
        .find(|w| w.widget_type == "fan-control")
        .unwrap();
    assert!(fan.props.is_empty());
    assert_eq!(
        fan.target.as_ref().map(ToString::to_string).as_deref(),
        Some("living_room_fan/fan")
    );

    let original: Value = serde_json::from_str(DASHBOARD_EXAMPLE).unwrap();
    assert_eq!(serde_json::to_value(&dashboard).unwrap(), original);
}

#[test]
fn dashboard_validation() {
    let base: Value = serde_json::from_str(DASHBOARD_EXAMPLE).unwrap();

    let mut wrong_version = base.clone();
    wrong_version["schema_version"] = json!(99);
    assert!(matches!(
        Dashboard::from_json(&wrong_version.to_string()),
        Err(ConfigError::UnsupportedSchemaVersion { found: 99, .. })
    ));

    let mut duplicate_page = base.clone();
    let page = duplicate_page["pages"][0].clone();
    duplicate_page["pages"].as_array_mut().unwrap().push(page);
    assert!(matches!(
        Dashboard::from_json(&duplicate_page.to_string()),
        Err(ConfigError::DuplicatePage(_))
    ));

    let mut too_wide = base.clone();
    too_wide["pages"][0]["widgets"][0]["col_span"] = json!(5);
    assert!(matches!(
        Dashboard::from_json(&too_wide.to_string()),
        Err(ConfigError::InvalidLayout { .. })
    ));

    let mut empty_span = base.clone();
    empty_span["pages"][0]["widgets"][0]["row_span"] = json!(0);
    assert!(matches!(
        Dashboard::from_json(&empty_span.to_string()),
        Err(ConfigError::InvalidLayout { .. })
    ));

    let mut bad_event = base;
    bad_event["kiosk"]["wake_on"][0]["event"] = json!("doorbell.exploded");
    assert!(matches!(
        Dashboard::from_json(&bad_event.to_string()),
        Err(ConfigError::Json(_))
    ));
}

/// What the editor can send: everything the UI might get wrong is refused
/// with a reason, nothing panics.
#[test]
fn dashboard_validation_rejects_what_the_editor_must_never_store() {
    let base: Value = serde_json::from_str(DASHBOARD_EXAMPLE).unwrap();
    let invalid = |edit: &dyn Fn(&mut Value)| {
        let mut dashboard = base.clone();
        edit(&mut dashboard);
        Dashboard::from_json(&dashboard.to_string()).unwrap_err()
    };
    let layout = |edit: &dyn Fn(&mut Value)| {
        let error = invalid(edit);
        assert!(
            matches!(error, ConfigError::InvalidLayout { ref page, .. } if page == "home"),
            "{error}"
        );
    };

    // Page ids are stable snake_case identifiers.
    for bad_id in ["", "Home", "my page", "home-2", "_home"] {
        let error = invalid(&|d| d["pages"][0]["id"] = json!(bad_id));
        assert!(
            matches!(error, ConfigError::InvalidPageId { .. }),
            "{bad_id:?}: {error}"
        );
    }
    let error = invalid(&|d| {
        let page = d["pages"][0].clone();
        let pages = d["pages"].as_array_mut().unwrap();
        pages.clear();
        pages.extend((0..=MAX_PAGES).map(|i| {
            let mut page = page.clone();
            page["id"] = json!(format!("page_{i}"));
            page
        }));
    });
    assert!(
        matches!(error, ConfigError::TooManyPages { found: 33, max: 32 }),
        "{error}"
    );

    layout(&|d| d["pages"][0]["title"] = json!("   "));
    layout(&|d| d["pages"][0]["title"] = json!("x".repeat(MAX_TITLE_CHARS + 1)));
    layout(&|d| d["pages"][0]["grid_columns"] = json!(0));
    layout(&|d| d["pages"][0]["grid_columns"] = json!(MAX_GRID_COLUMNS + 1));
    layout(&|d| d["pages"][0]["widgets"][0]["row_span"] = json!(MAX_ROW_SPAN + 1));
    layout(&|d| d["pages"][0]["widgets"][0]["col_span"] = json!(0));
    for bad_type in ["", "Light-Control", "light control", "light_control"] {
        layout(&|d| d["pages"][0]["widgets"][0]["type"] = json!(bad_type));
    }
    layout(&|d| d["pages"][0]["widgets"][0]["type"] = json!("x".repeat(MAX_WIDGET_TYPE_LEN + 1)));
    layout(&|d| {
        let widget = d["pages"][0]["widgets"][0].clone();
        d["pages"][0]["widgets"] = Value::Array(vec![widget; MAX_WIDGETS_PER_PAGE + 1]);
    });

    // Targets must be `<device_id>/<endpoint_id>`; serde refuses anything else.
    for bad_target in ["living_room_light", "Living Room/Light", "a/b/c", ""] {
        let error = invalid(&|d| d["pages"][0]["widgets"][1]["target"] = json!(bad_target));
        assert!(
            matches!(error, ConfigError::Json(_)),
            "{bad_target:?}: {error}"
        );
    }
}

#[test]
fn dashboard_validation_accepts_what_the_editor_makes() {
    let mut dashboard = Dashboard::from_json(DASHBOARD_EXAMPLE).unwrap();
    // A target that does not exist (yet) is fine: the device may be offline.
    dashboard.pages[0].widgets[1].target = Some("attic_light/main".parse().unwrap());
    // Unknown widget types are fine: a newer screen may know them.
    dashboard.pages[0].widgets[0].widget_type = "thermostat-control-2".to_owned();
    // A new, empty page with a generated id.
    dashboard.pages.push(Page {
        id: "page_2".to_owned(),
        title: "Página nueva".to_owned(),
        icon: Some("bedroom".to_owned()),
        grid_columns: MAX_GRID_COLUMNS,
        widgets: vec![],
    });
    dashboard.validate().unwrap();

    // No pages at all is valid too (the UI shows "nothing to show yet").
    dashboard.pages.clear();
    dashboard.validate().unwrap();
}

#[test]
fn dashboard_lists_every_target_it_uses() {
    let dashboard = Dashboard::from_json(DASHBOARD_EXAMPLE).unwrap();
    let targets: Vec<String> = dashboard.targets().map(ToString::to_string).collect();
    assert_eq!(
        targets,
        [
            "front_door_strike/main",
            "front_doorbell/main",
            "living_room_light/main",
            "living_room_fan/fan",
            "living_room_fan/light",
            "front_doorbell/main",
            "front_doorbell/main",
            "front_doorbell/main",
            "front_doorbell/main",
            "front_door_strike/main",
        ]
    );
}
