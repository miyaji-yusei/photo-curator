package app.photocurator.next

import android.content.Context
import android.util.Log
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/**
 * 出所ごとの控え（顔ぶれ [Listing]・ハッシュ値 [Fingerprints]・つまずき [Trouble]）のファイル名と、
 * 古い名前からの引き継ぎ（B2）。
 *
 * **名前は出所の鍵の [CacheName.of]（SHA-256 の 32 桁）。** 以前は鍵の英数字以外を `_` に潰した
 * 名前で、同じ NAS の `写真/旅行` と `写真/家族` のように**文字数が同じ日本語のフォルダ 2 つが
 * 同じファイルを使っていた**（一覧・ハッシュ値が混ざる。サイドカーの取り込みの守り `photoKeys` も
 * 別のフォルダの顔ぶれで判断する）。[Failures] は最初から SHA-256 の名前。
 *
 * **古い名前のファイルは消さない**（この版では残す。片付けは後の版で）。以前、名前を変えたときに
 * 古い絵を消して全部作り直しになり、困ったため。新しい名前が無いときだけ、古いファイルが
 * **本当にこの出所のものと確かめられれば**新しい名前へ写して使う。確かめられなければ使わず、
 * いつもの「控えが無い」ときと同じく作り直す。
 *
 * 確かめ方（[adoptIn]）:
 * - 顔ぶれ: 中の 1 件ずつが出所を指しているか。NAS は NAS の id とフォルダの道筋（「以下ぜんぶ」で
 *   なければ直下だけ）、Amazon は共有リンクの鍵。端末のアルバム（中に出所の手がかりが無い）は、
 *   古い名前を共有しうる出所が無いことを、鍵の形か端末のプロジェクト一覧で確かめる。空の一覧は使わない。
 * - ハッシュ値: 鍵の形から古い名前を共有しえないならそのまま。そうでなければ、この出所の（新しい名前の）
 *   顔ぶれと 1 件以上重なり、重なる分の大きさがすべて一致すること。
 * - つまずき: 中に手がかりが無いので、鍵の形か端末のプロジェクト一覧で、古い名前を共有しないこと。
 *
 * 一度決めたら（引き継いでも、使わなくても）印を残して、もう古いファイルを見ない
 * （引き継いだ控えを消したあとに、古いファイルからよみがえらせないため）。
 */
object SourceFiles {
    private const val TAG = "SourceFiles"

    /** 古い名前の作り方（**変えない**。古いファイルを探すのに使う）。 */
    private val UNSAFE = Regex("[^A-Za-z0-9_-]")

    /** 潰されない文字だけで、`_` も含まない鍵。古い名前がほかの鍵と重なりえない。 */
    private val ONLY_MINE = Regex("^[A-Za-z0-9-]+$")

    enum class Kind(val prefix: String, val suffix: String) {
        Listing("listing-", ".json"),
        Fingerprints("fingerprints-", ".json"),
        Trouble("trouble-", ".txt")
    }

    /** 引き継ぎの結果（テスト・ログ用）。 */
    enum class Outcome {
        /** 古いファイルを新しい名前へ写した。 */
        Adopted,

        /** 古いファイルはあったが、この出所のものと確かめられなかった（使わない。消さない）。 */
        Rejected,

        /** 新しい名前がすでにある・古いファイルが無い。 */
        Nothing
    }

    fun current(dir: File, kind: Kind, key: String) = File(dir, kind.prefix + CacheName.of(key) + kind.suffix)

    fun legacy(dir: File, kind: Kind, key: String) = File(dir, kind.prefix + legacyPart(key) + kind.suffix)

    /** 「この出所は引き継ぎを決めた」印。 */
    internal fun marker(dir: File, key: String) = File(dir, "source-names-v2-" + CacheName.of(key))

    internal fun legacyPart(key: String) = key.replace(UNSAFE, "_")

    /**
     * 古い名前を共有しうる鍵が**ありえない**か。潰されない文字だけで `_` も無ければ、
     * ほかの鍵を潰した名前（潰した所が `_` になる）と同じになることはない。端末のアルバム（bucket id）はこれ。
     */
    internal fun legacyOnlyMine(key: String) = ONLY_MINE.matches(key)

    /** 端末が知っている出所のうち、ほかに同じ古い名前になるものがあるか。 */
    internal fun legacyShared(key: String, known: Collection<String>): Boolean {
        val mine = legacyPart(key)
        return known.any { it != key && legacyPart(it) == mine }
    }

