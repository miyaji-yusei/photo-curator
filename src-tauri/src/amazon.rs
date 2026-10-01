//! Amazon Photos 共有リンク（設計 08 章・07 章 段7）。
//!
//! **ログイン・Cookie は使わない。読み取り専用。** 共有ページ自身が読んでいる
//! 非公式 API を、PC は Rust から直接呼ぶ（ブラウザの CORS 制限を受けない。
//! 2026-09-24 の実測で、JSON の API はブラウザからも読めたが、画像本体の
//! `fetch()` は CORS で失敗することを確認した。Web 版がこの出所に対応しない
//! 理由もそこにある）。

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::civil_timestamp_ms;

/// FILE 以外は 2 階層まで潜る（08章「落とし穴」3）。
const MAX_DEPTH: u32 = 2;

#[derive(Debug, Clone)]
pub struct AmazonSource {
    pub host: String,
    pub share_id: String,
}

/// リンクから host・shareId を取り出す。
/// `https://www.amazon.co.jp/photos/share/{id}` `.../clouddrive/share/{id}` の両方を受ける。
pub fn parse_share_url(url: &str) -> Option<AmazonSource> {
    let trimmed = url.trim();
    let after_scheme = trimmed.split("://").nth(1).unwrap_or(trimmed);
    let mut parts = after_scheme.splitn(2, '/');
    let host = parts.next()?.to_string();
    if !host.starts_with("www.amazon.") {
        return None;
    }
    let rest = parts.next().unwrap_or("");
    for prefix in ["photos/share/", "clouddrive/share/"] {
        if let Some(after) = rest.strip_prefix(prefix) {
            let share_id: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if !share_id.is_empty() {
                return Some(AmazonSource { host, share_id });
            }
        }
    }
    None
}

/// `ProjectSource.key` の形（08章5.1: `"{host}|{shareId}"`）。
pub fn key_for(source: &AmazonSource) -> String {
    format!("{}|{}", source.host, source.share_id)
}

pub fn parse_key(key: &str) -> Option<AmazonSource> {
    let (host, share_id) = key.split_once('|')?;
    Some(AmazonSource { host: host.to_string(), share_id: share_id.to_string() })
}

/// プロセスで 1 つの共有 agent。呼ぶたびに作ると接続（TCP＋TLS。tempLink は 302 で別ホストへ
/// 飛ぶので 2 ホスト分）を毎回やり直すことになる（U14）。ureq 2 の既定は 1 ホスト 1 本しか
/// 使い回さないので、並列数（`WORKERS`）以上に広げる。
fn agent() -> &'static ureq::Agent {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .max_idle_connections(32)
            .max_idle_connections_per_host(WORKERS.max(8))
            .build()
    });
    &AGENT
}

/// 共有リンクが消えた（404 など）ときの文。これが出たら準備を自動でやり直さない。
pub const GONE_MESSAGE: &str = "このリンクは削除されたか、無効です。";

fn describe_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(404, _) => GONE_MESSAGE.to_string(),
        ureq::Error::Status(code, _) => format!("Amazon から読めませんでした（{code}）。"),
        ureq::Error::Transport(_) => "ネットワークにつながっていません。".to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct ShareInfo {
    #[serde(rename = "nodeInfo")]
    node_info: NodeInfo,
}
#[derive(Debug, Deserialize)]
struct NodeInfo {
    id: String,
    name: String,
}

pub struct ShareRoot {
    pub node_id: String,
    pub name: String,
}

