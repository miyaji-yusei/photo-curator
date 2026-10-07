//! 並列実行（取り消しの台帳 TaskRegistry・worker 数・チャンクごとの確定・run_in_parallel と見捨てる仕組み・結果の書き込み）。
//! U54 で lib.rs から移した（中身は変えていない）。

use super::*;

#[derive(Default)]
pub(crate) struct TaskRegistry {
    pub(crate) running: Mutex<HashSet<String>>,
    pub(crate) cancelled: Mutex<HashSet<String>>,
}

impl TaskRegistry {
    pub(crate) fn start(&self, key: &str) -> Result<(), String> {
        let mut running = self
            .running
            .lock()
            .map_err(|_| "Task registry is unavailable")?;
        if !running.insert(key.to_owned()) {
            return Err("This project already has a task in progress.".into());
        }
        self.cancelled
            .lock()
            .map_err(|_| "Task registry is unavailable")?
            .remove(key);
        Ok(())
    }

    pub(crate) fn finish(&self, key: &str) {
        if let Ok(mut running) = self.running.lock() {
            running.remove(key);
        }
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.remove(key);
        }
    }

    pub(crate) fn cancel(&self, key: &str) {
        if let Ok(mut cancelled) = self.cancelled.lock() {
            cancelled.insert(key.to_owned());
        }
    }

    pub(crate) fn is_cancelled(&self, key: &str) -> bool {
        self.cancelled
            .lock()
            .map(|cancelled| cancelled.contains(key))
            .unwrap_or(true)
    }

    pub(crate) fn is_running(&self, key: &str) -> bool {
        self.running
            .lock()
            .map(|running| running.contains(key))
            .unwrap_or(false)
    }
}

/// 解析を誰のために走らせているか。同じ `run_burst_analysis` を、前面の
/// 「選別を開始」からもアイドル時の事前生成からも使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnalysisMode {
    /// 利用者が待っている。worker を既定数まで使う。
    Foreground,
    /// scan 完了後の事前生成。1 worker で、チャンクごとに手を止める。
    Background,
}

impl AnalysisMode {
    /// progress event の `task`。UI はこれを見て、前面のダイアログを出すか
    /// 邪魔にならない帯で知らせるだけにするかを決める。
    pub(crate) fn task(self) -> &'static str {
        match self {
            Self::Foreground => "burst",
            Self::Background => "background",
        }
    }

    pub(crate) fn workers(self, folder: &Path) -> usize {
        match self {
            Self::Foreground => analysis_worker_count_for(folder),
            Self::Background => 1,
        }
    }
}

/// worker 数。既定は「コア数の半分」を 2〜4 に丸めたもの。
///
/// 全開にしないのは、この処理が CPU ではなくディスク律速だから。Routine 2 で
/// 1枚 1.66 ms まで下がった今、支配的なのは読み取りそのものであり、同時に
/// 何本もシークを投げると HDD やネットワークドライブでは総スループットが落ちる。
pub(crate) fn analysis_worker_count() -> usize {
    if let Some(value) = std::env::var(WORKER_COUNT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
    {
        return value.clamp(1, MAX_ANALYSIS_WORKERS);
    }
    let cores = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2);
    (cores / 2).clamp(MIN_ANALYSIS_WORKERS, MAX_ANALYSIS_WORKERS)
}

/// フォルダに合わせた worker 数。ネットワークのフォルダは並列 2 に抑える
/// （同時に何本も投げると NAS 側が詰まって、かえって遅くなる）。
/// 環境変数で明示された値があればそれを優先する。
pub(crate) fn analysis_worker_count_for(folder: &Path) -> usize {
    if std::env::var(WORKER_COUNT_ENV).is_ok() {
        return analysis_worker_count();
    }
    if is_network_path(folder) {
        return NETWORK_ANALYSIS_WORKERS;
    }
    analysis_worker_count()
}

/// ネットワークのフォルダを解析するときの worker 数。
pub(crate) const NETWORK_ANALYSIS_WORKERS: usize = 2;

