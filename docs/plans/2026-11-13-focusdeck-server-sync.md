# focusdeck sunucu, istemci kipi ve Android: uygulama planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pano bir sunucuda yaşar. Ofis, ev ve telefon aynı kutuları, aynı notları görür. `config.toml`'da `[server]` yoksa focusdeck bugünkü gibi yerelde çalışır.

**Architecture:** Aynı programa `focusdeck server` alt komutu eklenir. Sunucu profilleri okur, kutuların plugin'lerini (`serve-local` dahil) bugünkü `PluginHost` ile çalıştırır, panoyu `axum` ile REST API olarak sunar (`/api/v1`, Bearer token, Caddy arkasında). İstemci kipinde worker `PluginHost` yerine HTTP konuşan bir uzak kutu sürer, profil düzeni sunucudan gelir. Android uygulaması aynı API'yi kullanır.

**Tech Stack:** Rust 2024 (`rust-version = "1.89"`), tokio, axum (yeni), reqwest 0.13 rustls (var), serde, sha2 (var), mockito ve tempfile testlerde. Android: Kotlin, Jetpack Compose, Material 3, OkHttp, kotlinx.serialization, WorkManager, `minSdk 26`.

**Spec:** `docs/superpowers/specs/2026-11-13-focusdeck-server-design.md`

## Bağlam

Notlar her makinenin kendi `~/.local/share/focusdeck/local.json` dosyasında, plugin token'ları da her makinede ayrı. Telefondan bakılamıyor. Ömer'in kararları (2026-11-13):

| Konu | Karar |
|---|---|
| Sunucu ne taşır | Bütün pano: plugin'ler sunucuda çalışır, token'lar oraya taşınır |
| Sunucuya ulaşılamazsa | Kutuda kırmızı hata satırı, `capture` kuyrukta bekler. Çevrimdışı eşitleme yok |
| Erişim | Caddy arkasında herkese açık HTTPS + token |
| Sunucu yeri | pve11'de yeni LXC, Forgejo `Caddy` reposundan vhost |
| Vikunja | Kutular olduğu gibi kalır, sunucuda Vikunja plugin'iyle çalışır |
| Android | Kotlin + Compose. Not ekleme, sürükle-bırak ile taşıma, bitirme |

"Ayrım nasıl yapılır" sorusunun cevabı bugünkü yapıda var: her kutunun bir plugin'i var. Vikunja kutusu profilde yoksa Vikunja devrede değil. Kip seçimi ise makine başına: `[server]` varsa istemci, yoksa yerel.

## Yaklaşım

Üç plan, sırayla. Her biri kendi başına çalışan bir sonuç verir.

1. **Plan A: çekirdek** (bu repo, aşağıda görev görev). Sunucu, API, istemci kipi.
2. **Plan B: kurulum** (pve11 LXC, Caddy, masaüstleri). Plan A bitince ayrıntılı komut planı `docs/plans/2026-11-1x-focusdeck-server-deploy.md` olarak yazılır, uzak host komutları o planda tek tek gösterilir. Aşağıda görev listesi var.
3. **Plan C: Android** (yeni repo `~/Desktop/personal/focusdeck-android`). API Plan A'da donar, Plan C onun üstüne kurulur. Ayrıntılı plan repoyla birlikte yazılır. Aşağıda görev listesi var.

Plan A'da `local` kaydına, plugin protokolüne ve plugin'lere dokunulmaz. Sunucu `serve-local`'ı bugünkü gibi çocuk süreç olarak başlatır.

## Değişecek dosyalar (Plan A)

