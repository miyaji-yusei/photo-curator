//! サイドカー（写真のフォルダ直下の `.photo-curator/catalog.json`）の読み書きと、
//! 端末が覚える状態（設計 03 章）。
//!
//! **判断はここに持たない。** 開き方の判断（`sidecarPlan`）は core（画面側の wasm）だけが呼ぶ。
//! ここは「どこへ・どう安全に書くか」（楽観ロックの書き込み。U34）と、端末が覚える値の保存だけ。
//! 書けるのは `.photo-curator/` の中だけで、写真の原本には書かない。

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

pub const SIDECAR_DIR: &str = ".photo-curator";
pub const SIDECAR_FILE: &str = "catalog.json";
/// 書き込みの間だけ置く排他のロック（設計書 §4.4。書き終われば消える）。
pub const LOCK_FILE: &str = "catalog.lock";
/// これより古いロックは、書いた端末が途中で止まったものと見なして壊す。
pub const LOCK_TTL: Duration = Duration::from_secs(60);

const SETTING_DEVICE_ID: &str = "device_id";

/// 端末が覚える、最後に読んだ／書いたサイドカーの控え。
///
/// U34 で `seen_token`・`seen_key`・`seen_epoch`・`local_epoch`・`detached` を足した。
/// `seen_token` が None の行は古い形のまま（画面側が `seen_at`/`seen_by`/`local_changed`
/// から `legacy:` の控えを作る）。古い 3 つも書き続ける（古い版のアプリに戻しても読める）。
#[derive(Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SidecarState {
    pub seen_at: i64,
    pub seen_by: String,
    pub local_changed: bool,
    /// 最後に読んだ／書いた版の見分け（core の `sidecar_token`）。None は古い形。
    #[serde(default)]
    pub seen_token: Option<String>,
    /// その版を読んだ／書いたときの、選別状況の比較キー。空は「分からない＝変更あり」。
    #[serde(default)]
    pub seen_key: String,
    /// その版のやり直しの世代。
    #[serde(default)]
    pub seen_epoch: Option<String>,
    /// この端末の選別状況のやり直しの世代（やり直すたびに新しい乱数）。
    #[serde(default)]
    pub local_epoch: Option<String>,
    /// 食い違いで「この端末の状況を残す」を選んだあと（自動では書かない）。
    #[serde(default)]
    pub detached: bool,
}