/// ① 共有の中身。
pub fn fetch_share_root(source: &AmazonSource) -> Result<ShareRoot, String> {
    let url = format!(
        "https://{}/drive/v1/shares/{}?shareId={}&resourceVersion=V2&ContentType=JSON",
        source.host, source.share_id, source.share_id
    );
    let resp = agent().get(&url).call().map_err(describe_error)?;
    let info: ShareInfo = resp.into_json().map_err(|e| e.to_string())?;
    Ok(ShareRoot { node_id: info.node_info.id, name: info.node_info.name })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmazonNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "contentProperties")]
    pub content_properties: Option<ContentProperties>,
    #[serde(rename = "tempLink")]
    pub temp_link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentProperties {
    #[serde(rename = "contentType")]
    pub content_type: Option<String>,
    #[serde(rename = "contentDate")]
    pub content_date: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ChildrenPage {
    count: usize,
    data: Vec<AmazonNode>,
}

/// ② 子の一覧。200 件ずつ、offset で送る（nextToken は来ない。08章2.2）。
fn fetch_children_page(source: &AmazonSource, node_id: &str, offset: usize) -> Result<ChildrenPage, String> {
    let url = format!(
        "https://{}/drive/v1/nodes/{}/children?asset=ALL&limit=200&offset={}&searchOnFamily=false&tempLink=true&shareId={}&sort=%5B%27contentProperties.contentDate+ASC%27%5D&resourceVersion=V2&ContentType=JSON",
        source.host, node_id, offset, source.share_id
    );
    let resp = agent().get(&url).call().map_err(describe_error)?;
    resp.into_json().map_err(|e| e.to_string())
}

fn fetch_all_children(source: &AmazonSource, node_id: &str) -> Result<Vec<AmazonNode>, String> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    loop {
        let page = fetch_children_page(source, node_id, offset)?;
        let got = page.data.len();
        out.extend(page.data);
        offset += got;
        if got == 0 || offset >= page.count {
            break;
        }
    }
    Ok(out)
}

fn is_image_node(node: &AmazonNode) -> bool {
    node.content_properties
        .as_ref()
        .and_then(|p| p.content_type.as_deref())
        .map(|t| t.starts_with("image/"))
        .unwrap_or(false)
}

/// FILE 以外は 2 階層まで潜り、`image/*` の FILE だけ集める
/// （**拡張子で判定しない**。08章「落とし穴」1・2）。
pub fn list_photos(source: &AmazonSource, root_node_id: &str) -> Result<Vec<AmazonNode>, String> {
    let children = fetch_all_children(source, root_node_id)?;
    list_photos_from_children(source, children)
}

/// 直下の子を取ってあるとき用（`preview()` が直下を 1 回だけ取って使う）。
fn list_photos_from_children(source: &AmazonSource, top_children: Vec<AmazonNode>) -> Result<Vec<AmazonNode>, String> {
    let mut photos = Vec::new();
    collect_children(source, top_children, 0, &mut photos)?;
    Ok(photos)
}

fn collect_children(source: &AmazonSource, children: Vec<AmazonNode>, depth: u32, out: &mut Vec<AmazonNode>) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for child in children {
        if child.kind == "FILE" {
            if is_image_node(&child) {
                out.push(child);
            }
        } else {
            if depth + 1 > MAX_DEPTH {
                continue;
            }
            let grandchildren = fetch_all_children(source, &child.id)?;
            collect_children(source, grandchildren, depth + 1, out)?;
        }
    }
    Ok(())
}

/// ③ 画像バイト列。`view_box` を指定するとその長辺まで縮小して来る（無指定は原本）。
pub fn fetch_image_bytes(temp_link: &str, view_box: Option<u32>) -> Result<Vec<u8>, String> {
    let url = match view_box {
        Some(edge) => format!("{temp_link}?viewBox={edge},{edge}"),
        None => temp_link.to_string(),
    };
    let resp = agent().get(&url).call().map_err(describe_error)?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(64 * 1024 * 1024) // 原本でも 64MB を超えることはない想定。念のため上限を置く。
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// 撮影時刻（ミリ秒）。**末尾の `Z` を信じない。** Amazon は EXIF のその土地の
/// 時計にそのまま `Z` を付けて返すので、offset を無視してそのまま読む
/// （08章「落とし穴」6。09-12 に Android で見つかった 9 時間ずれの教訓と同じ）。
pub fn parse_content_date(content_date: &str) -> Option<i64> {
    let cleaned = content_date.trim_end_matches('Z');
    let (date_part, time_part) = cleaned.split_once('T')?;
    let date: Vec<i64> = date_part.splitn(3, '-').filter_map(|s| s.parse().ok()).collect();
    let time_main = time_part.split('.').next().unwrap_or(time_part);
    let time: Vec<i64> = time_main.splitn(3, ':').filter_map(|s| s.parse().ok()).collect();
    if date.len() != 3 || time.len() != 3 {
        return None;
    }
    civil_timestamp_ms(date[0], date[1], date[2], time[0], time[1], time[2])
}

// ---------------------------------------------------------------------------
// 作成画面の見本（08章8.1）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmazonPreview {
    /// `ProjectSource.key`（`"{host}|{shareId}"`）。作成するときそのまま使う。
    pub key: String,
    pub name: String,
    pub count: usize,
    /// 先頭12枚ぶんの見本（`viewBox=160` を JPEG のまま）。原本は読まない。
    pub samples: Vec<Vec<u8>>,
}

