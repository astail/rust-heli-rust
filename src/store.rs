//! localStorage に JSON で設定を残す。元の JS 版と同じオリジンで動くので、同じ接頭辞 (rhtr_)・
//! 同じキー・同じ形で保存し、設定とベストタイムを JS 版と共有する。

use serde::{Serialize, de::DeserializeOwned};
use web_sys::Storage;

const PREFIX: &str = "rhtr_";
/// JS 版と分けていた頃の接頭辞
const OLD_PREFIX: &str = "rhrs_";

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

/// JS 版と分けて保存していた頃 (rhrs_) の値を、JS 版と共有するキーへ移して消す。起動時に呼ぶ。
/// 消すのは、移したあとに「すべてリセット」しても古い設定が戻ってこないようにするため
pub fn move_old_prefix() {
    let Some(s) = storage() else { return };
    let n = s.length().unwrap_or(0);
    let old_keys: Vec<String> = (0..n)
        .filter_map(|i| s.key(i).ok().flatten())
        .filter(|k| k.starts_with(OLD_PREFIX))
        .collect();
    for old_key in old_keys {
        let key = &old_key[OLD_PREFIX.len()..];
        let new_key = format!("{PREFIX}{key}");
        if let Ok(Some(old)) = s.get_item(&old_key) {
            let cur = s.get_item(&new_key).ok().flatten();
            if keep_old(key, &old, cur.as_deref()) {
                let _ = s.set_item(&new_key, &old);
            }
        }
        let _ = s.remove_item(&old_key);
    }
}

/// 古い値で上書きするか。JS 版に値がなければ移す。両方にあるときは、設定は JS 版のほうを残し、
/// ベストタイムは速いほうを残す
fn keep_old(key: &str, old: &str, cur: Option<&str>) -> bool {
    let Some(cur) = cur else { return true };
    if !key.starts_with("best_") {
        return false;
    }
    match (old.parse::<f64>(), cur.parse::<f64>()) {
        (Ok(o), Ok(c)) => o < c,
        // JS 版のベストタイムが壊れていれば古いほうを使う
        (Ok(_), Err(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::keep_old;

    #[test]
    fn moves_old_value_when_js_version_has_none() {
        assert!(keep_old("sens2", "0.3", None));
        assert!(keep_old("best_rings", "61.5", None));
    }

    #[test]
    fn settings_keep_the_js_version() {
        assert!(!keep_old("sens2", "0.3", Some("0.5")));
        assert!(!keep_old("camcfg", "{}", Some("{\"6\":{}}")));
    }

    #[test]
    fn best_times_keep_the_faster_one() {
        assert!(keep_old("best_rings", "58.25", Some("61.5")));
        assert!(!keep_old("best_rings", "63", Some("61.5")));
        assert!(!keep_old("best_hover", "20.1", Some("20.1")));
        assert!(keep_old("best_landing", "90", Some("null")));
        assert!(!keep_old("best_landing", "null", Some("90")));
    }
}
