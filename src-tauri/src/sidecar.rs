//! サイドカー（写真のフォルダ直下の `.photo-curator/catalog.json`）の読み書きと、
//! 端末が覚える状態（設計 03 章）。
//!
//! **判断（4 通り）はここに持たない。** `sidecarDecide` は core（画面側の wasm）だけが呼ぶ。
//! ここは「どこへ・どう安全に書くか」と、端末が覚える 3 つの値の保存だけ。
//! 書けるのは `.photo-curator/` の中だけで、写真の原本には書かない。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{fs, path::{Path, PathBuf}};
use uuid::Uuid;

pub const SIDECAR_DIR: &str = ".photo-curator";
pub const SIDECAR_FILE: &str = "catalog.json";

const SETTING_DEVICE_ID: &str = "device_id";

/// 端末が覚える、最後に読んだ／書いたサイドカーの印と、判断が変わったか。
#[derive(Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SidecarState {
    pub seen_at: i64,
    pub seen_by: String,
    pub local_changed: bool,
}

#[derive(Serialize, Debug, PartialEq, Eq, Clone)]
pub struct DeviceIdentity {
    pub id: String,
    pub name: String,
}

pub fn ensure_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sidecar_state (
           project_id TEXT PRIMARY KEY,
           seen_at INTEGER NOT NULL DEFAULT 0,
           seen_by TEXT NOT NULL DEFAULT '',
           local_changed INTEGER NOT NULL DEFAULT 0
         );",
    )
    .map_err(|error| error.to_string())
}

pub fn load_state(conn: &Connection, project_id: &str) -> Result<SidecarState, String> {
    let row = conn
        .query_row(
            "SELECT seen_at, seen_by, local_changed FROM sidecar_state WHERE project_id=?1",
            params![project_id],
            |row| {
                Ok(SidecarState {
                    seen_at: row.get(0)?,
                    seen_by: row.get(1)?,
                    local_changed: row.get::<_, i64>(2)? != 0,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(row.unwrap_or_default())
}

pub fn save_state(conn: &Connection, project_id: &str, state: &SidecarState) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sidecar_state (project_id, seen_at, seen_by, local_changed) VALUES (?1,?2,?3,?4)
         ON CONFLICT(project_id) DO UPDATE SET
           seen_at=excluded.seen_at, seen_by=excluded.seen_by, local_changed=excluded.local_changed",
        params![project_id, state.seen_at, state.seen_by, state.local_changed as i64],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

/// この端末の id と名前。id は初回に作って `app_settings` に残す。名前はホスト名。
pub fn device_identity(conn: &Connection) -> Result<DeviceIdentity, String> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key=?1",
            params![SETTING_DEVICE_ID],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    let id = match existing {
        Some(id) if !id.trim().is_empty() => id,
        _ => {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![SETTING_DEVICE_ID, id],
            )
            .map_err(|error| error.to_string())?;
            id
        }
    };
    Ok(DeviceIdentity { id, name: host_name() })
}

fn host_name() -> String {
    for key in ["COMPUTERNAME", "HOSTNAME"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return value.to_string();
            }
        }
    }
    if let Ok(text) = fs::read_to_string("/etc/hostname") {
        let text = text.trim();
        if !text.is_empty() {
            return text.to_string();
        }
    }
    "PC".to_string()
}

fn sidecar_dir(folder: &Path) -> PathBuf {
    folder.join(SIDECAR_DIR)
}

/// `catalog.json` か `catalog.<英数字とハイフン>.json` だけを許す（別の場所・別の名前へ書かせない）。
pub fn valid_file_name(name: &str) -> bool {
    if name == SIDECAR_FILE {
        return true;
    }
    let Some(middle) = name
        .strip_prefix("catalog.")
        .and_then(|rest| rest.strip_suffix(".json"))
    else {
        return false;
    };
    !middle.is_empty()
        && middle.len() <= 64
        && middle.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// 書けるか。**実際に書いてみて**決める（ディレクトリの読み取り専用の属性は、Windows では意味が違う）。
///
/// - `.photo-curator/` を作れて、その中に一時ファイル（`.probe-<乱数>`）を作って消せれば `readwrite`
/// - 読めるが書けなければ `readonly`（読んで取り込むだけ。ダイアログは出さない）
/// - フォルダが無い・読めなければ `none`
pub fn support(folder: &Path) -> &'static str {
    if !fs::metadata(folder).map(|meta| meta.is_dir()).unwrap_or(false) {
        return "none";
    }
    let dir = sidecar_dir(folder);
    let writable = fs::create_dir_all(&dir).is_ok() && {
        let probe = dir.join(format!(".probe-{}", Uuid::new_v4().simple()));
        let created = fs::write(&probe, b"").is_ok();
        // 作れたなら、消せるところまで確かめる（消せない共有には、一時ファイルが残り続ける）。
        created && fs::remove_file(&probe).is_ok()
    };
    if writable {
        return "readwrite";
    }
    // 書けない。読めるかどうかで readonly と none を分ける（`.photo-curator/` があればその中、無ければ写真のフォルダ）。
    let readable = fs::read_dir(&dir).is_ok() || fs::read_dir(folder).is_ok();
    if readable { "readonly" } else { "none" }
}

/// `catalog.json` の中身。無ければ None。
pub fn read(folder: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(sidecar_dir(folder).join(SIDECAR_FILE)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("サイドカーを読めませんでした: {error}")),
    }
}

