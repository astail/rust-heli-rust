//! localStorage に JSON で設定を残す。元の JS 版 (rhtr_) と同じオリジンで動くので接頭辞を分けてある。

use serde::{Serialize, de::DeserializeOwned};
use web_sys::Storage;

const PREFIX: &str = "rhrs_";

fn storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn get<T: DeserializeOwned>(key: &str) -> Option<T> {
    let raw = storage()?.get_item(&format!("{PREFIX}{key}")).ok()??;
    serde_json::from_str(&raw).ok()
}

pub fn get_or<T: DeserializeOwned>(key: &str, default: T) -> T {
    get(key).unwrap_or(default)
}

pub fn set<T: Serialize + ?Sized>(key: &str, v: &T) {
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(v)) {
        let _ = s.set_item(&format!("{PREFIX}{key}"), &json);
    }
}

/// ベストタイム以外の設定 (カメラ・HUD・感度・音など) をすべて消す
pub fn reset_all() {
    let Some(s) = storage() else { return };
    let n = s.length().unwrap_or(0);
    let keys: Vec<String> = (0..n).filter_map(|i| s.key(i).ok().flatten()).collect();
    for k in keys {
        if k.starts_with(PREFIX) && !k.starts_with(&format!("{PREFIX}best_")) {
            let _ = s.remove_item(&k);
        }
    }
}