    /** 古い名前を共有しないと言えるか（鍵の形から、だめなら端末のプロジェクト一覧から）。 */
    private suspend fun alone(key: String, known: suspend () -> Collection<String>): Boolean =
        legacyOnlyMine(key) || !legacyShared(key, known())

    /** 区切りを / に揃え、前後の / を落とす。 */
    private fun slashed(path: String) = path.replace('\\', '/').trim('/')

    /** NAS の道筋がそのフォルダの写真か。「以下ぜんぶ」でなければ直下だけ。 */
    internal fun insideFolder(path: String, folder: String, deep: Boolean): Boolean {
        val p = slashed(path)
        val f = slashed(folder)
        val rest = when {
            f.isEmpty() -> p
            p.startsWith("$f/") -> p.substring(f.length + 1)
            else -> return false
        }
        return rest.isNotEmpty() && (deep || !rest.contains('/'))
    }

    /** 古い顔ぶれがこの出所のものか。 */
    internal suspend fun listingBelongs(
        key: String,
        photos: List<Photo>,
        known: suspend () -> Collection<String>
    ): Boolean {
        // 空の一覧は使わない（作り直しても網の往復 1 回。空で始めると写真が無いように見える）。
        if (photos.isEmpty()) return false
        return when {
            photos.all { it.remote is SmbRef } -> {
                val nasId = key.substringBefore("|")
                val deep = key.endsWith("|**")
                val folder = key.removeSuffix("|**").substringAfter("|", "\u0000")
                if (folder == "\u0000") return false
                photos.all { photo ->
                    val ref = photo.remote as SmbRef
                    ref.nasId == nasId &&
                        insideFolder(ref.path, folder, deep) &&
                        photo.relativePath == ref.path.replace('\\', '/')
                }
            }
            photos.all { it.remote is AmazonRef } -> photos.all { (it.remote as AmazonRef).shareKey == key }
            // 端末のアルバム。中に出所の手がかりが無い。
            photos.all { it.remote == null } -> alone(key, known)
            else -> false
        }
    }

    /** 古いハッシュ値がこの出所のものか。[listing] はこの出所の（新しい名前の）顔ぶれ。 */
    internal fun fingerprintsBelong(
        key: String,
        listing: List<Photo>?,
        prints: Map<String, Fingerprint>
    ): Boolean {
        if (legacyOnlyMine(key)) return true
        if (listing.isNullOrEmpty() || prints.isEmpty()) return false
        val sizes = HashMap<String, Long>(listing.size)
        for (photo in listing) sizes[photo.relativePath] = photo.size
        var overlap = 0
        for ((path, print) in prints) {
            val size = sizes[path] ?: continue
            if (size != print.size) return false
            overlap += 1
        }
        return overlap > 0
    }

    /** 印に書いてある「決めた」種類。印が無ければ空。 */
    internal fun decided(dir: File, key: String): Set<Kind> {
        val file = marker(dir, key)
        if (!file.exists()) return emptySet()
        return file.readLines().mapNotNull { line -> Kind.entries.firstOrNull { it.name == line.trim() } }.toSet()
    }