| Dosya | Değişiklik |
|---|---|
| `Cargo.toml` | workspace'e `axum` |
| `crates/focusdeck/Cargo.toml` | `axum`, sürüm `0.4.0` |
| `crates/focusdeck/src/config.rs` | `[serve]`, `[server]` bölümleri, `CaptureWords: Serialize` |
| `crates/focusdeck/src/secrets.rs` | `read_token` |
| `crates/focusdeck/src/install.rs` | `https_or_loopback` (mevcut `is_loopback` kuralından) |
| `crates/focusdeck/src/fsio.rs` (yeni) | `write_atomic`, `unique_id`, `local/store.rs`'den taşınır |
| `crates/focusdeck/src/local/store.rs` | `fsio`'yu kullanır |
| `crates/focusdeck/src/api.rs` (yeni) | API gövde ve cevap tipleri, hata kodları |
| `crates/focusdeck/src/server/mod.rs`, `idempotency.rs`, `board.rs`, `http.rs` (yeni) | sunucu |
| `crates/focusdeck/src/remote.rs` (yeni) | `RemoteClient`, `RemoteBox`, profil seçimi |
| `crates/focusdeck/src/host.rs` | `BoxHost` enum'u |
| `crates/focusdeck/src/worker.rs`, `remind.rs`, `capture_cmd.rs` | `BoxHost` ve istemci kipi |
| `crates/focusdeck/src/queue.rs` | `Queued.id`, `Queued::key` |
| `crates/focusdeck/src/ui/app.rs`, `ui/run.rs` | plan bağını kapatma bayrağı, `Deck` |
| `crates/focusdeck/src/cli.rs`, `main.rs` | `server` alt komutu, kip seçimi |
| `crates/focusdeck/locales/en.ftl`, `tr.ftl` | yeni metinler |
| `crates/focusdeck/tests/server.rs` (yeni) | gerçek `focusdeck server` süreciyle uçtan uca |
| `README.md`, `README.tr.md` | "Sunucu" bölümü |

## Yeniden kullanılacaklar

- `ui::run::build_host` (`crates/focusdeck/src/ui/run.rs:251`): sunucu kutu kurarken aynen kullanır, `local` için `current_exe` + `serve-local` dahil.
- `PluginHost::describe`, `PluginHost::call` (`host.rs:128-170`): çöken plugin'i sonraki çağrıda yeniden başlatma burada hazır.
- `links::closures_after_list` (`links.rs:47`): sunucuda plan bağı.
- `capture::parse`, `capture::Words::new` (`capture.rs`): sunucuda `captures` ucu, istemci TUI'de profil cevabındaki kelimelerle.
- `profile::load`, `profile::select_name` (`profile.rs:256,270`): sunucu açılışı ve istemci profil seçimi.
- `secrets::check_secret`, `expand_home` (`secrets.rs:86-93`): token dosyası denetimi.
- `install::is_loopback` (`install.rs:128`): `https_or_loopback`'in temeli.
- `local::store::Store::write` ve `new_id` (`local/store.rs:195,220`): `fsio`'ya taşınıp kimlik deposu ve kuyruk kimliğiyle paylaşılır.
- `queue::Queue` (`queue.rs`): istemci kipinde de aynı kuyruk.
- `tests/host.rs` ve `fake-plugin`: sunucu testlerinde plugin yerine.

## Global Constraints