/// 原子的に書く。一時ファイルに書いて rename。途中で落ちても壊れた `catalog.json` は残らない。
pub fn write(folder: &Path, file_name: &str, json: &str) -> Result<(), String> {
    if !valid_file_name(file_name) {
        return Err("サイドカーのファイル名が正しくありません。".to_string());
    }
    let dir = sidecar_dir(folder);
    fs::create_dir_all(&dir)
        .map_err(|error| format!("サイドカーのフォルダを作れませんでした: {error}"))?;
    let temp = dir.join(format!(".{file_name}.{}.tmp", Uuid::new_v4().simple()));
    let result = fs::write(&temp, json.as_bytes())
        .and_then(|()| fs::rename(&temp, dir.join(file_name)));
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(format!("サイドカーを書けませんでした: {error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("photo-curator-sidecar-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch("CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .expect("settings");
        ensure_tables(&conn).expect("tables");
        conn
    }

    #[test]
    fn state_defaults_to_never_seen_and_round_trips() {
        let conn = memory();
        assert_eq!(load_state(&conn, "p1").unwrap(), SidecarState::default());
        let state = SidecarState { seen_at: 1234, seen_by: "dev-a".into(), local_changed: true };
        save_state(&conn, "p1", &state).unwrap();
        assert_eq!(load_state(&conn, "p1").unwrap(), state);
        let cleared = SidecarState { local_changed: false, ..state.clone() };
        save_state(&conn, "p1", &cleared).unwrap();
        assert_eq!(load_state(&conn, "p1").unwrap(), cleared);
        // 別のプロジェクトには影響しない。
        assert_eq!(load_state(&conn, "p2").unwrap(), SidecarState::default());
    }

    #[test]
    fn ensure_tables_is_idempotent() {
        let conn = memory();
        ensure_tables(&conn).unwrap();
        ensure_tables(&conn).unwrap();
    }

    #[test]
    fn device_id_is_created_once_and_kept() {
        let conn = memory();
        let first = device_identity(&conn).unwrap();
        let second = device_identity(&conn).unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.id.len(), 36);
        assert!(!first.name.is_empty());
    }

    #[test]
    fn writes_catalog_atomically_under_dot_photo_curator() {
        let folder = temp_dir("write");
        assert_eq!(read(&folder).unwrap(), None);
        write(&folder, SIDECAR_FILE, "{\"a\":1}").unwrap();
        write(&folder, SIDECAR_FILE, "{\"a\":2}").unwrap();
        assert_eq!(read(&folder).unwrap().as_deref(), Some("{\"a\":2}"));
        // 写真のフォルダ直下に増えたのは `.photo-curator/` だけ。中に一時ファイルは残らない。
        let top: Vec<_> = fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(top, vec![std::ffi::OsString::from(SIDECAR_DIR)]);
        let inner: Vec<_> = fs::read_dir(folder.join(SIDECAR_DIR)).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(inner, vec![std::ffi::OsString::from(SIDECAR_FILE)]);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn aside_file_is_written_next_to_catalog() {
        let folder = temp_dir("aside");
        write(&folder, SIDECAR_FILE, "mine").unwrap();
        write(&folder, "catalog.0123456789ab.json", "theirs").unwrap();
        assert_eq!(read(&folder).unwrap().as_deref(), Some("mine"));
        assert_eq!(
            fs::read_to_string(folder.join(SIDECAR_DIR).join("catalog.0123456789ab.json")).unwrap(),
            "theirs"
        );
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn rejects_file_names_that_could_escape_the_folder() {
        for bad in ["../catalog.json", "catalog..json", "catalog.a/b.json", "x.json", "catalog.json.tmp", "catalog..json", ""] {
            assert!(!valid_file_name(bad), "{bad}");
        }
        assert!(valid_file_name("catalog.json"));
        assert!(valid_file_name("catalog.ab-12.json"));
        let folder = temp_dir("bad");
        assert!(write(&folder, "../evil.json", "x").is_err());
        assert!(!folder.join(SIDECAR_DIR).exists());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn support_is_readonly_when_the_sidecar_folder_cannot_be_made() {
        // `.photo-curator` が同名のファイルだと、フォルダを作れない（書けない共有と同じ結果）。root でも通る。
        let folder = temp_dir("support-blocked");
        fs::write(folder.join(SIDECAR_DIR), b"not a folder").unwrap();
        assert_eq!(support(&folder), "readonly");
        let _ = fs::remove_dir_all(&folder);
    }

    /// root は 0o555 のディレクトリにも書けてしまい、readonly にならないため、root では走らせない。
    /// 一般ユーザーで `cargo test -- --ignored` を実行すると確かめられる。
    #[test]
    #[ignore = "root ではパーミッションを無視して書けてしまう（一般ユーザーで --ignored を付けて実行する）"]
    #[cfg(unix)]
    fn support_is_readonly_for_a_directory_without_write_permission() {
        use std::os::unix::fs::PermissionsExt;
        let folder = temp_dir("support-ro");
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let result = support(&folder);
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        let _ = fs::remove_dir_all(&folder);
        assert_eq!(result, "readonly");
    }

    #[test]
    fn support_leaves_no_probe_file_behind() {
        let folder = temp_dir("support-probe");
        assert_eq!(support(&folder), "readwrite");
        let inner: Vec<_> = fs::read_dir(folder.join(SIDECAR_DIR)).unwrap().collect();
        assert!(inner.is_empty(), "一時ファイルは消えている");
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn support_is_none_for_a_missing_folder() {
        let folder = temp_dir("support");
        assert_eq!(support(&folder), "readwrite");
        assert_eq!(support(&folder.join("nope")), "none");
        let _ = fs::remove_dir_all(&folder);
    }
}
