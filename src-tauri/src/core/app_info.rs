use serde::Serialize;

/// 프론트엔드 상태 표시줄에 노출하는 앱 정보.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
}

impl AppInfo {
    pub fn current() -> Self {
        Self {
            name: "Rivet",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_uses_display_name_and_cargo_version() {
        let info = AppInfo::current();
        assert_eq!(info.name, "Rivet");
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn serializes_name_and_version() {
        let json = serde_json::to_value(AppInfo::current()).unwrap();
        assert_eq!(json["name"], "Rivet");
        assert!(json["version"].is_string());
    }
}
