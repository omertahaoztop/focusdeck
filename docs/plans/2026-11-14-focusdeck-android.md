# focusdeck Android uygulaması: uygulama planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pixel 10 Pro Fold'da focusdeck panosu: kapalı ekranda sekmeli liste, açık ekranda ızgara, not ekleme, bitirme, taşıma, sıralama ve hatırlatıcı bildirimi, hepsi `https://deck.omertahaoztop.tr` REST API'siyle.

**Architecture:** Tek modüllü Android uygulaması. Ağ katmanı (`api/`) OkHttp + kotlinx.serialization ile sunucunun API sürüm 1'ine birebir uyar ve her sonucu `ApiResult` olarak döndürür. Mantık saf fonksiyonlarda ve `ViewModel`'lerde (`board/`, `capture/`, `reminders/`), ekranlar Compose'da (`ui/`). Bağımlılıklar `AppGraph`'ta elle kurulur.

**Tech Stack:** Kotlin, Jetpack Compose, Material 3 + Material 3 adaptive, Jetpack WindowManager, OkHttp, kotlinx.serialization, kotlinx-coroutines, WorkManager, DataStore. Testte JUnit 4, MockWebServer, kotlinx-coroutines-test, Robolectric, Compose UI test.

**Spec:** `~/Desktop/personal/focusdeck/docs/superpowers/specs/2026-11-14-focusdeck-android-design.md`

## Global Constraints

- Repo `github.com/omertahaoztop/focusdeck-android`, yerelde `~/Desktop/personal/focusdeck-android`, public, MIT.
- `applicationId` ve paket `tr.omertahaoztop.focusdeck`. `minSdk 26`, `compileSdk 36`, `targetSdk 36`. `versionName "0.1.0"`, `versionCode 1`.
- Bağımlılık sınırı: DI çerçevesi, Retrofit, Room yok. Yeni kütüphane eklemek planı durdurur ve sorulur.
- Kütüphane ve AGP sürümleri `gradle/libs.versions.toml`'da, yazıldığı günün en güncel kararlı sürümleri. Yerel araçlar: JDK 21, Gradle 9.8.2 (wrapper), Kotlin 2.4.10, SDK `/opt/android-sdk`.
- API: base `https://<sunucu>/api/v1`, `Authorization: Bearer <token>`. JSON alan adları `snake_case`, bilinmeyen alanlar yok sayılır (`ignoreUnknownKeys = true`, `explicitNulls = false`).
- Zaman aşımı: bağlantı 10 sn, istek (call) 60 sn.
- `done` ve `reorder` kendiliğinden tekrar gönderilmez. `drops` ve `captures` `Idempotency-Key` ile gider, tekrar aynı anahtarla.
- Token, `Authorization` başlığı ve istek gövdeleri hiçbir zaman loglanmaz. Bildirimlerde ve hata metinlerinde token yok.
- Release sürümü düz HTTP'ye izin vermez. Debug sürümü yalnız `localhost`, `127.0.0.1`, `[::1]`, `10.0.2.2` için `http://` kabul eder.
- `android:allowBackup="false"`.
- Her kullanıcı metni `values/strings.xml` (İngilizce) ve `values-tr/strings.xml` (Türkçe) içinde, iki dosyada aynı anahtarlar. Türkçe metinde doğru harfler (ç ğ ı İ ö ş ü).
- Commit: İngilizce, imperatif, tek satır, en çok 72 karakter, gövde yok. Dosyalar yolla stage edilir.
- Alt ajanlar yalnız görevlerinde yazılı komutları çalıştırır. Emülatör, imza anahtarı, GitHub ve telefon adımları (Task 11, 12) ana ajanındır.

## Review Focus

- Taşınan öğe bütün alanlarıyla gitmeli: `drops` gövdesindeki `item`, listeden gelen öğenin aynısı (`due` ofseti dahil, `labels`, `links`, `from`, `description`). Test: Task 2 `itemRoundTripKeepsEveryField`.
- `refresh` 0 ya da çok küçük gelen kutu sıcak döngüye girmemeli. Yoklama aralığı en az 5 sn. Test: Task 6 `zeroRefreshPollsEveryFiveSeconds`.
- Başlıkta birden çok ayraç (`"Mono · Redmine · Açık"`): grup ilk ayraçtan önceki kısım, ad gerisi. Ayraçsız ya da boş önekli başlık "Diğer"e düşer. Test: Task 5 `splitTitleUsesFirstSeparator`.
- Yapıştırılan token başında ya da sonunda boşluk ya da satır sonu taşır. Kaydetmeden önce kırpılır, boş token kabul edilmez. Test: Task 3 `saveConnectionTrimsToken`.
- Hatırlatıcı `at` değeri farklı ofsetle gelebilir (`+00:00`, `+03:00`). Karşılaştırma anlık zamanla yapılır, anahtar sunucunun metniyle. Test: Task 7 `offsetsCompareAsInstants`.

## Dosya yapısı

Kök `app/src/main/java/tr/omertahaoztop/focusdeck/`, testler `app/src/test/java/tr/omertahaoztop/focusdeck/` altında aynı yolda.

| Dosya | Sorumluluk | Görev |
|---|---|---|
| `settings.gradle.kts`, `build.gradle.kts`, `app/build.gradle.kts`, `gradle/libs.versions.toml`, `gradlew*` | derleme | 1, 12 |
| `app/src/main/AndroidManifest.xml` | izinler, alıcılar | 1, 4, 7 |
| `app/src/debug/res/xml/network_security_config.xml` | debug düz HTTP | 1 |
| `.github/workflows/ci.yml`, `.github/workflows/release.yml` | CI | 1, 12 |
| `api/Types.kt`, `api/ApiResult.kt`, `api/FocusdeckApi.kt`, `api/HttpFocusdeckApi.kt`, `api/ServerUrl.kt` | API | 2 |
| `settings/TokenCipher.kt`, `settings/SettingsStore.kt` | ayarlar, token | 3 |
| `capture/CaptureQueue.kt`, `capture/CaptureSender.kt`, `capture/CaptureWorker.kt` | not kuyruğu | 4 |
| `board/Grouping.kt`, `board/Grid.kt`, `board/DueText.kt` | saf düzen hesabı | 5 |
| `board/BoardViewModel.kt`, `board/DoneVerdict.kt` | pano durumu, eylemler | 6 |
| `reminders/ReminderPlan.kt`, `reminders/ReminderStore.kt`, `reminders/ReminderSync.kt`, `reminders/AndroidAlarms.kt`, `reminders/Receivers.kt`, `reminders/ReminderWorker.kt` | hatırlatıcı | 7 |
| `App.kt` (`FocusdeckApp`, `AppGraph`), `MainActivity.kt`, `ui/setup/*`, `ui/settings/*`, `ui/ErrorText.kt` | kurulum, gezinme | 8 |
| `ui/board/ListBoard.kt`, `ui/board/GridBoard.kt`, `ui/board/ItemRow.kt`, `ui/board/BoardScreen.kt`, `ui/detail/ItemDetail.kt` | pano ekranları | 9 |
| `ui/actions/SwipeDone.kt`, `ui/actions/DragDrop.kt`, `ui/actions/MoveDialog.kt`, `ui/capture/CaptureSheet.kt`, `ui/capture/QueueLine.kt` | eylem arayüzü | 10 |
| `README.md`, `README.tr.md`, `LICENSE` | belgeler | 1, 12 |