/// フォルダがネットワーク上か。UNC（`\\` 始まり）か、ドライブの種類が
/// `DRIVE_REMOTE`（割り当て済みのネットワークドライブ）なら true。
#[cfg(windows)]
pub(crate) fn is_network_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

    // windows-sys では別の feature（WindowsProgramming）にあるので、値（4）を直に持つ。
    const DRIVE_REMOTE: u32 = 4;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        // 接頭辞として読めなくても、`\\server` のように `\\` で始まるなら UNC とみなす。
        let text = path.to_string_lossy();
        return text.starts_with("\\\\") && !text.starts_with("\\\\?\\") && !text.starts_with("\\\\.\\");
    };
    let letter = match prefix.kind() {
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) => return true,
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        _ => return false,
    };
    let root: Vec<u16> = std::ffi::OsString::from(format!("{}:\\", letter as char))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `root` は NUL 終端の UTF-16 で、呼び出しの間だけ生きている。
    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
}

/// Windows 以外では常にローカル扱い。
#[cfg(not(windows))]
pub(crate) fn is_network_path(_path: &Path) -> bool {
    false
}
// (id, path, captured_at, timestamp_source)
pub(crate) type MetadataRecord = (String, String, Option<i64>, Option<String>);

/// ハッシュ段階が1枚について必要とするものすべて。
#[derive(Clone, Debug)]
pub(crate) struct HashRecord {
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) captured_at: i64,
    pub(crate) source: TimestampSource,
    pub(crate) cached: CachedAnalysis,
}

pub(crate) struct ChunkOutcome {
    pub(crate) committed: usize,
    /// 並列化後、本番の呼び出し（`flush_results`）は中断しない書き込みしか
    /// 行わないため常に false。キャンセルの判断は `run_in_parallel` の側へ
    /// 移った。`commit_in_chunks` 自体の契約は変えていないので残してある。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) cancelled: bool,
}

// items を ANALYSIS_CHUNK_SIZE ごとに commit しながら処理する。
// キャンセルされたら処理中のチャンクだけを rollback し、それまでに commit
// 済みの件数を返す。解析全体をひとつのトランザクションで囲んでいた頃は、
// 中断のたびに全件 rollback され、何度やり直しても結果が残らなかった。
pub(crate) fn commit_in_chunks<T>(
    conn: &Connection,
    items: &[T],
    is_cancelled: &dyn Fn() -> bool,
    on_item: &mut dyn FnMut(&Connection, &T, usize) -> Result<(), String>,
) -> Result<ChunkOutcome, String> {
    let mut committed = 0usize;
    for chunk in items.chunks(ANALYSIS_CHUNK_SIZE) {
        let transaction = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let mut cancelled = false;
        for (offset, item) in chunk.iter().enumerate() {
            if is_cancelled() {
                cancelled = true;
                break;
            }
            on_item(&transaction, item, committed + offset)?;
        }
        if cancelled {
            transaction.rollback().map_err(|error| error.to_string())?;
            return Ok(ChunkOutcome {
                committed,
                cancelled: true,
            });
        }
        transaction.commit().map_err(|error| error.to_string())?;
        committed += chunk.len();
    }
    Ok(ChunkOutcome {
        committed,
        cancelled: false,
    })
}

// ---------------------------------------------------------------------------
// 並列解析エンジン
//
// worker は「ファイルを読んで結果を返す」だけに徹し、DB には一切触れない。
// 単一の writer が結果を受け取り、`commit_in_chunks` でまとめて書く。
// **ファイル I/O 中に SQLite のトランザクションを保持しない**のがこの分離の
// 目的で、そうしないと遅い1枚がデータベース全体を握り続ける。
// ---------------------------------------------------------------------------