- `[server]` olmayan makinede davranış birebir aynı. Mevcut testlerin hepsi değişmeden geçer.
- API'den `config` tablosu, `*_file` yolu, e-posta, plugin adresi çıkmaz.
- Token en az 32 karakter, dosyası `secrets.rs` kuralıyla denetlenir. Token log'a, hata metnine, API cevabına girmez.
- `url` `https://`. `http://` yalnız `127.0.0.1`, `[::1]`, `localhost`.
- API alanları `snake_case`, hata gövdesi `{"error": {"code", "message"}}`, kodlar: `UNAUTHORIZED` 401, `NOT_FOUND` 404, `INVALID` 422, `UNSUPPORTED` 422, `PLUGIN` 502, `IN_PROGRESS` 409, `KEY_REUSED` 422.
- `listen` varsayılanı `127.0.0.1:8740`. İstemci istek zaman aşımı 20 sn. İstek gövdesi sınırı 1 MiB. Idempotency kaydı saklama süresi 90 gün.
- Kullanıcıya görünen her yeni metin `en.ftl` + `tr.ftl`'de, parite testi geçer. Türkçe metinler Türkçe karakterle.
- Her task sonunda: `cargo fmt --all --check`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings` temiz.
- Commit İngilizce, imperatif, tek satır ≤72 karakter, gövdesiz.

## Review Focus

1. **Sunucu çalışırken plugin çöker ya da 15 sn'de cevap vermez.** O kutu `PLUGIN` döner, öteki kutular etkilenmez, sonraki istekte plugin yeniden başlar. Test: Task 5 `crashed_plugin_answers_plugin_then_recovers`.
2. **Telefon ve masaüstü aynı anda aynı kutuya yazar** (biri taşırken öteki bitirir). İstekler kutu kilidinde sıraya girer, kilit hiçbir zaman iki kutuda birden tutulmaz. Test: Task 5 `concurrent_actions_on_one_box_are_serialized`, Task 6 `drop_between_local_boxes_moves_once`.
3. **`capture` kuyruğu sunucu cevap verdikten sonra öldürülür, not tekrar gönderilir.** İkinci istek aynı anahtarla gelir, ikinci not yaratılmaz. Test: Task 6 `capture_with_same_key_twice_creates_one_note`, Task 8 `remote_capture_sends_entry_key`.
4. **Yanlış ya da eksik token, `Bearer` öneksiz başlık, boş token dosyası.** Hep 401, gövde aynı biçimde, token hata metninde yok. Test: Task 6 `requests_without_valid_token_get_401`, Task 1 `read_token_rejects_short_and_exposed`.
5. **Sunucu kapalıyken TUI açılır, açıkken sunucu yeniden başlar.** Açılışta adresi ve nedeni söyleyen hata, açıkken kutuda kırmızı satır, sunucu dönünce sonraki yoklamada düzelir. Test: Task 7 `remote_box_network_error_is_red_line_then_recovers`.

---

## Plan A: çekirdek

### Task 1: `[serve]` ve `[server]` ayarları, token okuma

**Review:** per-task — security

**Files:**
- Modify: `crates/focusdeck/src/config.rs`, `crates/focusdeck/src/secrets.rs`, `crates/focusdeck/src/install.rs`
- Modify: `crates/focusdeck/locales/en.ftl`, `tr.ftl`

**Interfaces:**
- Produces: `config::ServeConfig { listen: std::net::SocketAddr, token_file: String }` (`listen` varsayılanı `127.0.0.1:8740`), `config::ServerConfig { url: String, token_file: String }`, `Config.serve: Option<ServeConfig>`, `Config.server: Option<ServerConfig>`, `CaptureWords: Serialize`. `secrets::read_token(file: &str, home: &Path) -> Result<String, SecretError>` (sondaki satır sonu kırpılır), yeni `SecretError::TokenTooShort(PathBuf)`. `install::https_or_loopback(url: &str) -> bool`.

- [ ] **Step 1: Failing testler.** `config.rs`: `config_without_server_sections_equals_default` (bugünkü örnek dosya, `serve` ve `server` `None`), `config_reads_serve_and_server` (`listen` verilmezse `127.0.0.1:8740`), `server_url_plain_http_is_refused_unless_loopback` (`http://deck.example.com` → `Config::load` hata, `http://127.0.0.1:8740` kabul). `secrets.rs`: `read_token_rejects_short_and_exposed` (31 karakter → `TokenTooShort`, 0644 → `Exposed`, 0600 ve `"abc…\n"` → satır sonu kırpılmış değer), `token_error_text_has_no_token` (hata metninde token değeri geçmez).
- [ ] **Step 2: Çalıştır, düştüğünü gör.** `cargo test -p focusdeck config:: secrets::` → derleme hatası ya da FAIL.
- [ ] **Step 3: Uygula.** `Config`'te `deny_unknown_fields` korunur. URL kuralı `Config::load` içinde `https_or_loopback` ile, hata metni `msg::ERROR_SERVER_URL_INSECURE`.
- [ ] **Step 4: Testler geçer, sonra üç kontrol komutu temiz.**
- [ ] **Step 5: Commit** `Add serve and server settings with token checks`

### Task 2: Ortak dosya yardımcıları

**Files:**
- Create: `crates/focusdeck/src/fsio.rs`
- Modify: `crates/focusdeck/src/local/store.rs`, `crates/focusdeck/src/lib.rs`

**Interfaces:**
- Produces: `fsio::write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()>` (`.tmp` + `fsync` + `rename` + dizin `fsync`, mod 0600), `fsio::unique_id() -> String` (bugünkü `new_id` üreteci).

- [ ] **Step 1:** `fsio.rs` testi `write_atomic_leaves_no_tmp_and_mode_0600`, `unique_ids_differ_across_calls`.
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** `Store::write` ve `new_id` bunları kullanır, davranış değişmez. `local/store.rs` testleri değişmeden geçer.
- [ ] **Step 4:** Üç kontrol komutu temiz.
- [ ] **Step 5: Commit** `Move atomic write and id generation into fsio`

### Task 3: API tipleri

**Review:** per-task — security

**Files:**
- Create: `crates/focusdeck/src/api.rs`
- Modify: `crates/focusdeck/src/lib.rs`