/// 共有リンクを読み、名前・枚数・見本12枚を返す（作成画面。08章「プロジェクト名の決め方」）。
pub fn preview(share_url: &str, sample_limit: usize) -> Result<AmazonPreview, String> {
    let source = parse_share_url(share_url).ok_or_else(|| "Amazon Photos の共有リンクの形ではありません。".to_string())?;
    let root = fetch_share_root(&source)?;
    // 直下の子は 1 回だけ取り、走査とアルバム名の決定の両方に使う（U14）。
    let top_children = fetch_all_children(&source, &root.node_id)?;
    // 共有の直下がアルバム1つだけなら、そのアルバムの名前を使う（08章「プロジェクト名の決め方」）。
    let albums: Vec<&AmazonNode> = top_children.iter().filter(|n| n.kind != "FILE").collect();
    let name = if albums.len() == 1 { albums[0].name.clone() } else { root.name.clone() };
    let nodes = list_photos_from_children(&source, top_children)?;

    let mut samples = Vec::new();
    for node in nodes.iter().take(sample_limit) {
        let Some(temp_link) = &node.temp_link else { continue };
        if let Ok(bytes) = fetch_image_bytes(temp_link, Some(160)) {
            samples.push(bytes);
        }
    }
    Ok(AmazonPreview { key: key_for(&source), name, count: nodes.len(), samples })
}

// ---------------------------------------------------------------------------
// tempLink の控え（表 `amazon_links`）と、それを使って画像を取る道具
// ---------------------------------------------------------------------------

/// 表示用・サムネイルを取るときの並列数（U14: 4 → 8。08章の約束を、ユーザー決定で 8 に変えた）。
pub const WORKERS: usize = 8;

pub fn ensure_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS amazon_links (
           project_id TEXT NOT NULL,
           node_id TEXT NOT NULL,
           temp_link TEXT NOT NULL,
           PRIMARY KEY (project_id, node_id)
         );",
    )
    .map_err(|error| error.to_string())
}