/// worker が1枚について返すもの。
#[derive(Clone, Debug)]
pub(crate) struct PhotoWork {
    /// 投入した順序。writer が重複と欠落を検出するのに使う。
    pub(crate) index: usize,
    pub(crate) photo_id: String,
    pub(crate) captured_at: Option<i64>,
    pub(crate) timestamp_source: Option<TimestampSource>,
    /// 中身が画像ではなかった。行を `is_missing=1` にして数から外す。
    pub(crate) not_image: bool,
    pub(crate) d_hash: Option<String>,
    pub(crate) thumbnail_path: Option<String>,
    pub(crate) thumbnail_source: Option<&'static str>,
    pub(crate) fingerprint: Option<(i64, i64)>,
    /// DB に書き戻す必要が無かった（ハッシュもサムネイルも据え置き）。
    pub(crate) hash_reused: bool,
    /// デコード失敗・非対応形式・権限エラー・timeout。
    /// **値が入っていても解析は続く。**
    pub(crate) error: Option<String>,
    /// `error` が「非対応」（読めたのに復号できない。原本が変わるまで再試行しない）か（U58）。
    /// false（既定）は一時的。**迷ったら false。**
    pub(crate) error_unsupported: bool,
    pub(crate) duration_ms: u64,
}

impl PhotoWork {
    pub(crate) fn new(index: usize, photo_id: &str) -> Self {
        Self {
            index,
            photo_id: photo_id.to_owned(),
            captured_at: None,
            timestamp_source: None,
            not_image: false,
            d_hash: None,
            thumbnail_path: None,
            thumbnail_source: None,
            fingerprint: None,
            hash_reused: false,
            error: None,
            error_unsupported: false,
            duration_ms: 0,
        }
    }

    /// 失敗の理由を記録する（文言と種類は `PhotoFailure` が決める）。
    pub(crate) fn fail(&mut self, failure: PhotoFailure) {
        self.error = Some(failure.message());
        self.error_unsupported = failure.kind() == ERROR_KIND_UNSUPPORTED;
    }

    /// `analysis_error_kind` 列に書く値。失敗が無ければ NULL。
    pub(crate) fn error_kind(&self) -> Option<&'static str> {
        self.error.as_ref().map(|_| {
            if self.error_unsupported {
                ERROR_KIND_UNSUPPORTED
            } else {
                ERROR_KIND_TRANSIENT
            }
        })
    }
}

pub(crate) const ERROR_KIND_UNSUPPORTED: &str = "unsupported";
pub(crate) const ERROR_KIND_TRANSIENT: &str = "transient";

/// 解析・表示用画像の失敗の理由（R2）。**画面に出す文言（`message`）と DB に書く種類（`kind`）は
/// ここだけが決める。** 呼ぶ側は `PhotoWork::fail` に理由を渡すだけ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PhotoFailure {
    /// 撮影時刻が読めなかった。
    NoCaptureTime,
    /// ファイルを開けない・読みの途中で切れた・サムネイルを作れない・理由が分からない。一時的。
    Unreadable,
    /// 全部読めたのに復号できない（壊れている・非対応の形式）。原本が変わらない限り同じ＝非対応。
    Undecodable,
    /// Amazon から取った画像を復号できない。文言は `Undecodable` と同じだが、網の側の
    /// 不調かもしれないので一時的のまま（U58 以前からの挙動）。
    RemoteUndecodable,
    /// Amazon から画像を取れなかった。理由の文言は取得側が作る。
    Fetch(String),
    /// 1 枚が時間内に終わらなかった（秒）。
    Timeout(u64),
    /// 動かせる worker が残らず、解析されないまま確定した。
    Stalled,
    /// 表示用の画像を作れなかった。
    DisplayFailed,
}

impl PhotoFailure {
    /// 復号の失敗（U58）を、ファイルの状態も見て解析の失敗にする。
    /// 非対応にするのは「全部読めたのに復号できない」ときだけ（`readable`＝fingerprint が取れた）。
    pub(crate) fn from_decode(failure: Option<DecodeFailure>, readable: bool) -> Self {
        if readable && failure == Some(DecodeFailure::Undecodable) {
            Self::Undecodable
        } else {
            Self::Unreadable
        }
    }

    /// 画面と `analysis_error` 列に出す文言。
    pub(crate) fn message(&self) -> String {
        match self {
            Self::NoCaptureTime => "撮影時刻を読み取れませんでした。".into(),
            Self::Unreadable => "ファイルを開けませんでした（移動・削除・権限）。".into(),
            Self::Undecodable | Self::RemoteUndecodable => "画像を読み取れませんでした（破損または非対応の形式）。".into(),
            Self::Fetch(message) => message.clone(),
            Self::Timeout(secs) => format!("解析が {secs} 秒以内に終わりませんでした。"),
            Self::Stalled => "読み込みが応答しないため、解析できませんでした。".into(),
            Self::DisplayFailed => "表示用の画像を作れませんでした。".into(),
        }
    }