**Interfaces:**
- Produces (hepsi `Serialize + Deserialize`, `snake_case`):
  - `ProfilesResponse { profiles: Vec<String>, default: Option<String> }`
  - `ProfileView { name, capture_into: Option<String>, capture_refs: BTreeMap<String,String>, capture_label_aliases: BTreeMap<String,String>, capture_words: CaptureWords, boxes: Vec<BoxView> }`
  - `BoxView { id, title, plugin, row: u16, col: u16, width: u16, height: u16, refresh: u64 /* saniye */, plan_into: Option<String>, describe: Option<Describe>, error: Option<String> }`, `BoxView::from_spec(spec: &BoxSpec, describe: Result<Describe, String>) -> BoxView`
  - `ProfileView::from_profile(p: &Profile, words: &CaptureWords, describes: Vec<Result<Describe,String>>) -> ProfileView`, `ProfileView::to_profile(&self) -> Profile` (her `BoxSpec.config` boş tablo)
  - `ItemsResponse { items: Vec<Item>, listed_at: DateTime<FixedOffset> }`
  - `ActionRequest { item: String, action: String, #[serde(default)] args: Value }`, `ActionResponse { item: Option<Item> }`
  - `DropRequest` = `focusdeck_protocol::DropParams`, `DropResponse { item: Item }`
  - `CaptureRequest { text: String, at: DateTime<FixedOffset>, lang: Option<String> }`, `CaptureResponse { #[serde(rename = "box")] box_id: String, item: Item }`
  - `BoxReminder { #[serde(rename = "box")] box_id: String, at, item: Item }`, `RemindersResponse { reminders: Vec<BoxReminder> }`
  - `ErrorCode` (`UNAUTHORIZED`, `NOT_FOUND`, `INVALID`, `UNSUPPORTED`, `PLUGIN`, `IN_PROGRESS`, `KEY_REUSED`; `SCREAMING_SNAKE_CASE`), `ErrorCode::status(self) -> u16`, `ApiError { error: ApiErrorBody { code: ErrorCode, message: String } }`

