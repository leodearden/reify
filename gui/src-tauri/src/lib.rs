#[cfg(feature = "gui")]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_tauri_config_valid() {
        let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json");
        let config_str = std::fs::read_to_string(config_path)
            .expect("tauri.conf.json should exist");
        let config: serde_json::Value = serde_json::from_str(&config_str)
            .expect("tauri.conf.json should be valid JSON");

        assert_eq!(config["productName"], "Reify");
        assert_eq!(config["version"], "0.1.0");
        assert_eq!(config["identifier"], "dev.reify.app");
        assert_eq!(config["build"]["devUrl"], "http://localhost:1420");
        assert_eq!(config["build"]["frontendDist"], "../dist");

        let window = &config["app"]["windows"][0];
        assert_eq!(window["title"], "Reify");
        assert_eq!(window["width"], 1400);
        assert_eq!(window["height"], 900);
        assert_eq!(window["resizable"], true);

        assert_eq!(config["bundle"]["active"], false);
    }

    #[test]
    fn test_default_capability_valid() {
        let cap_path = concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/default.json");
        let cap_str = std::fs::read_to_string(cap_path)
            .expect("capabilities/default.json should exist");
        let cap: serde_json::Value = serde_json::from_str(&cap_str)
            .expect("capabilities/default.json should be valid JSON");

        assert_eq!(cap["identifier"], "default");
        assert_eq!(cap["description"], "Default capabilities for Reify");

        let windows = cap["windows"].as_array().expect("windows should be array");
        assert!(windows.iter().any(|w| w == "main"));

        let permissions = cap["permissions"].as_array().expect("permissions should be array");
        assert!(permissions.iter().any(|p| p == "core:default"));
        assert!(permissions.iter().any(|p| p == "event:default"));
        assert!(permissions.iter().any(|p| p == "window:default"));
    }
}