    /// `analysis_error_kind` 列に書く値。
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Undecodable => ERROR_KIND_UNSUPPORTED,
            _ => ERROR_KIND_TRANSIENT,
        }
    }
}

/// 並列に流せる仕事。timeout した1枚を writer が単独で確定させるために、
/// 仕事そのものから写真の id を取れる必要がある。
pub(crate) trait PhotoJob: Send + Sync + 'static {
    fn photo_id(&self) -> &str;
}

/// 撮影時刻をまだ読んでいない1枚。
#[derive(Clone, Debug)]
pub(crate) struct MetadataJob {
    pub(crate) id: String,
    pub(crate) path: String,
}

impl PhotoJob for MetadataJob {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

impl PhotoJob for HashRecord {
    fn photo_id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Default)]
pub(crate) struct ParallelOutcome {
    /// writer が確定させた件数（timeout を含む）。
    pub(crate) completed: usize,
    pub(crate) cancelled: bool,
    pub(crate) timed_out: usize,
}

/// `items` を高々 `workers` 本のスレッドで処理し、結果を到着順に `on_result` へ渡す。
///
/// - キャンセルは `WATCHDOG_TICK_MS` ごとに見るので、200ms 程度で反応する。
/// - 1枚が `timeout` を超えたら、その1枚を失敗として確定させて先へ進む。
///   Rust では走っているデコードを安全に中断できないため、張り付いた worker は
///   放置する（`Arc` で共有しているので生き残っても安全）。**残りの worker と
///   writer は止まらない**のがここでの要件。
/// - そのため worker は join しない。join すると、まさに避けたかった
///   「遅い1枚に全体が引きずられる」状態に戻ってしまう。
/// - timeout で見捨てた worker は戻ってこないかもしれないので、1 本見捨てるたびに
///   代わりを 1 本足す（最大で `workers` 本まで。全体で `workers * 2` 本）。代わりも含めて
///   全員が固まって上限に達したら、まだ取られていない写真を全部失敗として確定して戻る
///   （永久に待たない）。正常なとき（timeout が起きないとき）は何も変わらない。
pub(crate) fn run_in_parallel<T, F>(
    items: Arc<Vec<T>>,
    workers: usize,
    timeout: Duration,
    is_cancelled: &dyn Fn() -> bool,
    work: F,
    on_result: &mut dyn FnMut(PhotoWork) -> Result<(), String>,
) -> Result<ParallelOutcome, String>
where
    T: PhotoJob,
    F: Fn(usize, &T) -> PhotoWork + Send + Sync + 'static,
{
    let total = items.len();
    if total == 0 {
        return Ok(ParallelOutcome::default());
    }
    // 上限は呼び出し側が決める（ローカル・NAS は `analysis_worker_count*` が
    // MAX_ANALYSIS_WORKERS に丸め済み。Amazon は amazon::WORKERS を使う）。
    let workers = workers.max(1).min(total);

    // worker 側はこのフラグだけを見る。`is_cancelled` は Tauri の State を
    // 借りていて 'static にできないため、writer が毎ティック転記する。
    let stop = Arc::new(AtomicBool::new(false));
    let cursor = Arc::new(AtomicUsize::new(0));
    // 見捨てた worker の代わりのぶんも含めて、枠を先に用意する。
    let max_slots = workers * 2;
    let in_flight: Arc<Vec<Mutex<Option<(usize, Instant)>>>> =
        Arc::new((0..max_slots).map(|_| Mutex::new(None)).collect());
    let work = Arc::new(work);
    let (sender, receiver) = mpsc::channel::<PhotoWork>();

    let spawn_worker = |slot: usize| {
        let items = items.clone();
        let cursor = cursor.clone();
        let in_flight = in_flight.clone();
        let work = work.clone();
        let stop = stop.clone();
        let sender = sender.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let index = cursor.fetch_add(1, Ordering::SeqCst);
                let Some(item) = items.get(index) else { break };
                if let Ok(mut cell) = in_flight[slot].lock() {
                    *cell = Some((index, Instant::now()));
                }
                let started = Instant::now();
                let mut result = work(index, item);
                result.duration_ms = started.elapsed().as_millis() as u64;
                if let Ok(mut cell) = in_flight[slot].lock() {
                    *cell = None;
                }
                if sender.send(result).is_err() {
                    break;
                }
            }
        });
    };
    for slot in 0..workers {
        spawn_worker(slot);
    }
    let mut spawned = workers;
    // 見捨てて、代わりを足した枠。
    let mut abandoned_slots = vec![false; max_slots];

    let mut reported = vec![false; total];
    let mut outcome = ParallelOutcome::default();
    let tick = Duration::from_millis(WATCHDOG_TICK_MS);
    while outcome.completed < total {
        if is_cancelled() {
            stop.store(true, Ordering::Relaxed);
            outcome.cancelled = true;
            return Ok(outcome);
        }
        match receiver.recv_timeout(tick) {
            Ok(result) => {
                // timeout として先に確定させた1枚が遅れて届くことがある。
                // 二重に数えない。
                if !reported[result.index] {
                    reported[result.index] = true;
                    outcome.completed += 1;
                    on_result(result)?;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        for (slot, cell) in in_flight.iter().enumerate() {
            let Some((index, started)) = cell.lock().ok().and_then(|cell| *cell) else {
                continue;
            };
            if started.elapsed() < timeout {
                continue;
            }
            if reported[index] {
                // 前の tick で timeout にした 1 枚。代わりをまだ足していなければ足す。
                if !abandoned_slots[slot] && spawned < max_slots {
                    abandoned_slots[slot] = true;
                    spawn_worker(spawned);
                    spawned += 1;
                }
                continue;
            }
            reported[index] = true;
            outcome.completed += 1;
            outcome.timed_out += 1;
            let mut result = PhotoWork::new(index, items[index].photo_id());
            result.duration_ms = started.elapsed().as_millis() as u64;
            result.fail(PhotoFailure::Timeout(timeout.as_secs()));
            on_result(result)?;
        }
        // 代わりも含めて、動かせる worker が 1 本も残っていない（全員が timeout 済みの
        // 1 枚から戻らず、足せる枠も無い）。まだ誰にも取られていない写真は、もう
        // 処理されないので、失敗として確定して戻る。
        let all_stuck = spawned == max_slots
            && in_flight.iter().all(|cell| {
                cell.lock()
                    .ok()
                    .and_then(|cell| *cell)
                    .is_some_and(|(index, _)| reported[index])
            });
        if all_stuck && outcome.completed < total {
            stop.store(true, Ordering::Relaxed);
            for index in 0..total {
                if reported[index] {
                    continue;
                }
                reported[index] = true;
                outcome.completed += 1;
                outcome.timed_out += 1;
                let mut result = PhotoWork::new(index, items[index].photo_id());
                result.fail(PhotoFailure::Stalled);
                on_result(result)?;
            }
            return Ok(outcome);
        }
    }
    stop.store(true, Ordering::Relaxed);
    Ok(outcome)
}

/// 溜まった結果をまとめて書く。
///
/// ここでキャンセルを見ないのは意図的。残っているのは最大
/// `ANALYSIS_CHUNK_SIZE` 件の UPDATE だけ（実測 0.1 ms/行）で、
/// 途中で投げ出すと読み終わった結果を捨てることになる。
pub(crate) fn flush_results(
    conn: &Connection,
    pending: &mut Vec<PhotoWork>,
    apply: &dyn Fn(&Connection, &PhotoWork) -> Result<(), String>,
) -> Result<usize, String> {
    if pending.is_empty() {
        return Ok(0);
    }
    let outcome = commit_in_chunks(
        conn,
        pending,
        &|| false,
        &mut |tx: &Connection, item: &PhotoWork, _index: usize| apply(tx, item),
    )?;
    pending.clear();
    Ok(outcome.committed)
}