## Görev sırası

Task 1 önce. Sonra 2 ve 3 paralel. 4, 5, 6, 7, 8 Task 2 ve 3'e dayanır: 5 paralel gidebilir, 4, 6, 7, 8 birbirinden bağımsız. 9 Task 5, 6, 8'e, 10 Task 4 ve 9'a dayanır. 11 ve 12 en son, ana ajanda.

---

### Task 1: Proje iskeleti ve CI

**Files:**
- Create: `settings.gradle.kts`, `build.gradle.kts`, `gradle.properties`, `gradle/libs.versions.toml`, Gradle wrapper (`gradle wrapper --gradle-version 9.8.2`), `app/build.gradle.kts`, `app/src/main/AndroidManifest.xml`, `app/src/main/java/tr/omertahaoztop/focusdeck/MainActivity.kt`, `app/src/main/res/values/strings.xml`, `app/src/main/res/values-tr/strings.xml`, `app/src/debug/res/xml/network_security_config.xml`, `app/src/debug/AndroidManifest.xml`, `.github/workflows/ci.yml`, `.gitignore`, `LICENSE` (MIT, "Ömer Taha Öztop", 2026), `README.md` (tek paragraf, Task 12'de genişler)
- Test: `app/src/test/java/tr/omertahaoztop/focusdeck/StringsParityTest.kt`

**Interfaces:**
- Produces: `BuildConfig.DEBUG`, `BuildConfig.VERSION_NAME`; `app_name` = "focusdeck" iki dilde; Compose, serialization, test bağımlılıkları katalogda (`libs.*`), `testOptions.unitTests.isIncludeAndroidResources = true`.

- [ ] **Step 1:** `git init -b main` ile `~/Desktop/personal/focusdeck-android` oluştur, dosyaları yaz. AGP 9 yerleşik Kotlin desteği getiriyorsa ayrı `kotlin-android` eklentisi uygulanmaz, getirmiyorsa uygulanır. Compose derleyici eklentisi Kotlin sürümüyle aynı.
- [ ] **Step 2: Test yaz.** `StringsParityTest.keysMatchInBothLanguages`: iki `strings.xml`'i XML olarak okur, `name` kümeleri eşit, boş değer yok.
- [ ] **Step 3:** Debug ağ ayarı: `cleartextTrafficPermitted="false"` taban, `domain-config cleartextTrafficPermitted="true"` içinde `localhost`, `127.0.0.1`, `::1`, `10.0.2.2`. Ana manifestte `allowBackup="false"`, `INTERNET` izni.
- [ ] **Step 4:** `ci.yml`: push ve pull_request; `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1`, `actions/setup-java` (temurin 21, SHA'ya sabit), `gradle/actions/setup-gradle` (SHA'ya sabit); `./gradlew lint testDebugUnitTest assembleDebug`. Sabitlenecek SHA'ları `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` ile al.
- [ ] **Step 5: Doğrula.** `./gradlew lint testDebugUnitTest assembleDebug` → BUILD SUCCESSFUL, `app/build/outputs/apk/debug/app-debug.apk` var.
- [ ] **Step 6: Commit.** `git add <dosyalar>; git commit -m "Scaffold focusdeck Android app with CI"`

### Task 2: API istemcisi

**Review:** per-task — security

**Files:**
- Create: `api/Types.kt`, `api/ApiResult.kt`, `api/FocusdeckApi.kt`, `api/HttpFocusdeckApi.kt`, `api/ServerUrl.kt`
- Test: `api/HttpFocusdeckApiTest.kt`, `api/TypesTest.kt`, `api/ServerUrlTest.kt`

**Interfaces:**
- Produces:

```kotlin
// Types.kt: @Serializable, alanlar sunucunun api.rs/types.rs tipleriyle aynı adlar
data class ProfilesResponse(val profiles: List<String>, val default: String? = null)
data class ProfileView(val name: String, @SerialName("capture_into") val captureInto: String? = null, val boxes: List<BoxView>)
data class BoxView(val id: String, val title: String, val plugin: String, val row: Int, val col: Int,
    val width: Int, val height: Int, val refresh: Long, val describe: Describe? = null, val error: String? = null)
data class Describe(val actions: List<String>, @SerialName("accepts_drop") val acceptsDrop: Boolean, val reminders: Boolean)
data class Source(val plugin: String, val id: String)
data class Link(val label: String, val url: String)
data class Item(val id: String, val title: String, val subtitle: String? = null, val url: String? = null,
    val due: String? = null, val overdue: Boolean = false, val source: Source? = null, val description: String? = null,
    val labels: List<String> = emptyList(), val from: String? = null, val links: List<Link> = emptyList())
data class ItemsResponse(val items: List<Item>, @SerialName("listed_at") val listedAt: String)
data class ActionRequest(val item: String, val action: String, val args: JsonObject = JsonObject(emptyMap()))
data class ActionResponse(val item: Item? = null)
data class DropRequest(val item: Item, val from: Source? = null)
data class DropResponse(val item: Item)
data class CaptureRequest(val text: String, val at: String, val lang: String)
data class CaptureResponse(@SerialName("box") val boxId: String, val item: Item)
data class BoxReminder(@SerialName("box") val boxId: String, val at: String, val item: Item)
data class RemindersResponse(val reminders: List<BoxReminder>)
val ApiJson: Json // ignoreUnknownKeys, explicitNulls = false, encodeDefaults = true

// ApiResult.kt
enum class ErrorCode { UNAUTHORIZED, NOT_FOUND, INVALID, UNSUPPORTED, KEY_REUSED, IN_PROGRESS, PLUGIN, INTERNAL }
enum class NetworkKind { DNS, CONNECT, TLS }
sealed interface ApiFailure {
    data class Server(val status: Int, val code: ErrorCode?, val message: String) : ApiFailure // code null: gövde API hatası değil (Cloudflare 403 gibi)
    data class Network(val kind: NetworkKind, val detail: String) : ApiFailure // istek sunucuya ulaşmadı
    data class Uncertain(val detail: String) : ApiFailure // istek gitti, cevap gelmedi (zaman aşımı, kopan bağlantı)
}
sealed interface ApiResult<out T> { data class Ok<T>(val value: T) : ApiResult<T>; data class Err(val failure: ApiFailure) : ApiResult<Nothing> }

// FocusdeckApi.kt
interface FocusdeckApi {
    suspend fun profiles(): ApiResult<ProfilesResponse>
    suspend fun profile(p: String): ApiResult<ProfileView>
    suspend fun items(p: String, box: String): ApiResult<ItemsResponse>
    suspend fun action(p: String, box: String, req: ActionRequest): ApiResult<ActionResponse>
    suspend fun drop(p: String, box: String, req: DropRequest, key: String): ApiResult<DropResponse>
    suspend fun capture(p: String, req: CaptureRequest, key: String): ApiResult<CaptureResponse>
    suspend fun reminders(p: String, since: OffsetDateTime): ApiResult<RemindersResponse>
}

// HttpFocusdeckApi.kt
class HttpFocusdeckApi(base: HttpUrl, token: String, client: OkHttpClient = defaultClient()) : FocusdeckApi
fun defaultClient(): OkHttpClient

// ServerUrl.kt
enum class UrlProblem { NOT_A_URL, HTTP_NOT_ALLOWED }
fun parseServerUrl(input: String, allowLocalHttp: Boolean): Result<HttpUrl> // hata: UrlProblemException(problem)
```

- [ ] **Step 1: Testleri yaz.**
  - `TypesTest.itemRoundTripKeepsEveryField`: 11 alanı dolu bir öğe JSON'u (`"due": "2026-11-14T17:00:00+03:00"`) decode edilip encode edilince JSON ağacı olarak aynı.
  - `TypesTest.unknownFieldsAreIgnored`: `capture_refs`, `capture_words`, `plan_into` taşıyan profil decode olur.
  - `HttpFocusdeckApiTest`: `sendsBearerToken` (başlık `Bearer t0k`); `mapsErrorBody` (422 `{"error":{"code":"KEY_REUSED","message":"m"}}` → `Server(422, KEY_REUSED, "m")`); `mapsNonApiBody` (403 HTML → `Server(403, null, …)`); `readTimeoutIsUncertain` (MockWebServer `SocketPolicy` ile cevapsız, kısa call timeout'lu client → `Uncertain`); `refusedConnectionIsNetwork` (kapalı port → `Network(CONNECT, …)`); `dropSendsIdempotencyKey`; `captureSendsIdempotencyKey`; `pathSegmentsAreEncoded` (`"a b"` profil → `/api/v1/profiles/a%20b`); `remindersSendsSince` (`since=2026-11-14T05:00:00+03:00`, URL kodlu); `reorderArgs` (gövde `{"item":"x","action":"reorder","args":{"position":2}}`).
  - `ServerUrlTest`: `https://deck.omertahaoztop.tr/` kabul; `http://deck.omertahaoztop.tr` her iki kipte `HTTP_NOT_ALLOWED`; `http://10.0.2.2:8740`, `http://[::1]:8740`, `http://localhost:8740` yalnız `allowLocalHttp = true` ile kabul; `deck` → `NOT_A_URL`; baştaki ve sondaki boşluk kırpılır.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.api.*'` → FAIL (sınıflar yok).
- [ ] **Step 3: Uygula.** İstekler `Dispatchers.IO`'da OkHttp `execute`. `UnknownHostException` → `Network(DNS)`, `ConnectException` ve bağlanma aşamasındaki `SocketTimeoutException` → `Network(CONNECT)`, `SSLException` → `Network(TLS)`, diğer `IOException` → `Uncertain`. 2xx dışı gövde `ApiJson` ile hata olarak çözülemezse `Server(status, null, "HTTP <status>")`, gövde metni mesaja konmaz. `defaultClient`: `connectTimeout(10 s)`, `callTimeout(60 s)`, `readTimeout(60 s)`, `HttpLoggingInterceptor` yok.
- [ ] **Step 4:** Aynı komut → PASS.
- [ ] **Step 5: Commit.** `git commit -m "Add focusdeck API client"`

### Task 3: Ayarlar ve şifreli token

**Review:** per-task — security, persisted format

**Files:**
- Create: `settings/TokenCipher.kt`, `settings/SettingsStore.kt`
- Test: `settings/SettingsStoreTest.kt`, `app/src/androidTest/java/tr/omertahaoztop/focusdeck/settings/KeystoreTokenCipherTest.kt`

**Interfaces:**
- Consumes: `HttpUrl` (OkHttp).
- Produces:

```kotlin
interface TokenCipher { fun encrypt(plain: String): ByteArray; fun decrypt(blob: ByteArray): String? } // anahtar yoksa ya da bozuksa null
class KeystoreTokenCipher(alias: String = "focusdeck-token") : TokenCipher // AndroidKeyStore, AES/GCM/NoPadding, 256 bit, blob = 12 bayt IV + şifreli metin
data class Connection(val url: HttpUrl, val token: String, val profile: String)
data class UiPrefs(val selectedTab: String?, val collapsed: Map<String, Boolean>, val notificationAsked: Boolean)
class SettingsStore(store: DataStore<Preferences>, cipher: TokenCipher) {
    val connection: Flow<Connection?> // token çözülemezse null
    val savedUrl: Flow<HttpUrl?>      // token silinse de kalır, kurulum ekranı doldurur
    suspend fun saveConnection(url: HttpUrl, token: String, profile: String)
    suspend fun clearToken()
    val ui: Flow<UiPrefs>
    suspend fun setSelectedTab(group: String)
    suspend fun setCollapsed(boxId: String, collapsed: Boolean)
    suspend fun setNotificationAsked()
}
```

- [ ] **Step 1: Testleri yaz** (`SettingsStoreTest`, geçici dosyalı `PreferenceDataStoreFactory`, sahte `TokenCipher` ters çevirip saklar): `saveConnectionTrimsToken` (`"  t0k\n"` → `connection.first().token == "t0k"`); `blankTokenIsRejected` (`"  "` → `IllegalArgumentException`); `tokenIsNotStoredInPlainText` (DataStore dosyasının baytlarında `t0k` geçmez); `undecryptableTokenGivesNoConnection` (cipher `null` dönerse `connection` null, `savedUrl` dolu); `clearTokenKeepsUrl`; `collapsedOverridesPersist`.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.settings.*'` → FAIL.
- [ ] **Step 3: Uygula.** Anahtarlar `url`, `token_enc` (Base64), `profile`, `tab`, `collapsed_<boxId>`, `notification_asked`. Keystore anahtarı ilk `encrypt`'te üretilir; `decrypt` her hata türünde `null` döner, hata metni loglanmaz.
- [ ] **Step 4:** Aynı komut → PASS. `KeystoreTokenCipherTest.roundTrip` (şifrele, çöz, eşit; alias silinince `null`) Task 11'de emülatörde koşar, burada yalnız derlenir: `./gradlew compileDebugAndroidTestKotlin` → BUILD SUCCESSFUL.
- [ ] **Step 5: Commit.** `git commit -m "Store server connection with Keystore-encrypted token"`

### Task 4: Not kuyruğu

**Review:** per-task — persisted format

**Files:**
- Create: `capture/CaptureQueue.kt`, `capture/CaptureSender.kt`, `capture/CaptureWorker.kt`
- Modify: `app/src/main/AndroidManifest.xml` (gerekirse WorkManager başlatıcısı varsayılan kalır)
- Test: `capture/CaptureQueueTest.kt`, `capture/CaptureSenderTest.kt`, `capture/CaptureWorkerTest.kt`

**Interfaces:**
- Consumes: `FocusdeckApi.capture`, `ApiResult`, `ApiFailure`, `ErrorCode`, `CaptureRequest`, `CaptureResponse` (Task 2).
- Produces:

```kotlin
@Serializable data class QueuedNote(val id: String, val text: String, val at: String, val lang: String, val failed: String? = null)
class CaptureQueue(store: DataStore<List<QueuedNote>>) {
    val notes: Flow<List<QueuedNote>>
    suspend fun add(text: String, at: OffsetDateTime, lang: String): QueuedNote // id = UUID.randomUUID()
    suspend fun remove(id: String)
    suspend fun markFailed(id: String, message: String)
}
fun captureQueueStore(context: Context): DataStore<List<QueuedNote>> // dosya "capture-queue.json"
enum class NoteFate { SENT, KEEP, FAILED, AUTH }
fun fateOf(result: ApiResult<CaptureResponse>): NoteFate
sealed interface SendOutcome {
    data class Sent(val response: CaptureResponse) : SendOutcome
    data object Queued : SendOutcome
    data class Rejected(val text: String, val message: String) : SendOutcome // ön planda kalıcı hata, kuyruktan çıktı
    data object AuthLost : SendOutcome
}
class CaptureSender(queue: CaptureQueue, api: suspend () -> Pair<FocusdeckApi, String>?) { // api + profil, bağlantı yoksa null
    suspend fun submit(text: String, now: OffsetDateTime, lang: String): SendOutcome // ön plan
    suspend fun drain(): Boolean // arka plan; true = KEEP kalan var, tekrar dene
}
fun deviceLang(locale: Locale = Locale.getDefault()): String // "tr" ya da "en"
class CaptureWorker(ctx: Context, params: WorkerParameters) : CoroutineWorker // drain true → Result.retry()
fun enqueueCaptureDrain(context: Context) // benzersiz "capture-drain", ExistingWorkPolicy.KEEP, NetworkType.CONNECTED, üstel geri çekilme
```

- [ ] **Step 1: Testleri yaz.**
  - `CaptureSenderTest.fateTable`: `Ok` → SENT; `Server` + INVALID, KEY_REUSED, UNSUPPORTED, NOT_FOUND → FAILED; `Server(401, UNAUTHORIZED)` → AUTH; IN_PROGRESS, PLUGIN, INTERNAL, `Server(503, null)`, `Server(403, null)`, `Network`, `Uncertain` → KEEP.
  - `retryUsesSameKey`: ilk `capture` `Uncertain`, `drain` ikinci kez aynı `key` ile çağırır.
  - `foregroundRejectRemovesNote`: `submit` + KEY_REUSED → `Rejected(text, message)`, kuyruk boş.
  - `backgroundRejectMarksFailed`: `drain` + UNSUPPORTED → not kuyrukta, `failed == message`, sonraki `drain` onu göndermez.
  - `authKeepsNote`: 401 → `AuthLost`, not kuyrukta.
  - `CaptureQueueTest.survivesReopen`: yaz, store'u yeniden aç, aynı liste; `atKeepsDeviceOffset` (`at` `+03:00` ile biter).
  - `CaptureWorkerTest.retriesWhileNotesRemain` (`TestListenableWorkerBuilder`, sahte gönderici).
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.capture.*'` → FAIL.
- [ ] **Step 3: Uygula.** `CaptureQueue` kotlinx.serialization tabanlı `Serializer<List<QueuedNote>>` ile. Bozuk dosyada `ReplaceFileCorruptionHandler` önce bozuk dosyayı `capture-queue.json.corrupt-<epoch saniye>` olarak kopyalar, sonra boş listeyle devam eder (not metni elle kurtarılabilsin). `at` `OffsetDateTime.truncatedTo(SECONDS)` ile RFC 3339. `drain` yalnız `failed == null` notları gönderir.
- [ ] **Step 4:** Aynı komut → PASS.
- [ ] **Step 5: Commit.** `git commit -m "Queue captured notes with idempotent retry"`

### Task 5: Düzen hesabı

**Files:**
- Create: `board/Grouping.kt`, `board/Grid.kt`, `board/DueText.kt`
- Test: `board/GroupingTest.kt`, `board/GridTest.kt`, `board/DueTextTest.kt`

**Interfaces:**
- Consumes: `BoxView`, `Item` (Task 2).
- Produces:

```kotlin
const val TITLE_SEPARATOR = " \u00B7 "
fun splitTitle(title: String): Pair<String?, String> // (grup, ad)
data class Section(val box: BoxView, val name: String)
data class Tab(val group: String?, val sections: List<Section>) // group null = "Diğer"
fun groupTabs(boxes: List<BoxView>): List<Tab> // tek grup çıkarsa yine tek eleman; sekme çubuğunu UI gizler
sealed interface ListRow {
    data class Box(val section: Section, val collapsed: Boolean) : ListRow
    data class EmptyRun(val sections: List<Section>) : ListRow // art arda boş ve katlı kutular
}
fun listRows(sections: List<Section>, counts: Map<String, Int>, overrides: Map<String, Boolean>): List<ListRow>
data class GridSpec(val columns: Int, val rows: Int, val emptyColumns: Set<Int>)
fun gridSpec(boxes: List<BoxView>): GridSpec
fun useGrid(widthDp: Float, heightDp: Float): Boolean // width >= 600 && height >= 480
fun rowHeightDp(availableDp: Float, rows: Int): Float // max(availableDp / rows, 112f)
fun shortDue(due: String, now: ZonedDateTime, locale: Locale): String
```

- [ ] **Step 1: Testleri yaz.**
  - `splitTitleUsesFirstSeparator`: `"Mono · Redmine · Açık"` → `("Mono", "Redmine · Açık")`; `"Gmail"` → `(null, "Gmail")`; `" · Gmail"` → `(null, " · Gmail")`; `"Mono·Gmail"` → `(null, "Mono·Gmail")`.
  - `groupTabsKeepsProfileOrder`: başlıklar `Mono · A`, `Kişisel · B`, `Mono · C`, `D` → sekmeler `Mono[A, C]`, `Kişisel[B]`, `null[D]`.
  - `emptyBoxesMergeWhenCollapsed`: sayılar `A=0, B=0, C=3, D=0` → `EmptyRun[A,B]`, `Box(C, false)`, `EmptyRun[D]`; `A` için `overrides[A]=false` → `Box(A, false)`, `EmptyRun[B]`, …; dolu kutu `overrides=true` → `Box(C, true)`.
  - `GridTest`: elle yazılmış kutular `A(row 0, col 0, w 1, h 2)`, `B(row 0, col 2, w 1, h 1)`, `C(row 1, col 2, w 2, h 1)` → `GridSpec(columns = 4, rows = 2, emptyColumns = {1})`; `useGrid(600f, 480f)` true, `useGrid(599f, 900f)` false, `useGrid(900f, 479f)` false; `rowHeightDp(300f, 4) == 112f`, `rowHeightDp(900f, 4) == 225f`.
  - `DueTextTest` (`Locale("tr")`, `now = 2026-11-14T10:00+03:00[Europe/Istanbul]`): bugün 17:00 → `"bugün 17:00"`, yarın → `"yarın"`, `2026-11-12` → `"12 Kas"`; `Locale.ENGLISH` → `"today 17:00"`, `"tomorrow"`, `"Nov 12"`; `+00:00` ofsetli değer yerel saate çevrilir.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.board.*'` → FAIL.
- [ ] **Step 3: Uygula.** `shortDue` "bugün/yarın" metinlerini string kaynağından değil sabit bir `Locale`'e göre seçer (Robolectric'siz test edilebilsin diye): `tr` için "bugün", "yarın", diğerleri için "today", "tomorrow"; ay kısaltması `DateTimeFormatter.ofPattern("d MMM", locale)`, Türkçe `"Kas"`.
- [ ] **Step 4:** Aynı komut → PASS.
- [ ] **Step 5: Commit.** `git commit -m "Add board grouping and grid layout logic"`

### Task 6: Pano durumu ve eylemler

**Review:** per-task — destructive operation, concurrency

**Files:**
- Create: `board/BoardViewModel.kt`, `board/DoneVerdict.kt`
- Test: `board/BoardViewModelTest.kt`, `board/DoneVerdictTest.kt`

**Interfaces:**
- Consumes: `FocusdeckApi` ve tipleri (Task 2).
- Produces:

```kotlin
sealed interface DoneVerdict { data object Done : DoneVerdict; data class Failed(val message: String) : DoneVerdict; data object Uncertain : DoneVerdict }
fun doneVerdict(result: ApiResult<ActionResponse>): DoneVerdict // Network → Failed (istek gitmedi), Uncertain → Uncertain
enum class Afterwards { GONE, STILL_THERE, UNKNOWN }
fun resolveUncertain(relist: ApiResult<ItemsResponse>, itemId: String): Afterwards

data class BoxState(val box: BoxView, val items: List<Item> = emptyList(), val error: String? = null,
    val stale: Boolean = false, val loading: Boolean = false, val hidden: Set<String> = emptySet())
data class PendingDone(val boxId: String, val item: Item, val deadlineMillis: Long)
sealed interface BoardMessage {
    data class Done(val title: String) : BoardMessage
    data class DoneFailed(val title: String, val message: String) : BoardMessage
    data class DoneUncertain(val title: String) : BoardMessage
    data class DropRejected(val boxTitle: String) : BoardMessage
    data class ActionFailed(val message: String, val retry: (() -> Unit)?) : BoardMessage
}
data class BoardState(val profile: ProfileView? = null, val boxes: Map<String, BoxState> = emptyMap(),
    val loadingProfile: Boolean = false, val unreachable: ApiFailure? = null,
    val pendingDone: PendingDone? = null, val message: BoardMessage? = null)

class BoardViewModel(api: FocusdeckApi, profile: String, clock: () -> Long = System::currentTimeMillis) : ViewModel() {
    val state: StateFlow<BoardState>
    val authLost: SharedFlow<Unit>
    fun start()            // öne geldi: profili al, kutu başına yoklama başlat
    fun stop()             // arka plana gitti: yoklamayı durdur, bekleyen done'ı hemen gönder
    fun refreshAll()       // profil + bütün kutular
    fun refreshBox(boxId: String)
    fun requestDone(boxId: String, itemId: String) // öğeyi gizler, 5 sn sayar; ikinci istek ilkini hemen gönderir
    fun undoDone()
    fun reorder(boxId: String, itemId: String, position: Int)
    fun drop(fromBoxId: String, itemId: String, toBoxId: String)
    fun messageShown()
}
const val UNDO_MILLIS = 5_000L
const val MIN_REFRESH_SECONDS = 5L
```

- [ ] **Step 1: Testleri yaz** (`kotlinx-coroutines-test`, `StandardTestDispatcher`, `Dispatchers.setMain`, sahte `FocusdeckApi` çağrıları kaydeder).
  - `DoneVerdictTest`: `Ok` → Done; `Server(502, PLUGIN, "x")` → `Failed("x")`; `Network` → Failed; `Uncertain` → Uncertain. `resolveUncertain`: listede yok → GONE, var → STILL_THERE, `Err` → UNKNOWN.
  - `doneWaitsForUndoWindow`: `requestDone` sonrası 4 999 ms'de `action` çağrılmadı, öğe `hidden`; 5 000 ms'de tek çağrı `ActionRequest("x", "done")`.
  - `undoSendsNothing`: `undoDone` sonrası 10 sn ilerle, `action` hiç çağrılmadı, öğe görünür.
  - `stopFlushesPendingDone`: `requestDone` + `stop()` → `action` beklemeden çağrıldı.
  - `secondDoneFlushesFirst`.
  - `failedDoneRestoresItem`: PLUGIN → öğe görünür, `DoneFailed`, `action` bir kez çağrıldı.
  - `uncertainDoneRelists`: `Uncertain` + yeniden listede öğe yok → `Done` mesajı; öğe var → `DoneUncertain`, öğe görünür; iki durumda da `action` bir kez çağrıldı.
  - `doneRelistsSamePluginBoxes`: başarıdan sonra aynı `plugin`'li iki kutu da listelendi, başka plugin'li kutu listelenmedi.
  - `zeroRefreshPollsEveryFiveSeconds`: `refresh = 0` kutu 20 sn içinde 5 kez listelenir (0, 5, 10, 15, 20 sn); `refresh = 60` kutu 1 kez.
  - `stopHaltsPolling`: `stop()` sonrası zaman ilerleyince yeni `items` çağrısı yok.
  - `failedListKeepsStaleItems`: önce 2 öğe, sonra `Network` → öğeler duruyor, `stale = true`, `error` dolu; sonraki başarı ikisini temizler.
  - `describeNullBoxIsNotPolled`.
  - `dropSendsSourceAndKey`: `drop("a", "x", "b")` → `drop(p, "b", DropRequest(item = a'daki öğe, from = Source(a.plugin, "x")), key)`; `Uncertain` sonrası `ActionFailed.retry` aynı `key` ile çağırır; başarıda `a` ve `b` listelendi.
  - `dropOnNonAcceptingBoxSendsNothing`: hedef `acceptsDrop = false` → çağrı yok, `DropRejected`.
  - `reorderIsNotRetried`: `Uncertain` → bir çağrı, kutu yeniden listelendi.
  - `unauthorizedEmitsAuthLost`: herhangi bir çağrıda 401 → `authLost` bir kez yayar.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.board.BoardViewModelTest' --tests 'tr.omertahaoztop.focusdeck.board.DoneVerdictTest'` → FAIL.
- [ ] **Step 3: Uygula.** Durum tek `MutableStateFlow`, güncellemeler `update {}` ile. Kutu başına bir yoklama işi (`viewModelScope`, `start` yeniden başlatır, `stop` iptal eder); aralık `max(box.refresh, MIN_REFRESH_SECONDS)`. Bekleyen `done` gönderimi `NonCancellable` içinde, `stop` ya da `onCleared` onu iptal etmez. Profil değişince yeni profilde olmayan kutuların durumu ve yoklaması düşer.
- [ ] **Step 4:** Aynı komut → PASS.
- [ ] **Step 5: Commit.** `git commit -m "Add board state with undoable done and drops"`

### Task 7: Hatırlatıcılar

**Review:** per-task — persisted format

**Files:**
- Create: `reminders/ReminderPlan.kt`, `reminders/ReminderStore.kt`, `reminders/ReminderSync.kt`, `reminders/AndroidAlarms.kt`, `reminders/Receivers.kt`, `reminders/ReminderWorker.kt`
- Modify: `app/src/main/AndroidManifest.xml` (`USE_EXACT_ALARM`, `SCHEDULE_EXACT_ALARM` `maxSdkVersion="32"`, `RECEIVE_BOOT_COMPLETED`, `POST_NOTIFICATIONS`; `AlarmReceiver`, `SyncTriggerReceiver` `BOOT_COMPLETED` + `TIMEZONE_CHANGED` + `TIME_SET`), iki `strings.xml` (kanal adı "Hatırlatıcılar" / "Reminders")
- Test: `reminders/ReminderPlanTest.kt`, `reminders/ReminderSyncTest.kt`, `reminders/AlarmReceiverTest.kt`

**Interfaces:**
- Consumes: `FocusdeckApi.reminders`, `FocusdeckApi.profile`, `BoxReminder` (Task 2); `SettingsStore.connection` (Task 3).
- Produces:

```kotlin
fun reminderKey(r: BoxReminder): String // "${boxId}\u0000${item.id}\u0000${at}", at sunucunun metniyle
data class ReminderPlan(val notifyNow: List<BoxReminder>, val schedule: List<BoxReminder>, val cancel: Set<String>)
fun planReminders(reminders: List<BoxReminder>, shown: Set<String>, scheduled: Set<String>, now: Instant): ReminderPlan
@Serializable data class AlarmPayload(val key: String, val boxId: String, val boxTitle: String, val itemId: String, val title: String, val at: String)
@Serializable data class ReminderState(val shown: Map<String, Long> = emptyMap(), val scheduled: Map<String, AlarmPayload> = emptyMap())
class ReminderStore(store: DataStore<ReminderState>) {
    suspend fun state(): ReminderState
    suspend fun markShown(key: String, nowMillis: Long) // 48 saatten eski kayıtları da siler
    suspend fun setScheduled(scheduled: Map<String, AlarmPayload>)
}
interface AlarmScheduler { fun schedule(payload: AlarmPayload, atMillis: Long); fun cancel(key: String) }
interface ReminderNotifier { fun show(payload: AlarmPayload) }
class AndroidAlarms(context: Context) : AlarmScheduler, ReminderNotifier
class ReminderSync(connection: suspend () -> Pair<FocusdeckApi, String>?, store: ReminderStore,
    alarms: AlarmScheduler, notifier: ReminderNotifier, now: () -> Instant) { suspend fun run(): Boolean }
fun enqueueReminderSync(context: Context, periodic: Boolean) // periyodik "reminder-sync" 15 dk KEEP, tek seferlik "reminder-sync-now" REPLACE
const val EXTRA_BOX = "box"; const val EXTRA_ITEM = "item" // bildirim dokunuşu MainActivity'ye
```

- [ ] **Step 1: Testleri yaz.**
  - `ReminderPlanTest` (`now = 2026-11-14T10:00:00+03:00`): geçmiş ve gösterilmemiş → `notifyNow`; gösterilmiş → hiçbirinde; `now + 23 sa` → `schedule`; `now + 25 sa` → hiçbirinde; `scheduled`'da olup cevapta olmayan anahtar → `cancel`; `offsetsCompareAsInstants` (`at = 2026-11-14T06:59:00+00:00` geçmiş, `07:01:00+00:00` gelecek).
  - `ReminderSyncTest`: `since` `now − 12 sa` ile çağrılır; plan sahte `AlarmScheduler`/`ReminderNotifier`'a uygulanır, hemen gösterilenler `shown`'a yazılır; kutu başlığı profilden, profil alınamazsa kutu kimliği; bağlantı yoksa `run` hiçbir şey çağırmaz; `Err` → `false`, alarm iptal edilmez.
  - `AlarmReceiverTest` (Robolectric): yayın bildirim gösterir (`ShadowNotificationManager`), başlık öğe başlığı, metin `"<kutu> · 17:00"`, metinde adres yok; anahtar `shown`'a yazılır.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.reminders.*'` → FAIL.
- [ ] **Step 3: Uygula.** `AndroidAlarms.schedule`: `canScheduleExactAlarms()` (API 31+) true ya da API < 31 ise `setExactAndAllowWhileIdle`, değilse `setAndAllowWhileIdle`; `PendingIntent` kimliği `data = Uri("focusdeck://reminder/" + Uri.encode(key))`, `FLAG_IMMUTABLE or FLAG_UPDATE_CURRENT`. `AlarmReceiver` `goAsync` ile `markShown` yazar. `SyncTriggerReceiver` tek seferlik eşitleme kuyruğa atar (açılışta alarmlar silinmiş olur, `schedule` her eşitlemede yeniden kurar). Bildirim izni yoksa `show` sessizce geçer.
- [ ] **Step 4:** Aynı komut → PASS.
- [ ] **Step 5: Commit.** `git commit -m "Add reminder sync with exact alarms"`

### Task 8: Kurulum, ayarlar ve uygulama iskeleti

**Files:**
- Create: `App.kt`, `ui/ErrorText.kt`, `ui/setup/SetupScreen.kt`, `ui/setup/SetupViewModel.kt`, `ui/settings/SettingsScreen.kt`
- Modify: `MainActivity.kt`, `AndroidManifest.xml` (`android:name=".FocusdeckApp"`), iki `strings.xml`
- Test: `ui/setup/SetupViewModelTest.kt`, `ui/ErrorTextTest.kt`, `ui/setup/SetupScreenTest.kt`

**Interfaces:**
- Consumes: `parseServerUrl`, `HttpFocusdeckApi`, `ApiFailure` (Task 2); `SettingsStore`, `KeystoreTokenCipher` (Task 3); `enqueueCaptureDrain` (Task 4); `enqueueReminderSync` (Task 7).
- Produces:

```kotlin
class FocusdeckApp : Application() { lateinit var graph: AppGraph }
class AppGraph(context: Context) {
    val settings: SettingsStore
    val captureQueue: CaptureQueue
    suspend fun api(): Pair<FocusdeckApi, String>? // bağlantı + profil
}
@StringRes fun errorText(failure: ApiFailure): Int // DNS, CONNECT, TLS, Uncertain (zaman aşımı), 401, 403, diğer
sealed interface Screen { data object Setup : Screen; data class ProfilePick(val profiles: List<String>, val default: String?) : Screen; data object Board : Screen; data object Settings : Screen }
@Composable fun BoardHost(graph: AppGraph, openItem: Pair<String, String>?, onSettings: () -> Unit) // Task 9 doldurur; bu görevde "Pano" yazan yer tutucu
```

- [ ] **Step 1: Testleri yaz.**
  - `ErrorTextTest`: yedi durumun her biri ayrı kaynak; `Server(403, null)` "bu ağdan erişim yok" metnine gider.
  - `SetupViewModelTest`: tek profil → doğrudan `Board`; `default` var → `Board` o profille; iki profil, `default` yok → `ProfilePick`; 401 → hata `errorText`; `http://deck…` → `HTTP_NOT_ALLOWED` hatası, istek gitmez.
  - `SetupScreenTest` (Robolectric Compose): token alanı `PasswordVisualTransformation` kullanır; adres dolu gelir (`savedUrl`).
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.ui.*'` → FAIL.
- [ ] **Step 3: Uygula.** Gezinme elle (`Screen` durumu, `rememberSaveable`). Açılışta bağlantı yoksa `Setup`. `authLost` → `clearToken` + `Setup`. Kurulum bitince bildirim izni (API 33+) bir kez sorulur, `setNotificationAsked`. Açılışta `enqueueCaptureDrain` ve `enqueueReminderSync(periodic = true)` + tek seferlik. Ayarlar: adres/token değiştirme (kurulum ekranı), profil değiştirme, bildirim izni durumu + `Settings.ACTION_APP_NOTIFICATION_SETTINGS`, `BuildConfig.VERSION_NAME`. Bildirimden gelen `EXTRA_BOX`/`EXTRA_ITEM` `BoardHost`'a `openItem` olarak geçer.
- [ ] **Step 4:** Aynı komut → PASS; `./gradlew assembleDebug` → BUILD SUCCESSFUL.
- [ ] **Step 5: Commit.** `git commit -m "Add setup, settings and navigation"`

### Task 9: Pano ekranları

**Files:**
- Create: `ui/board/BoardScreen.kt`, `ui/board/ListBoard.kt`, `ui/board/GridBoard.kt`, `ui/board/ItemRow.kt`, `ui/detail/ItemDetail.kt`
- Modify: `App.kt` (`BoardHost` gerçek ekranı çağırır), iki `strings.xml`
- Test: `ui/board/ListBoardTest.kt`, `ui/board/GridBoardTest.kt`, `ui/detail/ItemDetailTest.kt`

**Interfaces:**
- Consumes: `BoardViewModel`, `BoardState`, `BoxState` (Task 6); `groupTabs`, `listRows`, `gridSpec`, `useGrid`, `rowHeightDp`, `shortDue` (Task 5); `SettingsStore.ui`, `setSelectedTab`, `setCollapsed` (Task 3).
- Produces: `@Composable fun BoardScreen(vm: BoardViewModel, ui: UiPrefs, actions: BoardActions)`; `data class BoardActions(val onSettings: () -> Unit, val onTab: (String) -> Unit, val onCollapse: (String, Boolean) -> Unit)`; `@Composable fun ItemRow(item: Item, now: ZonedDateTime, modifier: Modifier)`; `@Composable fun ItemDetail(boxTitle: String, item: Item, canDone: Boolean, canMove: Boolean, onDone: () -> Unit, onMove: () -> Unit)`. Task 10 bu ekranlara `Modifier` ve düğme bağlar.

- [ ] **Step 1: Testleri yaz** (Robolectric Compose, `@Config(qualifiers = …)`).
  - `ListBoardTest` (fixture: `Mono · Redmine` 3 öğe biri gecikmiş, `Mono · Inbox` 0, `Kişisel · Gmail` 2, `Kişisel · Inbox` 0, `Kişisel · Bu akşam` 0, `Kişisel · Fikirler` 0, `Notlar` 1): `w411dp-h891dp` → sekmeler "Mono 3", "Kişisel 2", "Diğer 1"; tek gruplu profilde sekme çubuğu yok; Mono sekmesinde kırmızı nokta (`testTag("overdue-dot")`); Kişisel'de boş kutular tek satırda "Inbox · Bu akşam · Fikirler 0"; bölüm başlığına dokununca `onCollapse(boxId, true)`; yana kaydırınca sonraki sekme; öğeye dokununca tam sayfa ayrıntı; `describe == null` kutuda `error` metni kırmızı satırda.
  - `GridBoardTest`: `w841dp-h701dp` → ızgara (`testTag("grid")`), kutu `row/col` konumunda; `w673dp-h420dp` → liste; öğeye dokununca `ModalBottomSheet` içinde ayrıntı.
  - `ItemDetailTest`: `canDone = false` iken "Bitti" yok; link dokunuşu `Intent.ACTION_VIEW` başlatır.
  - `openItemShowsDetail`: `BoardScreen` `openItem = ("b", "x")` ile açılınca `x`'in ayrıntısı görünür (bildirim dokunuşu).
  - Ortak: `unreachable` doluyken üstte "Sunucuya ulaşılamıyor · <neden>"; `stale` kutuda soluk liste + hata satırı.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.ui.board.*' --tests 'tr.omertahaoztop.focusdeck.ui.detail.*'` → FAIL.
- [ ] **Step 3: Uygula.** `BoardViewModel` `viewModel(key = profile) { BoardViewModel(api, profile) }` ile `AppGraph.api()`'den kurulur. Düzen seçimi `currentWindowAdaptiveInfo().windowSizeClass` ile `useGrid` eşikleri. Liste `HorizontalPager` + `PrimaryScrollableTabRow`, aşağı çekmek `PullToRefreshBox` → `refreshAll`; ızgarada üst çubukta yenile düğmesi. Izgara `Layout` ile: sütun ağırlığı eşit, `emptyColumns` 8 dp. Kutu kaydırma konumu `rememberSaveable(key = box.id)`, açık ayrıntı (`boxId`, `itemId`) `rememberSaveable` ile, katlanınca ve açılınca korunur. `LifecycleEventEffect(ON_START/ON_STOP)` → `vm.start()/stop()`. Profil yüklenirken kutu iskeletleri.
- [ ] **Step 4:** Aynı komut → PASS.
- [ ] **Step 5: Commit.** `git commit -m "Add list and grid board screens"`

### Task 10: Eylem arayüzü

**Files:**
- Create: `ui/actions/SwipeDone.kt`, `ui/actions/DragDrop.kt`, `ui/actions/MoveDialog.kt`, `ui/capture/CaptureSheet.kt`, `ui/capture/QueueLine.kt`
- Modify: `ui/board/BoardScreen.kt`, `ui/board/ListBoard.kt`, `ui/board/GridBoard.kt`, `ui/detail/ItemDetail.kt` (bağlantılar), iki `strings.xml`
- Test: `ui/actions/SwipeDoneTest.kt`, `ui/actions/DragDropTest.kt`, `ui/actions/MoveDialogTest.kt`, `ui/capture/CaptureSheetTest.kt`

**Interfaces:**
- Consumes: `BoardViewModel.requestDone`, `undoDone`, `reorder`, `drop`, `BoardMessage` (Task 6); `CaptureSender.submit`, `CaptureQueue.notes`, `remove`, `SendOutcome`, `deviceLang` (Task 4); Task 9 ekranları.
- Produces: kullanıcıya görünen eylemler; yeni ortak arayüz yok.

- [ ] **Step 1: Testleri yaz** (Robolectric Compose, sahte `FocusdeckApi` ile gerçek `BoardViewModel`).
  - `SwipeDoneTest`: `done` destekleyen kutuda sola kaydırma → öğe kaybolur, snackbar "Bitti: <başlık>" + "Geri al"; "Geri al" → öğe geri, 6 sn sonra `action` çağrılmadı; `done` desteklemeyen kutuda kaydırma etkisiz.
  - `DragDropTest`: uzun basıp başka kutuya bırakma → `drop`; `acceptsDrop = false` kutu soluk (`testTag("drop-dim")`), bırakınca "<kutu> bırakma kabul etmiyor"; aynı kutuda iki satır aşağı bırakma, `reorder` varsa `position = 2`, yoksa çağrı yok; katlı bölüm üstünde 600 ms bekleyince açılır.
  - `MoveDialogTest`: liste kaynak kutuyu ve bırakma kabul etmeyenleri içermez, grup başlıklarıyla; seçim `drop` çağırır.
  - `CaptureSheetTest`: "+" → alt panel, odak alanda, ipucu `// açıklama  #etiket  @kimden  yarın 9:00`; `Sent` → snackbar "Eklendi: <başlık> → <kutu>"; `Queued` → "Kuyruğa alındı"; `Rejected` → metin alana geri döner ve hata görünür; kuyrukta 2 bekleyen + 1 gönderilemeyen → "2 not gönderilmeyi bekliyor", "1 not gönderilemedi"; gönderilemeyenler listesinde "Düzenle" metni alana koyar ve kaydı siler, "Sil" kaydı siler.
- [ ] **Step 2:** `./gradlew testDebugUnitTest --tests 'tr.omertahaoztop.focusdeck.ui.actions.*' --tests 'tr.omertahaoztop.focusdeck.ui.capture.*'` → FAIL.
- [ ] **Step 3: Uygula.** Kaydırma `SwipeToDismissBox` (`EndToStart`), dönüşte `requestDone`, kutu satırı kendi durumuna döner (gizleme ViewModel'den). Sürükleme `pointerInput` + `detectDragGesturesAfterLongPress`, hedef kutu sınırları `onGloballyPositioned` ile toplanır, yarı saydam kopya üst katmanda; sekmeler arası sürükleme yok. Uzun basıp parmak kıpırdamadan kaldırılırsa menü açılır ("Bitti", "Taşı…"); "Taşı…" ayrıntıda da var. `Sent` sonrası `vm.refreshBox(response.boxId)`, `Queued` sonrası `enqueueCaptureDrain`.
- [ ] **Step 4:** Aynı komut → PASS; tüm test takımı `./gradlew lint testDebugUnitTest assembleDebug` → BUILD SUCCESSFUL.
- [ ] **Step 5: Commit.** `git commit -m "Add swipe done, drag and drop and note capture UI"`

### Task 11: Emülatörde uçtan uca (ana ajan)

**Files:**
- Test: yok (elle senaryo, ekran görüntüleri `docs/screenshots/folded.png`, `docs/screenshots/unfolded.png`)

- [ ] **Step 1:** `sdkmanager "emulator" "system-images;android-36;google_apis;x86_64"`, `avdmanager create avd -n fd-fold -k "system-images;android-36;google_apis;x86_64" -d pixel_10_pro_fold`.
- [ ] **Step 2:** Geçici `HOME` altında deneme sunucusu: `focusdeck` deposundan derlenmiş ikili, `[serve]` varsayılan `127.0.0.1:8740`, geçici token dosyası, profilde `Mono · Inbox`, `Mono · İşler`, `Kişisel · Inbox` (`local` kutular, biri `capture_into`, birinde `due`'lu öğe). `focusdeck server` hub ile başlatılır.
- [ ] **Step 3:** `emulator -avd fd-fold` (KVM), `./gradlew installDebug connectedDebugAndroidTest` → `KeystoreTokenCipherTest` PASS.
- [ ] **Step 4:** Uygulamada `http://10.0.2.2:8740` + token. Kapalı ve açık duruşta (`adb emu fold` / `unfold`): not ekleme, bitirme ve geri alma, sürükleyip taşıma, sıralama, sunucu durdurulup not → "kuyruğa alındı", sunucu açılınca gönderilir; 2 dk sonraya `due`'lu öğe → hatırlatıcı bildirimi. Her adımın sonucu masaüstü TUI'de istemci kipinde (`[server] url = "http://127.0.0.1:8740"`) görülür. Ekran görüntüleri `adb exec-out screencap -p`.
- [ ] **Step 5:** Geçici `HOME`, token ve sunucu durdurulup silinir (`trash-put`). Ekran görüntüleri commit: `git commit -m "Add emulator screenshots"`

### Task 12: Release, belgeler, telefon (ana ajan)

**Review:** per-task — security

**Files:**
- Create: `.github/workflows/release.yml`, `README.tr.md`
- Modify: `app/build.gradle.kts` (release `signingConfig` ortam değişkenlerinden: `SIGNING_KEYSTORE_PATH`, `SIGNING_STORE_PASSWORD`, `SIGNING_KEY_ALIAS`, `SIGNING_KEY_PASSWORD`; değişken yoksa release imzasız kalır ve CI'daki etiket işi düşer), `README.md`

- [ ] **Step 1:** `release.yml`: `push: tags: ['v*']`; `SIGNING_KEYSTORE_BASE64` secret'ı geçici dosyaya çözülür, `./gradlew assembleRelease`, `sha256sum` → `focusdeck-<sürüm>.apk` ve `.sha256`, `attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8 # v4.2.2`, `gh release create`. `permissions` en dar (`contents: write`, `id-token: write`, `attestations: write`).
- [ ] **Step 2:** README.md (İngilizce) ve README.tr.md: ne olduğu, iki ekran görüntüsü, APK indirme + `sha256sum -c` + kurulum, sunucu adresi ve token, derleme. Türkçe dosya `detect_tr_ai.py` ile denetlenir.
- [ ] **Step 3:** `gh repo create omertahaoztop/focusdeck-android --public --source . --push`. CI yeşil: `gh run watch`.
- [ ] **Step 4:** İmza anahtarı: `keytool -genkeypair` (RSA 4096, 10000 gün, alias `focusdeck`), parolalar `openssl rand` ile, dosya `~/.cache/focusdeck-android-signing/` altında 0600. Ömer Bitwarden'e tek kayıt ekler (dosya ek, alias ve iki parola); onayından sonra `gh secret set` dört secret'ı stdin'den alır, dizin `trash-put` ile silinir. Değerler ekrana basılmaz.
- [ ] **Step 5:** `git tag v0.1.0 && git push origin v0.1.0`; sürüm sayfasında APK ve `.sha256`, indirilen dosyada `sha256sum -c` OK.
- [ ] **Step 6:** Telefon: kablosuz ADB ile `adb install`, Ömer mobil veriden `https://deck.omertahaoztop.tr` + token girer, bir not ve bir bitirme masaüstünde görülür.
- [ ] **Step 7:** ops-journal: `wiki/kisisel-repolar.md`'ye focusdeck-android satırı, `wiki/kisisel-telefon.md` yüzey tablosuna focusdeck, focusdeck servis sayfasına telefon kaybında token yenileme adımı; `check.py` OK.