/// 楽観ロックの書き込みの結果。
#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum CheckedWrite {
    /// 書けて、読み戻しても同じだった。
    Written,
    /// 見た版と違っていた（書いていない）か、書いた直後に別の端末に置き換えられた。判定し直す。
    Changed,
    /// ほかの端末が書いている（ロックが新しい）。次の契機に回す。
    Locked,
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
    .map_err(|error| error.to_string())?;
    // U34 の列。古い表には足す（値の無い行は古い形のまま読む）。
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(sidecar_state)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|error| error.to_string())?;
    for (name, definition) in [
        ("seen_token", "TEXT"),
        ("seen_key", "TEXT NOT NULL DEFAULT ''"),
        ("seen_epoch", "TEXT"),
        ("local_epoch", "TEXT"),
        ("detached", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !columns.iter().any(|column| column == name) {
            conn.execute_batch(&format!("ALTER TABLE sidecar_state ADD COLUMN {name} {definition};"))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn load_state(conn: &Connection, project_id: &str) -> Result<SidecarState, String> {
    let row = conn
        .query_row(
            "SELECT seen_at, seen_by, local_changed, seen_token, seen_key, seen_epoch, local_epoch, detached
             FROM sidecar_state WHERE project_id=?1",
            params![project_id],
            |row| {
                Ok(SidecarState {
                    seen_at: row.get(0)?,
                    seen_by: row.get(1)?,
                    local_changed: row.get::<_, i64>(2)? != 0,
                    seen_token: row.get(3)?,
                    seen_key: row.get(4)?,
                    seen_epoch: row.get(5)?,
                    local_epoch: row.get(6)?,
                    detached: row.get::<_, i64>(7)? != 0,
                })
            },
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(row.unwrap_or_default())
}

pub fn save_state(conn: &Connection, project_id: &str, state: &SidecarState) -> Result<(), String> {
    conn.execute(
        "INSERT INTO sidecar_state
           (project_id, seen_at, seen_by, local_changed, seen_token, seen_key, seen_epoch, local_epoch, detached)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(project_id) DO UPDATE SET
           seen_at=excluded.seen_at, seen_by=excluded.seen_by, local_changed=excluded.local_changed,
           seen_token=excluded.seen_token, seen_key=excluded.seen_key, seen_epoch=excluded.seen_epoch,
           local_epoch=excluded.local_epoch, detached=excluded.detached",
        params![
            project_id,
            state.seen_at,
            state.seen_by,
            state.local_changed as i64,
            state.seen_token,
            state.seen_key,
            state.seen_epoch,
            state.local_epoch,
            state.detached as i64
        ],
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

/// 取ったロック。手放すとき（drop）に、**まだ自分が書いた中身のときだけ**ファイルを消す（U52 D5。
/// ほかの端末が古いと見なして取り直していたら、そのロックは残す。Android の U44 と同じ）。
struct LockGuard {
    path: PathBuf,
    body: Vec<u8>,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if fs::read(&self.path).is_ok_and(|current| current == self.body) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

fn try_create_lock(path: &Path, holder: &str) -> std::io::Result<LockGuard> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    // 形は Android と同じ `{"holder","at"}`（at は書いた端末の時計の ms）。nonce は放すときの見分け。
    let body = format!(
        "{{\"holder\":{},\"at\":{},\"nonce\":\"{}\"}}",
        serde_json::Value::String(holder.to_string()),
        now_millis(),
        Uuid::new_v4().simple()
    )
    .into_bytes();
    let _ = file.write_all(&body);
    Ok(LockGuard { path: path.to_path_buf(), body })
}

/// ロックの中身の `at`（ms）。無い・読めなければ None。
fn lock_at(body: &[u8]) -> Option<u128> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value.get("at")?.as_u64().map(u128::from)
}

/// 古いロックか（U52 D5。Android の U44 と同じ判断）:
/// - 中身に `at` がある: `at`（書いた端末の時計）と更新時刻（NAS の時計）の**両方**が `ttl` より古い
/// - 中身が空・読めない（作った直後でまだ書いていない、など）: 更新時刻が古い
/// - 更新時刻が分からなければ古いと見なさない
fn lock_stale(path: &Path, body: &[u8], ttl: Duration) -> bool {
    let Some(age) = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
    else {
        return false;
    };
    if age <= ttl {
        return false;
    }
    match lock_at(body) {
        Some(at) => now_millis().saturating_sub(at) > ttl.as_millis(),
        None => true,
    }
}

/// 排他のロックを取る。あれば、古いもの（[`lock_stale`]）だけ壊して取り直す。取れなければ None。
/// **壊す前にもう一度読み、中身が変わっていれば（ほかの端末が取り直した）壊さない。**
fn acquire_lock(dir: &Path, holder: &str, ttl: Duration) -> Result<Option<LockGuard>, String> {
    let path = dir.join(LOCK_FILE);
    for _ in 0..2 {
        match try_create_lock(&path, holder) {
            Ok(guard) => return Ok(Some(guard)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let first = match fs::read(&path) {
                    Ok(body) => body,
                    // 読む前に消えた → 取り直す。
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(_) => return Ok(None),
                };
                if !lock_stale(&path, &first, ttl) {
                    return Ok(None);
                }
                if !fs::read(&path).is_ok_and(|again| again == first) {
                    return Ok(None);
                }
                // 書いた端末が途中で止まった。壊して取り直す（同時に壊した端末があれば、どちらかが負ける）。
                let _ = fs::remove_file(&path);
            }
            Err(error) => return Err(format!("サイドカーのロックを作れませんでした: {error}")),
        }
    }
    Ok(None)
}

/// 楽観ロックの書き込み（設計書 §4.4）。
///
/// ロックを取る → `catalog.json` を読み、`expected`（画面が判断に使った中身。無かったなら None）と
/// 同じことを確かめる → 一時ファイルに書いて rename → 読み戻して確かめる → ロックを放す。
/// 見た版と違えば**書かずに** `Changed` を返す（画面が判定し直す）。
#[cfg_attr(not(test), allow(dead_code))]
pub fn write_checked(folder: &Path, json: &str, expected: Option<&str>, holder: &str) -> Result<CheckedWrite, String> {
    write_checked_with(folder, json, expected, holder, LOCK_TTL, None)
}

/// [`write_checked`] に、置き換える版の退避を足したもの（U52 D4）。`aside_tag` があり、置き換える版が
/// あれば、**ロックを取って見た版と同じことを確かめたあとで** `catalog.<印>.<時刻>.json` に退避する。
/// 退避が書けなければ置き換えない（Err）。
pub fn write_checked_aside(
    folder: &Path,
    json: &str,
    expected: Option<&str>,
    holder: &str,
    aside_tag: Option<&str>,
) -> Result<CheckedWrite, String> {
    write_checked_with(folder, json, expected, holder, LOCK_TTL, aside_tag)
}

fn write_checked_with(
    folder: &Path,
    json: &str,
    expected: Option<&str>,
    holder: &str,
    ttl: Duration,
    aside_tag: Option<&str>,
) -> Result<CheckedWrite, String> {
    let dir = sidecar_dir(folder);
    fs::create_dir_all(&dir).map_err(|error| format!("サイドカーのフォルダを作れませんでした: {error}"))?;
    let Some(_lock) = acquire_lock(&dir, holder, ttl)? else {
        return Ok(CheckedWrite::Locked);
    };
    let current = read(folder)?;
    if current.as_deref() != expected {
        return Ok(CheckedWrite::Changed);
    }
    if let (Some(tag), Some(replaced)) = (aside_tag, current.as_deref()) {
        // **退避に失敗したら上書きしない。** 消してしまうより、次に持ち越す。
        write_aside(folder, tag, replaced)?;
    }
    write(folder, SIDECAR_FILE, json)?;
    // 古い版のアプリはロックを見ない。読み戻して、自分の書いたものが残っているかを確かめる。
    if read(folder)?.as_deref() != Some(json) {
        return Ok(CheckedWrite::Changed);
    }
    Ok(CheckedWrite::Written)
}

// ---------------------------------------------------------------------------
// 退避（U52 D4。名前・数は Android の U44 と同じ）
// ---------------------------------------------------------------------------

/// NAS の退避を、印ごとにいくつまで残すか（Android の `ASIDE_KEEP` と同じ。CON-3 のため小さく）。
pub const ASIDE_KEEP: usize = 5;
/// 端末の中の退避を、プロジェクトごとにいくつまで残すか（Android の `aside/` と同じ）。
pub const LOCAL_ASIDE_KEEP: usize = 5;

/// 退避の名前に使う端末の印か（画面の `asideTag` が作る: 英数字・`-`・`_`、12 文字まで）。
fn valid_aside_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 64 && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// UTC の `yyyyMMddHHmmss`（名前の順が時刻の順になる）。
fn utc_stamp(at: SystemTime) -> String {
    let secs = at.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // 日付（proleptic グレゴリオ暦。Howard Hinnant の civil_from_days）。
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// 退避の名前（`catalog.<印>.<UTC yyyyMMddHHmmss>.json`、`n` が 2 以上なら `-n`）。
pub fn aside_name(tag: &str, stamp: &str, n: u32) -> String {
    if n > 1 {
        format!("catalog.{tag}.{stamp}-{n}.json")
    } else {
        format!("catalog.{tag}.{stamp}.json")
    }
}

/// その印の時刻つきの退避なら（時刻, 通し番号）。時刻の無い古い名前は None。
fn parse_aside(name: &str, tag: &str) -> Option<(String, u32)> {
    let rest = name.strip_prefix("catalog.")?.strip_prefix(tag)?.strip_prefix('.')?.strip_suffix(".json")?;
    let (stamp, n) = match rest.split_once('-') {
        Some((stamp, n)) => (stamp, n.parse::<u32>().ok()?),
        None => (rest, 1),
    };
    (stamp.len() == 14 && stamp.chars().all(|c| c.is_ascii_digit())).then(|| (stamp.to_string(), n))
}

/// 退避を NAS に書く（いまの時刻で）。返すのは書いた名前。
pub fn write_aside(folder: &Path, tag: &str, json: &str) -> Result<String, String> {
    write_aside_at(folder, tag, json, SystemTime::now())
}

/// 退避を NAS に書く。**無いときだけ作る**（`create_new`。前の退避もほかの端末の退避も上書きしない）。
/// 同じ秒に重なれば `-2` 以降。書けたら、同じ印の時刻つきの退避を新しい [`ASIDE_KEEP`] 個だけ残す
/// （片付けの失敗は止めない。時刻の無い古い名前・ほかの印の退避は消さない）。
pub fn write_aside_at(folder: &Path, tag: &str, json: &str, now: SystemTime) -> Result<String, String> {
    if !valid_aside_tag(tag) {
        return Err("退避のファイル名が正しくありません。".to_string());
    }
    let dir = sidecar_dir(folder);
    fs::create_dir_all(&dir).map_err(|error| format!("サイドカーのフォルダを作れませんでした: {error}"))?;
    let stamp = utc_stamp(now);
    // 同じ秒の退避より後ろの番号から（片付けで消えた番号を使い直すと、新しい退避が古い扱いで消える）。
    let first = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| parse_aside(&entry.file_name().to_string_lossy(), tag))
                .filter(|(found, _)| *found == stamp)
                .map(|(_, n)| n)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
        + 1;
    let mut written = None;
    for n in first..first + 9 {
        let name = aside_name(tag, &stamp, n);
        match fs::OpenOptions::new().write(true).create_new(true).open(dir.join(&name)) {
            Ok(mut file) => {
                let result = file.write_all(json.as_bytes()).and_then(|()| file.sync_all());
                drop(file);
                if let Err(error) = result {
                    // 書きかけの退避は残さない（中身の無い退避を、あるものと思わせない）。
                    let _ = fs::remove_file(dir.join(&name));
                    return Err(format!("NAS に退避できませんでした: {error}"));
                }
                written = Some(name);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("NAS に退避できませんでした: {error}")),
        }
    }
    let Some(name) = written else {
        return Err("NAS に退避のファイルを作れませんでした（同じ名前がありました）".to_string());
    };
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut ours: Vec<(String, u32, PathBuf)> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let file = entry.file_name().to_string_lossy().into_owned();
                parse_aside(&file, tag).map(|(stamp, n)| (stamp, n, entry.path()))
            })
            .collect();
        ours.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        for (_, _, path) in ours.into_iter().skip(ASIDE_KEEP) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(name)
}

/// 端末の中に退避する（`dir` はアプリのデータフォルダの `aside/`）。名前は `<プロジェクト>-<ミリ秒>.json`
/// （同じ時刻があれば 1 つずらす。前の退避を上書きしない）。プロジェクトごとに最新 [`LOCAL_ASIDE_KEEP`] 個だけ残す。
pub fn write_local_aside(dir: &Path, project_id: &str, json: &str) -> Result<(), String> {
    if project_id.is_empty() || !project_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("プロジェクトの id が正しくありません。".to_string());
    }
    fs::create_dir_all(dir).map_err(|error| format!("端末の中に退避できませんでした: {error}"))?;
    let mut at = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    loop {
        let path = dir.join(format!("{project_id}-{at}.json"));
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let result = file.write_all(json.as_bytes()).and_then(|()| file.sync_all());
                drop(file);
                if let Err(error) = result {
                    let _ = fs::remove_file(&path);
                    return Err(format!("端末の中に退避できませんでした: {error}"));
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => at += 1,
            Err(error) => return Err(format!("端末の中に退避できませんでした: {error}")),
        }
    }
    let prefix = format!("{project_id}-");
    if let Ok(entries) = fs::read_dir(dir) {
        let mut ours: Vec<(u128, PathBuf)> = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let millis = name.strip_prefix(&prefix)?.strip_suffix(".json")?.parse::<u128>().ok()?;
                Some((millis, entry.path()))
            })
            .collect();
        ours.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in ours.into_iter().skip(LOCAL_ASIDE_KEEP) {
            let _ = fs::remove_file(path);
        }
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
        let state = SidecarState { seen_at: 1234, seen_by: "dev-a".into(), local_changed: true, ..SidecarState::default() };
        save_state(&conn, "p1", &state).unwrap();
        assert_eq!(load_state(&conn, "p1").unwrap(), state);
        let cleared = SidecarState { local_changed: false, ..state.clone() };
        save_state(&conn, "p1", &cleared).unwrap();
        assert_eq!(load_state(&conn, "p1").unwrap(), cleared);
        // 別のプロジェクトには影響しない。
        assert_eq!(load_state(&conn, "p2").unwrap(), SidecarState::default());
    }

    #[test]
    fn new_fields_round_trip_and_default_to_the_old_form() {
        let conn = memory();
        let state = SidecarState {
            seen_at: 5,
            seen_by: "dev".into(),
            local_changed: false,
            seen_token: Some("w-1".into()),
            seen_key: "j1:abc".into(),
            seen_epoch: Some("e-1".into()),
            local_epoch: Some("e-2".into()),
            detached: true,
        };
        save_state(&conn, "p1", &state).unwrap();
        assert_eq!(load_state(&conn, "p1").unwrap(), state);
        // 一度も見ていない（空の token）と、古い形（None）は区別して残る。
        let never = SidecarState { seen_token: Some(String::new()), ..SidecarState::default() };
        save_state(&conn, "p2", &never).unwrap();
        assert_eq!(load_state(&conn, "p2").unwrap().seen_token.as_deref(), Some(""));
        // 画面から来る JSON に新しい項目が無くても読める（古い版の画面）。
        let old: SidecarState =
            serde_json::from_str(r#"{"seenAt":1,"seenBy":"x","localChanged":true}"#).unwrap();
        assert_eq!(old.seen_token, None);
        assert!(!old.detached);
    }

    #[test]
    fn old_table_gets_the_new_columns_and_keeps_its_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE sidecar_state (
               project_id TEXT PRIMARY KEY,
               seen_at INTEGER NOT NULL DEFAULT 0,
               seen_by TEXT NOT NULL DEFAULT '',
               local_changed INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO sidecar_state VALUES ('p1', 777, 'pc', 1);",
        )
        .unwrap();
        ensure_tables(&conn).unwrap();
        let state = load_state(&conn, "p1").unwrap();
        assert_eq!((state.seen_at, state.seen_by.as_str(), state.local_changed), (777, "pc", true));
        assert_eq!(state.seen_token, None, "古い行は古い形のまま（画面が legacy: の控えを作る）");
        assert_eq!(state.seen_key, "");
        assert!(!state.detached);
    }

    fn lock_path(folder: &Path) -> PathBuf {
        folder.join(SIDECAR_DIR).join(LOCK_FILE)
    }

    fn leftovers(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder.join(SIDECAR_DIR))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn at_utc(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    /// U52 D4: NAS の退避は Android（U44）と同じ名前で、上書きせず、同じ印の時刻つきは新しい 5 つだけ残す。
    #[test]
    fn aside_is_timestamped_never_overwrites_and_keeps_the_newest_five() {
        let folder = temp_dir("aside-stamp");
        let dir = folder.join(SIDECAR_DIR);
        fs::create_dir_all(&dir).unwrap();
        // 時刻の無い古い名前・ほかの印の退避は消さない。
        fs::write(dir.join("catalog.abc.json"), "old").unwrap();
        fs::write(dir.join("catalog.other.20200101000000.json"), "other").unwrap();
        // 2026-10-03 04:05:06 UTC
        let base = 1_791_000_306;
        let first = write_aside_at(&folder, "abc", "v1", at_utc(base)).unwrap();
        assert_eq!(first, "catalog.abc.20261003040506.json");
        // 同じ秒なら -2（前の退避を上書きしない）。
        let second = write_aside_at(&folder, "abc", "v2", at_utc(base)).unwrap();
        assert_eq!(second, "catalog.abc.20261003040506-2.json");
        assert_eq!(fs::read_to_string(dir.join(&first)).unwrap(), "v1");
        // 同じ秒に 7 つ書いても、どれも別の名前で、片付けで消えるのは古い番号から。
        let mut same: Vec<String> = Vec::new();
        for index in 0..7 {
            same.push(write_aside_at(&folder, "burst", &format!("b{index}"), at_utc(base)).unwrap());
        }
        let unique: std::collections::HashSet<&String> = same.iter().collect();
        assert_eq!(unique.len(), 7);
        assert!(dir.join(same.last().unwrap()).exists(), "いちばん新しい退避は残る");
        for offset in 1..=5 {
            write_aside_at(&folder, "abc", "later", at_utc(base + offset)).unwrap();
        }
        let ours: Vec<String> = leftovers(&folder)
            .into_iter()
            .filter(|name| name.starts_with("catalog.abc.2"))
            .collect();
        assert_eq!(ours.len(), 5, "{ours:?}");
        assert!(!ours.contains(&first) && !ours.contains(&second), "古い 2 つが消える: {ours:?}");
        assert_eq!(fs::read_to_string(dir.join("catalog.abc.json")).unwrap(), "old");
        assert!(dir.join("catalog.other.20200101000000.json").exists());
        // 印に使えない文字は断る。
        assert!(write_aside_at(&folder, "a/b", "x", at_utc(base)).is_err());
        let _ = fs::remove_dir_all(&folder);
    }

    /// U52 D4: 置き換える版の退避は、ロックを取って見た版と同じことを確かめた**あと**で書く。
    #[test]
    fn checked_write_asides_the_replaced_version_only_after_checking() {
        let folder = temp_dir("checked-aside");
        write(&folder, SIDECAR_FILE, "android-v1").unwrap();
        // 見た版と違う → 書かない。退避も作らない。
        assert_eq!(
            write_checked_aside(&folder, "pc", Some("old"), "pc", Some("zzzzzzzz-111")).unwrap(),
            CheckedWrite::Changed
        );
        assert_eq!(leftovers(&folder), vec![SIDECAR_FILE.to_string()]);
        // 見た版のまま → 退避してから書く。
        assert_eq!(
            write_checked_aside(&folder, "pc", Some("android-v1"), "pc", Some("zzzzzzzz-111")).unwrap(),
            CheckedWrite::Written
        );
        let names = leftovers(&folder);
        let aside: Vec<&String> = names.iter().filter(|name| name.starts_with("catalog.zzzzzzzz-111.")).collect();
        assert_eq!(aside.len(), 1, "{names:?}");
        assert_eq!(fs::read_to_string(folder.join(SIDECAR_DIR).join(aside[0])).unwrap(), "android-v1");
        assert_eq!(read(&folder).unwrap().as_deref(), Some("pc"));
        let _ = fs::remove_dir_all(&folder);
    }

    /// U52 D4: 退避を書けなければ、置き換えない。
    #[test]
    fn checked_write_does_not_replace_when_the_aside_cannot_be_written() {
        let folder = temp_dir("checked-aside-fail");
        write(&folder, SIDECAR_FILE, "android-v1").unwrap();
        // 印が正しくない（退避の名前を作れない）→ 書かない。
        assert!(write_checked_aside(&folder, "pc", Some("android-v1"), "pc", Some("bad/tag")).is_err());
        assert_eq!(read(&folder).unwrap().as_deref(), Some("android-v1"));
        assert!(!lock_path(&folder).exists());
        let _ = fs::remove_dir_all(&folder);
    }

    /// U52 D4: 端末の中の退避（アプリのデータフォルダの `aside/`）。プロジェクトごとに最新 5 つ。
    #[test]
    fn local_aside_keeps_the_newest_five_per_project() {
        let dir = temp_dir("local-aside").join("aside");
        for index in 0..7 {
            write_local_aside(&dir, "p1", &format!("v{index}")).unwrap();
        }
        write_local_aside(&dir, "p2", "other").unwrap();
        let mut mine: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("p1-"))
            .collect();
        mine.sort();
        assert_eq!(mine.len(), 5, "{mine:?}");
        let contents: Vec<String> = mine.iter().map(|name| fs::read_to_string(dir.join(name)).unwrap()).collect();
        assert_eq!(contents, vec!["v2", "v3", "v4", "v5", "v6"]);
        assert!(fs::read_dir(&dir).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().starts_with("p2-")));
        // プロジェクトの id に使えない文字は断る。
        assert!(write_local_aside(&dir, "../x", "x").is_err());
    }

    #[test]
    fn checked_write_creates_replaces_and_releases_the_lock() {
        let folder = temp_dir("checked");
        assert_eq!(write_checked(&folder, "v1", None, "pc").unwrap(), CheckedWrite::Written);
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Written);
        assert_eq!(read(&folder).unwrap().as_deref(), Some("v2"));
        // ロックも一時ファイルも残らない。
        assert_eq!(leftovers(&folder), vec![SIDECAR_FILE.to_string()]);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn checked_write_does_not_write_over_a_version_it_did_not_see() {
        let folder = temp_dir("checked-changed");
        write(&folder, SIDECAR_FILE, "android-progressed").unwrap();
        // 画面は「無かった」と思って判断した → 書かない。
        assert_eq!(write_checked(&folder, "pc-untouched", None, "pc").unwrap(), CheckedWrite::Changed);
        // 画面は古い版を見て判断した → 書かない。
        assert_eq!(write_checked(&folder, "pc-untouched", Some("old"), "pc").unwrap(), CheckedWrite::Changed);
        assert_eq!(read(&folder).unwrap().as_deref(), Some("android-progressed"));
        assert_eq!(leftovers(&folder), vec![SIDECAR_FILE.to_string()]);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn checked_write_waits_for_a_fresh_lock_and_breaks_a_stale_one() {
        let folder = temp_dir("checked-lock");
        write(&folder, SIDECAR_FILE, "v1").unwrap();
        fs::write(lock_path(&folder), "{\"holder\":\"android\"}").unwrap();
        // 新しいロック → 書かない（ほかの端末が書いている）。ロックは残す。
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Locked);
        assert_eq!(read(&folder).unwrap().as_deref(), Some("v1"));
        assert!(lock_path(&folder).exists());
        // 古いロック（TTL より前）→ 壊して書く。
        let old = SystemTime::now() - Duration::from_secs(120);
        fs::File::options().write(true).open(lock_path(&folder)).unwrap().set_modified(old).unwrap();
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Written);
        assert_eq!(read(&folder).unwrap().as_deref(), Some("v2"));
        assert!(!lock_path(&folder).exists());
        let _ = fs::remove_dir_all(&folder);
    }

    fn now_ms() -> u128 {
        SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis()
    }

    fn age_lock(folder: &Path, secs: u64) {
        let old = SystemTime::now() - Duration::from_secs(secs);
        fs::File::options().write(true).open(lock_path(folder)).unwrap().set_modified(old).unwrap();
    }

    /// U52 D5（Android の U44 と同じ判断）: 中身の `at` が新しければ、更新時刻が古くても壊さない
    /// （NAS の時計と PC の時計がずれていても、生きているロックを壊さない）。
    #[test]
    fn lock_with_a_fresh_at_is_not_broken_even_if_its_modified_time_is_old() {
        let folder = temp_dir("lock-fresh-at");
        write(&folder, SIDECAR_FILE, "v1").unwrap();
        fs::write(lock_path(&folder), format!("{{\"holder\":\"android\",\"at\":{}}}", now_ms())).unwrap();
        age_lock(&folder, 120);
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Locked);
        assert_eq!(read(&folder).unwrap().as_deref(), Some("v1"));
        assert!(lock_path(&folder).exists());
        let _ = fs::remove_dir_all(&folder);
    }

    /// U52 D5: `at` と更新時刻の両方が古ければ壊す。`at` が古くても更新時刻が新しければ壊さない。
    #[test]
    fn lock_is_broken_only_when_both_at_and_modified_time_are_old() {
        let folder = temp_dir("lock-both-old");
        write(&folder, SIDECAR_FILE, "v1").unwrap();
        let old_at = now_ms() - 120_000;
        fs::write(lock_path(&folder), format!("{{\"holder\":\"android\",\"at\":{old_at}}}")).unwrap();
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Locked);
        age_lock(&folder, 120);
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Written);
        assert!(!lock_path(&folder).exists());
        let _ = fs::remove_dir_all(&folder);
    }

    /// U52 D5: 中身が空（作った直後でまだ書いていない）なら、更新時刻が古いときだけ壊す。
    #[test]
    fn empty_lock_is_broken_only_when_its_modified_time_is_old() {
        let folder = temp_dir("lock-empty");
        write(&folder, SIDECAR_FILE, "v1").unwrap();
        fs::write(lock_path(&folder), "").unwrap();
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Locked);
        age_lock(&folder, 120);
        assert_eq!(write_checked(&folder, "v2", Some("v1"), "pc").unwrap(), CheckedWrite::Written);
        let _ = fs::remove_dir_all(&folder);
    }

    /// U52 D5: 放すのは自分が書いた中身のときだけ（ほかの端末が古いと見なして取り直したロックは残す）。
    #[test]
    fn releasing_leaves_a_lock_that_someone_else_took_over() {
        let folder = temp_dir("lock-release");
        let dir = folder.join(SIDECAR_DIR);
        fs::create_dir_all(&dir).unwrap();
        let guard = acquire_lock(&dir, "pc", LOCK_TTL).unwrap().expect("取れる");
        let mine = fs::read_to_string(lock_path(&folder)).unwrap();
        assert!(mine.contains("\"holder\":\"pc\"") && mine.contains("\"at\":"), "{mine}");
        // ほかの端末が取り直した。
        fs::write(lock_path(&folder), "{\"holder\":\"android\",\"at\":1}").unwrap();
        drop(guard);
        assert_eq!(fs::read_to_string(lock_path(&folder)).unwrap(), "{\"holder\":\"android\",\"at\":1}");
        // 自分のロックなら放す。
        fs::remove_file(lock_path(&folder)).unwrap();
        let guard = acquire_lock(&dir, "pc", LOCK_TTL).unwrap().expect("取れる");
        drop(guard);
        assert!(!lock_path(&folder).exists());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn checked_write_releases_the_lock_when_it_fails() {
        let folder = temp_dir("checked-fail");
        // catalog.json が読めない（フォルダになっている）→ 失敗。ロックは放し、何も書かない。
        fs::create_dir_all(folder.join(SIDECAR_DIR).join(SIDECAR_FILE).join("inner")).unwrap();
        assert!(write_checked(&folder, "v2", None, "pc").is_err());
        assert!(!lock_path(&folder).exists());
        assert_eq!(leftovers(&folder), vec![SIDECAR_FILE.to_string()]);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn checked_write_result_is_sent_as_a_lowercase_word() {
        assert_eq!(serde_json::to_string(&CheckedWrite::Written).unwrap(), "\"written\"");
        assert_eq!(serde_json::to_string(&CheckedWrite::Changed).unwrap(), "\"changed\"");
        assert_eq!(serde_json::to_string(&CheckedWrite::Locked).unwrap(), "\"locked\"");
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