/// そのプロジェクトの tempLink を丸ごと入れ替える（一覧を読み直したときの全部が最新）。
pub fn save_links(conn: &Connection, project_id: &str, links: &[(String, String)]) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|error| error.to_string())?;
    tx.execute("DELETE FROM amazon_links WHERE project_id=?1", params![project_id])
        .map_err(|error| error.to_string())?;
    for (node_id, temp_link) in links {
        tx.execute(
            "INSERT INTO amazon_links (project_id,node_id,temp_link) VALUES (?1,?2,?3)
             ON CONFLICT(project_id,node_id) DO UPDATE SET temp_link=excluded.temp_link",
            params![project_id, node_id, temp_link],
        )
        .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

pub fn load_links(conn: &Connection, project_id: &str) -> Result<HashMap<String, String>, String> {
    let mut statement = conn
        .prepare("SELECT node_id,temp_link FROM amazon_links WHERE project_id=?1")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<HashMap<_, _>, _>>().map_err(|error| error.to_string())
}

pub fn delete_links(conn: &Connection, project_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM amazon_links WHERE project_id=?1", params![project_id])
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 一覧の写真から (node id, tempLink) の組を作る。tempLink が無い行は入れない。
pub fn links_of(nodes: &[AmazonNode]) -> Vec<(String, String)> {
    nodes
        .iter()
        .filter_map(|node| node.temp_link.clone().map(|link| (node.id.clone(), link)))
        .collect()
}

/// 撮影時刻（`contentDate`）。無ければ None。
pub fn captured_at_of(node: &AmazonNode) -> Option<i64> {
    node.content_properties
        .as_ref()
        .and_then(|p| p.content_date.as_deref())
        .and_then(parse_content_date)
}

/// 端末に置くファイル名に使える形にする（node id はそのままパスにしない）。
pub fn safe_file_stem(node_id: &str) -> String {
    node_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// あるプロジェクトの tempLink と、切れたときの取り直し。
///
/// 画像を取って失敗したら、一覧を読み直して tempLink を取り直し、**1 回だけ**やり直す。
/// 共有そのものが消えていた（404）と分かったら `gone` を立て、以後は網を使わずに失敗を返す
/// （準備を自動でやり直し続けない）。
pub struct LinkBook {
    project_id: String,
    source: AmazonSource,
    db_path: PathBuf,
    inner: Mutex<BookInner>,
    gone: AtomicBool,
}

struct BookInner {
    generation: u64,
    links: HashMap<String, String>,
    /// 一覧の読み直しが最後に失敗した時刻と理由。`REFRESH_RETRY_AFTER` のあいだは網を使わず、
    /// 同じ理由を返す（回線が切れているとき、写真の枚数ぶん、読み直しの接続待ちを
    /// 順番に繰り返さないため）。
    failed: Option<(Instant, String)>,
}

/// 一覧の読み直しが失敗したあと、次に試すまで待つ時間。
const REFRESH_RETRY_AFTER: Duration = Duration::from_secs(15);

impl LinkBook {
    pub fn load(db_path: PathBuf, conn: &Connection, project_id: &str, source: AmazonSource) -> Result<Self, String> {
        Ok(Self {
            project_id: project_id.to_string(),
            source,
            db_path,
            inner: Mutex::new(BookInner {
                generation: 0,
                links: load_links(conn, project_id)?,
                failed: None,
            }),
            gone: AtomicBool::new(false),
        })
    }

    pub fn is_gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }

    fn snapshot(&self, node_id: &str) -> (Option<String>, u64) {
        match self.inner.lock() {
            Ok(inner) => (inner.links.get(node_id).cloned(), inner.generation),
            Err(_) => (None, 0),
        }
    }

    /// 一覧を読み直して tempLink を取り直す。同じ世代を見た worker が同時に来ても、読み直すのは 1 人だけ。
    fn refresh(&self, seen_generation: u64) -> Result<(), String> {
        let mut inner = self.inner.lock().map_err(|_| "tempLink の控えを読めません。".to_string())?;
        if inner.generation != seen_generation {
            return Ok(());
        }
        if let Some((at, message)) = &inner.failed {
            if at.elapsed() < REFRESH_RETRY_AFTER {
                return Err(message.clone());
            }
        }
        let result = fetch_share_root(&self.source).and_then(|root| list_photos(&self.source, &root.node_id));
        let nodes = match result {
            Ok(nodes) => nodes,
            Err(message) => {
                if message == GONE_MESSAGE {
                    self.gone.store(true, Ordering::SeqCst);
                } else {
                    inner.failed = Some((Instant::now(), message.clone()));
                }
                return Err(message);
            }
        };
        inner.failed = None;
        let links = links_of(&nodes);
        inner.links = links.iter().cloned().collect();
        inner.generation += 1;
        // 控えに書けなくても、この実行では使える。
        if let Ok(conn) = Connection::open(&self.db_path) {
            let _ = conn.busy_timeout(Duration::from_secs(5));
            let _ = save_links(&conn, &self.project_id, &links);
        }
        Ok(())
    }

    /// 画像のバイト列。`view_box` が None なら原本。
    pub fn fetch(&self, node_id: &str, view_box: Option<u32>) -> Result<Vec<u8>, String> {
        if self.is_gone() {
            return Err(GONE_MESSAGE.to_string());
        }
        let (link, generation) = self.snapshot(node_id);
        if let Some(link) = &link {
            if let Ok(bytes) = fetch_image_bytes(link, view_box) {
                return Ok(bytes);
            }
        }
        self.refresh(generation)?;
        let (link, _) = self.snapshot(node_id);
        let link = link.ok_or_else(|| "この写真は Amazon Photos に見つかりませんでした。".to_string())?;
        // 共有が生きているのを確かめたあとの失敗は、その 1 枚の失敗として返す。
        fetch_image_bytes(&link, view_box).map_err(|message| {
            if message == GONE_MESSAGE {
                "この写真は Amazon Photos から読めませんでした。".to_string()
            } else {
                message
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_tables(&conn).unwrap();
        conn
    }

    // 回線が切れているとき、一覧の読み直しが失敗するたびに次の写真がまた読み直しに
    // 入り、接続待ちを写真の枚数ぶん順番に繰り返していた（レビュー R14）。失敗の
    // 直後は網を使わず、同じ理由をすぐ返す。試験は網を使わない（使えば別の文言になる）。
    #[test]
    fn a_failed_refresh_is_not_retried_straight_away() {
        let source = parse_share_url("https://www.amazon.co.jp/photos/share/abc").unwrap();
        let book = LinkBook::load(PathBuf::from("unused.sqlite3"), &memory(), "p1", source).unwrap();
        book.inner.lock().unwrap().failed = Some((Instant::now(), "ネットワークにつながっていません。".to_string()));
        let started = Instant::now();
        // リンクの控えに無い写真 → 読み直し → 失敗の直後なので、網を使わずに返る。
        for _ in 0..50 {
            assert_eq!(
                book.fetch("node-without-link", None).unwrap_err(),
                "ネットワークにつながっていません。"
            );
        }
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
        assert!(!book.is_gone());
    }

    #[test]
    fn 共有リンクの形からhostとshareIdを取り出す() {
        let source = parse_share_url("https://www.amazon.co.jp/photos/share/abcDEF-12_3").unwrap();
        assert_eq!(source.host, "www.amazon.co.jp");
        assert_eq!(source.share_id, "abcDEF-12_3");
    }

    #[test]
    fn clouddrive形式と前後の空白と余計なクエリを受ける() {
        let source = parse_share_url("  https://www.amazon.com/clouddrive/share/xyz789?ref_=abc&x=1#top \n").unwrap();
        assert_eq!(source.host, "www.amazon.com");
        assert_eq!(source.share_id, "xyz789");
        let source = parse_share_url("https://www.amazon.co.jp/photos/share/q1w2?ref_=cm_sw_r").unwrap();
        assert_eq!(source.share_id, "q1w2");
    }

    #[test]
    fn 共有ではないリンクとamazonでないリンクは断る() {
        // `/photos/shared/…` は「共有」ではなく別のページ（share の後ろに d が続くだけ）。
        assert!(parse_share_url("https://www.amazon.co.jp/photos/shared/abc").is_none());
        assert!(parse_share_url("https://www.amazon.co.jp/photos/share/").is_none());
        assert!(parse_share_url("https://www.amazon.co.jp/photos/all").is_none());
        assert!(parse_share_url("https://example.com/photos/share/abc").is_none());
        assert!(parse_share_url("").is_none());
    }

    #[test]
    fn keyの行き来ができる() {
        let source = AmazonSource { host: "www.amazon.co.jp".into(), share_id: "abc".into() };
        let key = key_for(&source);
        assert_eq!(key, "www.amazon.co.jp|abc");
        let back = parse_key(&key).unwrap();
        assert_eq!(back.host, source.host);
        assert_eq!(back.share_id, source.share_id);
        assert!(parse_key("abc").is_none());
    }

    #[test]
    fn 撮影時刻はzを信じずその土地の時計のまま読む() {
        let expected = civil_timestamp_ms(2021, 7, 23, 13, 13, 29).unwrap();
        assert_eq!(parse_content_date("2021-07-23T13:13:29.000Z"), Some(expected));
        // Z が無くても、小数秒が無くても同じ数字なら同じ値。
        assert_eq!(parse_content_date("2021-07-23T13:13:29"), Some(expected));
        // 時差が付いていても足し引きしない（そのまま読む）。
        assert_eq!(parse_content_date("2021-07-23T13:13:29Z"), Some(expected));
        assert_eq!(parse_content_date("2021-07-23T00:00:00.000Z"), civil_timestamp_ms(2021, 7, 23, 0, 0, 0));
    }

    #[test]
    fn 読めない撮影時刻はnone() {
        assert_eq!(parse_content_date(""), None);
        assert_eq!(parse_content_date("2021-07-23"), None);
        assert_eq!(parse_content_date("not-a-date"), None);
        assert_eq!(parse_content_date("2021-13-40T25:61:61Z"), None);
    }

    const CHILDREN: &str = r#"{
      "count": 5,
      "data": [
        {"id": "n1", "name": "a.jpg", "kind": "FILE",
         "contentProperties": {"contentType": "image/jpeg", "contentDate": "2021-07-23T13:13:29.000Z", "size": 1234},
         "tempLink": "https://content.example/n1"},
        {"id": "n2", "name": "b.mp4", "kind": "FILE",
         "contentProperties": {"contentType": "video/mp4", "size": 99},
         "tempLink": "https://content.example/n2"},
        {"id": "n3", "name": "c.heic", "kind": "FILE",
         "contentProperties": {"contentType": "image/heic"}},
        {"id": "n4", "name": "拡張子が嘘.jpg", "kind": "FILE",
         "contentProperties": {"contentType": "application/octet-stream"}},
        {"id": "a1", "name": "アルバム", "kind": "FOLDER"}
      ]
    }"#;

    #[test]
    fn 子ノードのjsonからimageだけを拾う() {
        let page: ChildrenPage = serde_json::from_str(CHILDREN).unwrap();
        assert_eq!(page.count, 5);
        let images: Vec<&str> = page.data.iter().filter(|n| n.kind == "FILE" && is_image_node(n)).map(|n| n.id.as_str()).collect();
        // 拡張子ではなく contentType で決める。動画・種類の分からないもの・アルバムは入らない。
        assert_eq!(images, vec!["n1", "n3"]);
        let first = &page.data[0];
        assert_eq!(captured_at_of(first), civil_timestamp_ms(2021, 7, 23, 13, 13, 29));
        assert_eq!(captured_at_of(&page.data[2]), None);
    }

    #[test]
    fn tempLinkの控えを保存して読み出す() {
        let conn = memory();
        let page: ChildrenPage = serde_json::from_str(CHILDREN).unwrap();
        let links = links_of(&page.data);
        // tempLink を持つのは n1・n2 だけ。
        assert_eq!(links.len(), 2);
        save_links(&conn, "p1", &links).unwrap();
        save_links(&conn, "p2", &[("n1".into(), "https://other/n1".into())]).unwrap();
        let loaded = load_links(&conn, "p1").unwrap();
        assert_eq!(loaded.get("n1").map(String::as_str), Some("https://content.example/n1"));
        assert_eq!(loaded.len(), 2);

        // 取り直したら丸ごと入れ替わる（消えた写真の分は残らない）。
        save_links(&conn, "p1", &[("n1".into(), "https://content.example/n1-new".into())]).unwrap();
        let loaded = load_links(&conn, "p1").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get("n1").map(String::as_str), Some("https://content.example/n1-new"));
        // 別のプロジェクトには触れない。
        assert_eq!(load_links(&conn, "p2").unwrap().len(), 1);

        delete_links(&conn, "p1").unwrap();
        assert!(load_links(&conn, "p1").unwrap().is_empty());
        assert_eq!(load_links(&conn, "p2").unwrap().len(), 1);
    }

    #[test]
    fn 端末に置くファイル名はパスにならない() {
        assert_eq!(safe_file_stem("abc-DEF_12"), "abc-DEF_12");
        assert_eq!(safe_file_stem("../../etc/passwd"), "______etc_passwd");
    }
}