- [ ] **Step 1:** Testler: `box_view_never_carries_config` (`config = { key_file = "~/x.key", email = "a@b" }` olan `BoxSpec` → JSON'da `config`, `key_file`, `a@b` geçmez), `error_codes_serialize_upper_snake_and_map_status` (`PLUGIN` → 502, `IN_PROGRESS` → 409), `profile_view_round_trips_to_profile` (konum, boyut, `refresh`, `plan_into`, `capture_into` korunur).
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** Uygula.
- [ ] **Step 4:** Üç kontrol komutu temiz.
- [ ] **Step 5: Commit** `Add API request and response types`

### Task 4: Tekrar gönderme kaydı

**Review:** per-task — concurrency, persisted format

**Files:**
- Create: `crates/focusdeck/src/server/mod.rs`, `crates/focusdeck/src/server/idempotency.rs`

**Interfaces:**
- Consumes: `fsio::write_atomic`.
- Produces: `IdempotencyStore::open(path: PathBuf, now: DateTime<FixedOffset>) -> io::Result<(IdempotencyStore, usize /* silinen in_progress */)>`, `claim(&self, key: &str, body_sha256: [u8; 32], now) -> io::Result<Claim>`, `complete(&self, key: &str, response: &Value) -> io::Result<()>`, `release(&self, key: &str) -> io::Result<()>`. `enum Claim { New, Replay(Value), InProgress, Reused }`. İçeride `std::sync::Mutex`, her değişiklik `write_atomic` ile diske.
- Dosya biçimi: `{"version": 1, "keys": {"<key>": {"body": "<hex>", "state": "in_progress"|"done", "response": …, "at": RFC3339}}}`.

- [ ] **Step 1:** Testler: `same_key_same_body_replays`, `same_key_other_body_is_reused`, `claim_while_in_progress`, `release_allows_retry`, `reopen_drops_in_progress_and_counts_them`, `entries_older_than_90_days_are_pruned_on_open`, `eight_threads_claiming_one_key_get_one_new`.
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** Uygula. Sahiplenme oku-kontrol-yaz tek kilit içinde.
- [ ] **Step 4:** Üç kontrol komutu temiz.
- [ ] **Step 5: Commit** `Add idempotency key store for the server`

### Task 5: Sunucu panosu

**Review:** per-task — concurrency

**Files:**
- Create: `crates/focusdeck/src/server/board.rs`
- Test: aynı dosyada, `fake-plugin` ile (`tests/host.rs`'teki kurulum düzeni)

**Interfaces:**
- Consumes: `build_host`, `PluginHost`, `api::*`, `links::closures_after_list`, `capture::parse`.
- Produces: `Board::load(paths: &Paths, cfg: &Config, home: &Path) -> anyhow::Result<Board>` (`profiles/*.toml` hepsi, biri bozuksa hata, hiç profil yoksa hata). Yöntemler (hepsi `async`, `&self`): `profiles() -> ProfilesResponse`, `profile_view(p) -> Result<ProfileView, BoardError>`, `items(p, b, now) -> Result<ItemsResponse, BoardError>`, `action(p, b, ActionRequest) -> Result<ActionResponse, BoardError>`, `drop(p, b, DropRequest) -> Result<DropResponse, BoardError>`, `capture(p, CaptureRequest) -> Result<CaptureResponse, BoardError>`, `reminders(p, since) -> Result<RemindersResponse, BoardError>`. `enum BoardError { NotFound, Invalid(String), Unsupported(String), Plugin(String) }`, `BoardError::code(&self) -> ErrorCode`.
- Kutu durumu: `tokio::sync::Mutex<PluginHost>` + `std::sync::Mutex<Cache { items, listed_at, empty_lists }>`. Kural: spec'teki "Sunucu" bölümü (önbellek süresi `refresh`, başarılı eylem/bırakmada plugin bazlı düşürme bütün profillerde, plan bağı yalnız gerçek listede, iki kutu kilidi aynı anda yok, `describe` hatasında her istekte yeniden deneme).

- [ ] **Step 1:** Testler: `items_within_refresh_come_from_cache` (fake plugin çağrı sayacı 1), `action_drops_cache_of_boxes_with_same_plugin`, `unsupported_action_is_refused_without_calling_plugin`, `unknown_profile_or_box_is_not_found`, `crashed_plugin_answers_plugin_then_recovers`, `plan_link_closes_task_on_server_after_two_empty_lists`, `concurrent_actions_on_one_box_are_serialized` (fake plugin aynı anda iki istek görürse hata verir), `capture_without_capture_into_is_invalid`.
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** Uygula. `fake-plugin`'e test için gereken davranış (çağrı sayacı, eşzamanlılık denetimi, çökme) eksikse `crates/fake-plugin/src/main.rs`'e eklenir.
- [ ] **Step 4:** Üç kontrol komutu temiz.
- [ ] **Step 5: Commit** `Add server board that runs plugins and caches lists`

### Task 6: HTTP katmanı ve `focusdeck server`

**Review:** per-task — security

**Files:**
- Create: `crates/focusdeck/src/server/http.rs`, `crates/focusdeck/tests/server.rs`
- Modify: `Cargo.toml`, `crates/focusdeck/Cargo.toml`, `crates/focusdeck/src/cli.rs`, `crates/focusdeck/src/main.rs`, `locales/en.ftl`, `tr.ftl`

**Interfaces:**
- Consumes: `Board`, `IdempotencyStore`, `secrets::read_token`, `api::*`.
- Produces: `server::http::router(board: Arc<Board>, keys: Arc<IdempotencyStore>, token: String) -> axum::Router`, `server::run(paths: &Paths, cfg: &Config) -> anyhow::Result<()>`, `cli::Command::Server`. Uçlar spec'teki tabloyla birebir.
- Kimlik: `Authorization: Bearer <t>`, iki tarafın SHA-256 özeti sabit zamanlı karşılaştırılır. Gövde sınırı 1 MiB. `drops` ve `captures`'ta `Idempotency-Key` varsa gövde baytlarının SHA-256'sıyla `claim`, plugin hatasında `release`.
- Açılışta tek satır (`msg::SERVER_LISTENING`, adres ile), silinen `in_progress` anahtar sayısı sıfırdan büyükse log satırı.

- [ ] **Step 1:** `tests/server.rs`, gerçek `CARGO_BIN_EXE_focusdeck server` süreci, geçici `FOCUSDECK_*_DIR`, iki `local` kutulu profil: `requests_without_valid_token_get_401` (başlıksız, yanlış token, `Bearer` öneksiz; gövde `{"error":{"code":"UNAUTHORIZED",…}}`, token gövdede yok), `profile_has_no_config_or_secret_paths`, `capture_then_list_shows_note`, `capture_with_same_key_twice_creates_one_note`, `same_key_other_body_is_key_reused`, `drop_between_local_boxes_moves_once`, `unknown_profile_is_404`, `server_without_serve_section_exits_with_error`.
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** Uygula.
- [ ] **Step 4:** Üç kontrol komutu temiz.
- [ ] **Step 5: Commit** `Serve the board over an authenticated REST API`

### Task 7: Uzak kutu ve istemci kipinde TUI

**Files:**
- Create: `crates/focusdeck/src/remote.rs`
- Modify: `crates/focusdeck/src/host.rs`, `worker.rs`, `ui/app.rs`, `ui/run.rs`, `main.rs`, `locales/*.ftl`

**Interfaces:**
- Consumes: `ServerConfig`, `read_token`, `api::*`.
- Produces: `RemoteClient::connect(cfg: &ServerConfig, home: &Path) -> Result<RemoteClient, RemoteError>` ve `profiles`, `profile`, `items`, `action`, `drop(…, key: Option<&str>)`, `capture(…, key: Option<&str>)`, `reminders` (hepsi `async`). `enum RemoteError { Network(String), Unauthorized, Api { code: ErrorCode, message: String } }`. `RemoteBox::new(client: Arc<RemoteClient>, profile: String, view: &BoxView)`. `host::BoxHost { Process(PluginHost), Remote(RemoteBox) }` ve `describe()`, `call<P, R>(method, params)` (`list`, `action`, `drop`, `reminders` uçlara eşlenir, `describe` hatasında profil yeniden çekilir). `HostError::Remote(String)`. `remote::resolve_profile(client, cli, env, cfg) -> anyhow::Result<String>`. `App::with_server_links(self) -> App` (plan bağını kapatır). `ui::run::Deck { Local(Profile), Remote { client: Arc<RemoteClient>, view: ProfileView } }`, `tui(paths, cfg, deck)`.

- [ ] **Step 1:** Testler (`mockito`): `remote_box_lists_items`, `remote_plugin_error_message_passes_through`, `unauthorized_names_server_address` (metinde token yok), `remote_box_network_error_is_red_line_then_recovers`, `resolve_profile_follows_flag_env_default_single` ; `app.rs`: `with_server_links_sends_no_closure_done`.
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** Uygula. Worker `BoxHost` sürer, `Process` yolunda davranış aynı. İstemci kipinde `Words::new(i18n::current(), &view.capture_words)`.
- [ ] **Step 4:** Üç kontrol komutu temiz. Mevcut `tests/host.rs`, `tests/commands.rs` değişmeden geçer.
- [ ] **Step 5: Commit** `Run the TUI against a focusdeck server`

### Task 8: İstemci kipinde `capture` ve `remind`

**Review:** per-task — persisted format

**Files:**
- Modify: `crates/focusdeck/src/queue.rs`, `capture_cmd.rs`, `remind.rs`, `main.rs`, `locales/*.ftl`

**Interfaces:**
- Consumes: `RemoteClient::capture`, `RemoteClient::reminders`, `fsio::unique_id`.
- Produces: `Queued.id: Option<String>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`), `Queued::new(text, at, lang) -> Queued` (`id` dolu), `Queued::key(&self) -> String` (`id` ya da `sha256(at.to_rfc3339() + "\n" + text)` hex).

- [ ] **Step 1:** Testler: `old_queue_line_without_id_parses_and_keeps_key` (iki okumada aynı anahtar), `remote_capture_sends_entry_key` (mockito `Idempotency-Key` başlığını eşler), `remote_invalid_capture_is_dropped_from_queue_and_notified`, `unreachable_server_keeps_note_in_queue`, `remote_remind_prints_due_reminders`.
- [ ] **Step 2:** Düştüğünü gör.
- [ ] **Step 3:** Uygula. Yerel kipte `capture` akışı birebir aynı kalır.
- [ ] **Step 4:** Üç kontrol komutu temiz.
- [ ] **Step 5: Commit** `Deliver captures and reminders through the server`

### Task 9: Belgeler

**Files:**
- Modify: `README.md`, `README.tr.md`, `crates/focusdeck/Cargo.toml` (sürüm `0.4.0`)

- [ ] **Step 1:** İki README'ye "Sunucu" bölümü: `[serve]`, `[server]`, `openssl rand -hex 32` ile token, Caddy örneği (`reverse_proxy 127.0.0.1:8740`), istemci kipinde neyin sunucudan geldiği, API tablosu ve hata kodları. "Ayarlar" bölümündeki anahtar listesine iki bölüm.
- [ ] **Step 2:** Üç kontrol komutu temiz. README'deki Türkçe metin `turkish-anti-ai-skill` denetiminden geçer.
- [ ] **Step 3: Commit** `Document the server and client mode`

---

## Plan B: kurulum (görev listesi)

Plan A birleşince ayrıntılı komutlarla ayrı plan dosyası olarak yazılır. Uzak hostta değişiklik yapan her komuttan önce host ve komut yazılır, pve11'e tek SSH bağlantısı.

1. Sürüm ikilisi: `rust:1-trixie` konteynerinde `cargo build --release --locked`, Debian 13 glibc'sine uygun olsun diye.
2. pve11'de yeni Debian 13 LXC (`pct create`'te `--description` yok), `focusdeck` kullanıcısı, systemd birimi (`Description=` yok, `User=focusdeck`, `Restart=on-failure`).
3. LXC'de `focusdeck plugin install vikunja redmine zulip gmail whatsapp`. `pano.toml` ve gizli dosyalar 0600 olarak taşınır, `local` kutulara `refresh = "30s"`. `[serve]` ve sunucu token'ı.
4. Ofis ve ev `local.json` birleştirilir (`jq`), kopya kimlik yok diye doğrulanır, LXC'ye konur.
5. Forgejo `Caddy` reposuna `deck.omertahaoztop.tr` vhost'u, DNS kaydı, CI ile dağıtım.
6. Masaüstlerine `[server]` ve token dosyası. Doğrulama: iki makinede aynı not görünür, birinde `x` ötekinde kaybolur, `capture` kuyruğu sunucu kapalıyken bekler.
7. Wiki: `journal_write kind=wiki` ile servis sayfası, GOWA sayfasına bağımlılık satırı.