    /**
     * [dir] の中で、[key] の出所の古い名前の控えを引き継ぐ。**古いファイルは消さない。**
     *
     * 種類ごとに 1 回だけ決め、決めた種類を印（[marker]）に書く。決めた種類はもう見ない。
     * ハッシュ値だけは、確かめるのに要る新しい名前の顔ぶれがまだ無ければ**決めずに待つ**
     * （作り直した顔ぶれが書かれたあとに確かめる。待たずに捨てると NAS の全部を読み直すことになる）。
     *
     * [writing] はこれから書く（消す）種類と、この起動中にすでに書いた種類。**書く前に決めてしまう**（その種類は引き継がない）。
     * 書いたあとに古いファイルで上書きしないため。
     *
     * 読み書きに失敗したら例外のまま（その回の決定は印に残さず、次にまた試す）。
     * [known] は端末が知っている出所の鍵（プロジェクト一覧）。要るときだけ 1 回呼ぶ。
     * 返すのは、この回に決めた種類とその結果。
     */
    internal suspend fun adoptIn(
        dir: File,
        key: String,
        writing: Set<Kind> = emptySet(),
        known: suspend () -> Collection<String>
    ): Map<Kind, Outcome> {
        val before = decided(dir, key)
        if (before.size == Kind.entries.size) return emptyMap()
        var cached: Collection<String>? = null
        val knownOnce: suspend () -> Collection<String> = { cached ?: known().also { cached = it } }

        val out = LinkedHashMap<Kind, Outcome>()
        for (kind in Kind.entries) {
            if (kind in before) continue
            if (kind in writing) {
                out[kind] = Outcome.Nothing
                continue
            }
            val outcome = when (kind) {
                Kind.Listing -> adoptOne(dir, kind, key) { text ->
                    listingBelongs(key, Listing.parse(text), knownOnce)
                }
                Kind.Fingerprints -> {
                    val listingFile = current(dir, Kind.Listing, key)
                    val waiting = !legacyOnlyMine(key) && !listingFile.exists() &&
                        legacy(dir, kind, key).exists() && !current(dir, kind, key).exists()
                    if (waiting) null else adoptOne(dir, kind, key) { text ->
                        if (legacyOnlyMine(key)) true else {
                            val listing = try {
                                Listing.parse(listingFile.readText())
                            } catch (error: Exception) {
                                null
                            }
                            fingerprintsBelong(key, listing, Fingerprints.parse(text))
                        }
                    }
                }
                Kind.Trouble -> adoptOne(dir, kind, key) { text ->
                    text.isNotBlank() && alone(key, knownOnce)
                }
            }
            if (outcome != null) out[kind] = outcome
        }
        if (out.isNotEmpty()) {
            val now = before + out.keys
            marker(dir, key).writeAtomically { file ->
                file.writeText(Kind.entries.filter { it in now }.joinToString("\n") { it.name })
            }
        }
        return out
    }

    private suspend fun adoptOne(
        dir: File,
        kind: Kind,
        key: String,
        belongs: suspend (String) -> Boolean
    ): Outcome {
        val now = current(dir, kind, key)
        val old = legacy(dir, kind, key)
        if (now.exists() || !old.exists() || now.name == old.name) return Outcome.Nothing
        val bytes = old.readBytes()
        val ok = try {
            belongs(String(bytes, Charsets.UTF_8))
        } catch (error: kotlinx.coroutines.CancellationException) {
            throw error
        } catch (error: Exception) {
            // 中身が読めない古い控えは使わない（作り直す）。
            false
        }
        if (!ok) return Outcome.Rejected
        // **写すだけ。古いファイルはそのまま残す。**
        now.writeAtomically { it.writeBytes(bytes) }
        return Outcome.Adopted
    }

    /** 出所ごとの順番待ち。**引き継ぎと、その出所の控えへの書き込みの始まりを 1 本に並べる。** */
    private val locks = ConcurrentHashMap<String, Mutex>()

    /** この起動中に書いた（書こうとした）「種類:鍵」。印を書けなかったときも、書いた種類は引き継がない。 */
    private val written: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /** 全部の種類を決め終えた出所（もう印も見ない）。 */
    private val settled: MutableSet<String> = ConcurrentHashMap.newKeySet()

    /**
     * 控えを読み書きする前に呼ぶ。**新しい名前の控えに触る前に、古い名前からの引き継ぎを済ませる**
     * （先に保存が走ると、引き継げたはずのハッシュ値を作り直すことになる）。
     * 書く（消す）ときは [writing] にその種類を渡す（その種類は引き継がない）。
     * 失敗しても止めない（控えが無いときと同じく作り直す）。
     */
    suspend fun adopt(context: Context, key: String, writing: Kind? = null) {
        if (writing != null) written += writing.name + ":" + key
        if (key in settled) return
        val lock = locks.computeIfAbsent(key) { Mutex() }
        lock.withLock {
            if (key in settled) return
            try {
                withContext(Dispatchers.IO) {
                    val app = context.applicationContext ?: context
                    val dir = app.filesDir
                    val wrote = Kind.entries.filter { (it.name + ":" + key) in written }.toSet()
                    val outcome = adoptIn(dir, key, wrote) { Projects.all(app).map { it.source.key } }
                    if (outcome.values.any { it != Outcome.Nothing }) {
                        Log.i(TAG, "古い名前の控え（$key）: $outcome（古いファイルは残す）")
                    }
                    if (decided(dir, key).size == Kind.entries.size) settled += key
                }
            } catch (error: kotlinx.coroutines.CancellationException) {
                throw error
            } catch (error: Exception) {
                Log.w(TAG, "古い名前の控えを引き継げなかった: $key", error)
            }
        }
    }
}