## Plan C: Android (görev listesi)

Repo `focusdeck-android`. API'yi Plan A'nın `api.rs`'i belirler, Kotlin tipleri ona birebir uyar.

1. Proje iskeleti (Compose, Material 3, `minSdk 26`), GitHub Actions'ta test + imzalı APK sürümü.
2. API istemcisi (OkHttp, kotlinx.serialization, 20 sn zaman aşımı), hata kodları, Keystore + DataStore ile token saklama.
3. Kurulum ekranı (adres, token, bağlantı denemesi) ve profil seçimi.
4. Pano: kutular alt alta bölüm, sayı ve gecikmiş sayısı, aşağı çekip yenileme, öndeyken kutu başına `refresh` ile yoklama, hata satırı. Öğe ayrıntısı, linkleri tarayıcıda açma.
5. Bitirme (kaydırma + düğme, yalnız `done` destekleyen kutuda) ve not ekleme (yüzen düğme → `captures`, `Idempotency-Key` ile).
6. Sürükle-bırak: uzun basma, `accepts_drop` olan bölüme bırakma → `drops`, aynı kutuda `reorder`. Öğe menüsünde "Taşı".
7. Hatırlatıcı: WorkManager 15 dk + açılışta, geçmiş 12 saat hemen bildirim, 24 saat ileri AlarmManager tam alarm, gösterilenler cihazda.
8. Türkçe/İngilizce metinler, sürüm 0.1.0 APK'sı.

## Doğrulama

- Plan A: her task'ın testleri, sonunda `cargo test --workspace --locked` ve clippy. Elle: yerelde `focusdeck server` (`listen = 127.0.0.1:8740`) + ikinci bir `FOCUSDECK_CONFIG_DIR` ile `[server] url = "http://127.0.0.1:8740"` olan TUI. İki terminalde aynı profil açılır, birinde `a` ile not, ötekinde 30 sn içinde görünür, `x` ve sürükle-bırak iki tarafta tutarlı. Bir tmux ekran görüntüsüyle kanıtlanır. `[server]` silinince yerel `local.json` notları geri gelir.
- Plan B: `curl -s -o /dev/null -w '%{http_code}' https://deck.omertahaoztop.tr/api/v1/profiles` token'sız 401, token'lı 200. İki masaüstünde aynı pano ekran görüntüsü.
- Plan C: emülatörde yerel `focusdeck server`'a bağlanıp not ekleme, bitirme, sürükleyerek taşıma, masaüstü TUI'de aynı sonucu görme. Ekran görüntüleri.
