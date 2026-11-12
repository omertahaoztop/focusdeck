# focusdeck çekirdek + Vikunja plugin'i: uygulama planı (Plan 1/4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fare ve tek tuşla yönetilen, profil ve plugin destekli focusdeck çekirdeğini ve ilk gerçek plugin'i (Vikunja) yazıp bu makinede Sözlü işler + Fikirler kutularıyla çalıştırmak.

**Architecture:** `focusdeck` reposu bir Cargo workspace'idir: `focusdeck-protocol` (ortak tipler, `serve` yardımcısı), `focusdeck` (TUI ve alt komutlar), `fake-plugin` (yalnız testler için). Her kutu için plugin ayrı süreçtir, tokio görevi içinde satır başına JSON ile konuşulur, UI saf bir durum makinesidir (`App`) ve yan etkileri `Effect` olarak döndürür. `focusdeck-plugins` reposu Vikunja plugin'ini taşır, protokol crate'ine yerel yol ile bağlanır.

**Tech Stack:** Rust 2024 (rustc 1.99), ratatui 0.30, crossterm 0.29, tokio 1, serde/serde_json, toml 1, reqwest 0.13 (rustls), sha2 0.11, clap 4, chrono 0.4, unicode-width, thiserror 2, anyhow 1; testlerde tempfile 3, mockito 1.

**Spec:** `docs/superpowers/specs/2026-11-12-focusdeck-design.md`

**Sonraki planlar:** Plan 2 redmine, gmail, zulip plugin'leri ve GitHub yayını (repolar, sürüm iş akışı, `index.toml`, protokol bağımlılığının git etiketine geçmesi). Plan 3 GOWA LXC ve whatsapp plugin'i. Plan 4 geçiş (F9, zamanlayıcı, şifre yenileme, `pano` emekliliği). Bu plan kısayollara, systemd birimlerine ve `pano`'ya dokunmaz.

## Global Constraints

- Linux x86_64 ve aarch64. Windows/macOS kodu yazılmaz.
- `edition = "2024"`, workspace `resolver = "3"`.
- Kullanıcıya görünen her metin Türkçe ve tam karakterli (ç ğ ı İ ö ş ü). Kod tanımlayıcıları İngilizce.
- Yorum yalnız kodun gösteremediği neden için, tek satır. Dosya başlığı en fazla tek satır. Bölüm ayırıcı yorum yok.
- Commit: İngilizce, imperatif, tek satır ≤72 karakter, gövde yok, önek yok (`Add plugin host`).
- Şifre değeri hiçbir yerde loglanmaz, yazdırılmaz, hata mesajına girmez. İstek gövdeleri loglanmaz.
- Dizinler: ayar `~/.config/focusdeck`, plugin'ler `~/.local/share/focusdeck/plugins`, durum `~/.local/state/focusdeck`. Testler `FOCUSDECK_CONFIG_DIR`, `FOCUSDECK_DATA_DIR`, `FOCUSDECK_STATE_DIR` ile geçersiz kılar.
- Protokol sürümü `1`, istek zaman aşımı 15 saniye, varsayılan yenileme `2m`.
- Varsayılan dizin adresi `https://raw.githubusercontent.com/omertahaoztop/focusdeck-plugins/main/index.toml`.
- Her task sonunda `cargo test --workspace` ve `cargo clippy --workspace --all-targets -- -D warnings` temiz.

## Review Focus

1. Plugin stdout'a JSON olmayan bir satır basar (hata ayıklama çıktısı): o istek kutuda hata olur, çekirdek çökmez, plugin yeniden başlatılır. Test: Task 4 `garbage_line_is_error_and_restarts`.
2. Terminal çok küçük (20x5) ya da yeniden boyutlanır: çizim panik atmaz. Test: Task 8 `renders_in_tiny_terminal`.
3. Uzun Türkçe ve emoji içeren başlık: görüntü genişliğine göre kesilir, karakter sınırında panik yok. Test: Task 8 `truncate_respects_display_width`.
4. Saat sınırları: geçmiş `23:59` yarına, aynı gün geçmiş `pzt 09:00` bir hafta sonrasına düşer. Test: Task 3 `past_time_rolls_to_tomorrow`, `same_weekday_past_rolls_a_week`.
5. Dizinde plugin var ama bu makinenin hedefi yok: açık hata, diske dosya yazılmaz. Test: Task 6 `missing_target_writes_nothing`.

---

### Task 1: Workspace ve protokol crate'i

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `crates/focusdeck-protocol/Cargo.toml`, `crates/focusdeck-protocol/src/lib.rs`, `crates/focusdeck-protocol/src/types.rs`, `crates/focusdeck-protocol/src/serve.rs`
- Test: `crates/focusdeck-protocol/tests/serve.rs`

**Interfaces:**
- Produces (`focusdeck_protocol`):
  - `pub const PROTOCOL_VERSION: u32 = 1;`
  - `Request { id: u64, method: String, params: serde_json::Value }` (`params` varsayılan `Null`)
  - `Response { id: u64, result: Option<Value>, error: Option<RpcError> }`, `RpcError { message: String }`; `None` alanlar serileştirilmez
  - `Describe { name: String, version: String, protocol: u32, actions: Vec<String>, accepts_drop: bool, reminders: bool }`
  - `Item { id: String, title: String, subtitle: Option<String>, url: Option<String>, due: Option<DateTime<FixedOffset>>, overdue: bool, source: Option<Source>, description: Option<String>, labels: Vec<String>, from: Option<String>, links: Vec<Link> }`; `Vec` alanlar `#[serde(default)]`, `Option` alanlar eksikse `None`
  - `Source { plugin: String, id: String }`, `Link { label: String, url: String }`
  - `View` (`#[serde(rename_all = "lowercase")]`: `Summary`, `Full`), `ListParams { view: View }`, `ListResult { items: Vec<Item> }`
  - `ActionParams { item: String, action: String, args: Value }`, `DropParams { item: Item, from: Option<Source> }`, `RemindersParams { since: DateTime<FixedOffset> }`, `Reminder { at: DateTime<FixedOffset>, item: Item }`, `RemindersResult { reminders: Vec<Reminder> }`
  - `pub trait Plugin { fn describe(&self) -> Describe; fn configure(&mut self, config: Value) -> Result<(), String>; fn list(&mut self, p: ListParams) -> Result<Vec<Item>, String>; fn action(&mut self, p: ActionParams) -> Result<Option<Item>, String>; fn drop_item(&mut self, p: DropParams) -> Result<Item, String> { Err("bu kutu bırakılan öğe kabul etmiyor".into()) } fn reminders(&mut self, p: RemindersParams) -> Result<Vec<Reminder>, String> { Ok(vec![]) } }`
  - `pub fn serve<P: Plugin>(plugin: P, input: impl BufRead, output: impl Write) -> std::io::Result<()>`

- [ ] **Step 1: Workspace iskeleti**

`Cargo.toml` workspace üyeleri: `crates/focusdeck-protocol`, `crates/focusdeck`, `crates/fake-plugin` (son ikisi sonraki task'larda eklenir, şimdilik yalnız protokol). `[workspace.package] edition = "2024"`, `license = "MIT"`. `[workspace.dependencies]` Tech Stack'teki sürümler. `.gitignore`: `/target`, `.superpowers/`.

- [ ] **Step 2: Başarısız testleri yaz**

```rust
// tests/serve.rs
#[test] fn describe_roundtrip()          // serve'e {"id":1,"method":"describe"} ver, cevapta result.protocol == 1, result.name == "deneme"
#[test] fn unknown_method_is_error()     // {"id":2,"method":"yok"} → error.message == "bilinmeyen yöntem: yok", result yok
#[test] fn bad_json_line_is_error_with_id_zero() // "{bozuk" → {"id":0,"error":{"message": "geçersiz istek: ..."}} ile başlar
#[test] fn plugin_error_is_forwarded()   // list Err("401") döndüren plugin → error.message == "401"
#[test] fn item_omits_nothing_required() // Item'ı JSON'a çevir, "labels":[] ve "overdue":false var; {"id":"1","title":"a"} çözülür, labels boş
#[test] fn drop_default_message()        // drop_item uygulamayan plugin → error.message == "bu kutu bırakılan öğe kabul etmiyor"
```

Testteki plugin `Deneme` adında, `Plugin`'i uygulayan küçük bir yapı. Girdi `Cursor<&[u8]>`, çıktı `Vec<u8>`; her cevap satırı ayrı ayrı `serde_json::from_str::<Response>` ile okunur.

- [ ] **Step 3: Çalıştır, derleme hatasıyla düştüğünü gör**

Run: `cargo test -p focusdeck-protocol`
Expected: FAIL (`Plugin`, `serve` tanımsız)

- [ ] **Step 4: Tipleri ve `serve`'i yaz**

`serve` her satırı `Request` olarak çözer, yöntem adına göre params tipini çözüp trait yöntemini çağırır, cevabı tek satır JSON yazıp `flush` eder, EOF'ta `Ok(())` döner. Params çözülemezse `error: "geçersiz parametre: <serde hatası>"`.

- [ ] **Step 5: Testler geçsin**

Run: `cargo test -p focusdeck-protocol` → PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore crates/focusdeck-protocol
git commit -m "Add workspace and plugin protocol crate"
```

### Task 2: Dizinler, ayar, profil ve şifre dosyası denetimi

**Review:** per-task — security

**Files:**
- Create: `crates/focusdeck/Cargo.toml`, `crates/focusdeck/src/main.rs` (şimdilik boş `fn main`), `src/paths.rs`, `src/config.rs`, `src/profile.rs`, `src/secrets.rs`, `src/duration.rs`
- Test: aynı dosyalarda `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `paths::Paths { config_dir: PathBuf, data_dir: PathBuf, state_dir: PathBuf }`, `Paths::from_env() -> Paths`, `fn profile_path(&self, name: &str) -> PathBuf` (`config_dir/profiles/<name>.toml`), `fn plugin_dir(&self) -> PathBuf`, `fn log_file(&self) -> PathBuf` (`state_dir/focusdeck.log`)
  - `config::Config { default_profile: Option<String>, registries: Vec<String>, capture_command: Vec<String> }`, `Config::load(paths: &Paths) -> anyhow::Result<Config>` (dosya yoksa varsayılanlar; `capture_command` varsayılanı `["rofi", "-dmenu", "-p", "Not"]`)
  - `profile::Profile { name, capture_into: Option<String>, capture_refs: BTreeMap<String, String>, capture_label_aliases: BTreeMap<String, String>, boxes: Vec<BoxSpec> }` (TOML'da `[[box]]`)
  - `profile::BoxSpec { id, title, plugin: String, row: u16, col: u16, width: u16 (varsayılan 1), height: u16 (varsayılan 1), refresh: Duration (varsayılan 2m, TOML'da "2m"), plan_into: Option<String>, config: toml::Table }`
  - `profile::load(path: &Path) -> Result<Profile, ProfileError>`, `profile::select_name(cli: Option<&str>, env: Option<&str>, cfg: &Config) -> Result<String, ProfileError>`
  - `Profile::box_index(&self, id: &str) -> Option<usize>`
  - `secrets::plugin_config(spec: &BoxSpec, home: &Path) -> Result<serde_json::Value, SecretError>`
  - `duration::parse(s: &str) -> Result<Duration, String>` (`"90s"`, `"2m"`, `"1h"`)

- [ ] **Step 1: Başarısız testleri yaz**

```rust
// duration
parse("90s") == 90s; parse("2m") == 120s; parse("1h") == 3600s; parse("2") is Err; parse("0m") is Err

// profile::load (tempfile ile yazılan TOML'lar)
#[test] fn loads_example_profile()      // spec'teki iş profili örneği: 2 kutu, refresh sozlu=60s redmine=120s, width/height 1
#[test] fn rejects_duplicate_box_id()    // hata metni "aynı kutu kimliği iki kez: sozlu"
#[test] fn rejects_unknown_capture_into()// "capture_into bilinmeyen kutuya bakıyor: yok"
#[test] fn rejects_plan_into_self()      // "redmine kutusu kendine plan_into veremez"
#[test] fn rejects_overlapping_cells()   // (0,0) w=2 ile (0,1): "sozlu ve fikir kutuları aynı hücrede: satır 0 sütun 1"

// select_name
cli Some("ev") kazanır; cli None env Some("is") → "is"; ikisi None, cfg.default_profile Some("is") → "is";
hepsi None → Err metni "profil seçilmedi: --profile ver ya da config.toml'a default_profile yaz"

// secrets::plugin_config
#[test] fn expands_tilde_in_file_keys()  // token_file="~/s/v" → home/s/v; url değişmez
#[test] fn rejects_group_readable_secret() // 0640 dosya → Err, metin "<yol> dosyasını başkaları okuyabiliyor, chmod 600 yap"
#[test] fn rejects_missing_secret()      // "<yol> bulunamadı"
#[test] fn accepts_0600_secret()         // Ok, JSON'da genişletilmiş yol; dosya içeriği JSON'da yok
```

- [ ] **Step 2: Çalıştır, düştüğünü gör**

Run: `cargo test -p focusdeck` → FAIL

- [ ] **Step 3: Modülleri yaz**

`plugin_config` `config` tablosunu JSON'a çevirir; adı `_file` ile biten her string değerde baştaki `~/`'i `home` ile değiştirir, `std::fs::metadata` ile varlığını ve `mode() & 0o077 == 0` koşulunu denetler. Dosyayı okumaz. Hücre çakışması `row..row+height` × `col..col+width` kümeleriyle bulunur.

- [ ] **Step 4: Testler geçsin**

Run: `cargo test -p focusdeck` → PASS

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck Cargo.toml
git commit -m "Add paths, config, profile and secret file checks"
```

### Task 3: Dışarıdan not ayrıştırıcı ve kuyruk

**Files:**
- Create: `crates/focusdeck/src/capture.rs`, `crates/focusdeck/src/queue.rs`

**Interfaces:**
- Consumes: `Profile` (Task 2), `Item`, `Link` (Task 1)
- Produces:
  - `capture::Capture { title: String, description: Option<String>, from: Option<String>, labels: Vec<String>, links: Vec<Link>, due: Option<DateTime<FixedOffset>> }`
  - `capture::parse(text: &str, now: DateTime<FixedOffset>, refs: &BTreeMap<String, String>, aliases: &BTreeMap<String, String>) -> Result<Capture, String>`
  - `Capture::into_item(self) -> Item` (`id` boş, `overdue` false)
  - `queue::Queue::new(path: PathBuf)`, `fn push(&self, text: &str, at: DateTime<FixedOffset>) -> io::Result<()>`, `fn take_all(&self) -> io::Result<Vec<Queued>>`, `fn put_back(&self, items: &[Queued]) -> io::Result<()>`; `Queued { text: String, at: DateTime<FixedOffset> }`, dosya `state_dir/capture-queue.jsonl`, satır başına `{"text":…,"at":…}`

- [ ] **Step 1: Başarısız testleri yaz**

`now` = `2026-11-12T10:00:00+03:00` (Perşembe). `refs = {R: "https://destek.mono.tr/issues/{}"}`, `aliases = {tel: telefon}`.

```rust
parse("Ali'ye VPN logu at", ..) → title "Ali'ye VPN logu at", due None
parse("log at 30dk") → due 10:30
parse("log at 2s") → due 12:00
parse("log at 14:30") → due bugün 14:30
#[test] fn past_time_rolls_to_tomorrow() // parse("log at 09:15") → 2026-11-13T09:15
parse("log at yarın 09:00") ve "yarin 09:00" → 2026-11-13T09:00
parse("log at pzt 09:00") → 2026-11-16T09:00
#[test] fn same_weekday_past_rolls_a_week() // parse("x per 09:00") → 2026-11-19T09:00
parse("@ali: log at") → from Some("ali"), title "log at"; ikinci @veli başlıkta kalır
parse("log #tel #mail") → labels ["telefon", "mail"]
parse("bak #123") → title "bak #123", labels []
parse("S3 R45539 bak") → links [Link{label:"R45539", url:"https://destek.mono.tr/issues/45539"}], title "S3 bak"
parse("S3 R4 bak") → link yok, title "S3 R4 bak"
parse("başlık // ayrıntı burada") → description Some("ayrıntı burada")
parse("@ali 14:30") → Err("başlık boş")
parse("log 25:00") → due None, title "log 25:00"

// queue (tempdir)
push iki kez, take_all iki kayıt sırayla döner ve dosya silinir; put_back bir kayıt yazar; boş dosyada take_all boş döner
```

- [ ] **Step 2: Düştüğünü gör** — `cargo test -p focusdeck capture queue` → FAIL

- [ ] **Step 3: Yaz**

Kurallar spec'in "Dışarıdan not alma" bölümündeki sırayla; gün tablosu `pzt|pts:0 sal:1 çar|car:2 per|prş|prs:3 cum:4 cmt:5 paz:6`. Saat deseni `([01]?\d|2[0-3]):([0-5]\d)`, süre deseni `(\d+)(dk|s)`. Ref eşleşmesi: kelime bir `capture_refs` anahtarıyla başlar ve geri kalanı en az iki rakamdır.

- [ ] **Step 4: Geçsin** — PASS

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck/src/capture.rs crates/focusdeck/src/queue.rs crates/focusdeck/src/main.rs
git commit -m "Add capture parser and offline queue"
```

### Task 4: Plugin süreci ve kutu işçisi

**Review:** per-task — concurrency

**Files:**
- Create: `crates/fake-plugin/Cargo.toml` (`publish = false`), `crates/fake-plugin/src/main.rs`, `crates/focusdeck/src/host.rs`, `crates/focusdeck/src/worker.rs`
- Test: `crates/focusdeck/tests/host.rs`

**Interfaces:**
- Consumes: protokol tipleri (Task 1)
- Produces:
  - `host::HostError` (thiserror): `Spawn(String)`, `Timeout`, `Crashed`, `BadLine(String)`, `Plugin(String)`, `Protocol { expected: u32, got: u32 }`. `Display` metinleri: `"plugin başlatılamadı: {0}"`, `"plugin 15 sn içinde cevap vermedi"`, `"plugin kapandı"`, `"plugin geçersiz satır yazdı: {0}"`, `"{0}"`, `"protokol uyuşmuyor: çekirdek {expected}, plugin {got}"`
  - `host::PluginHost::new(label: String, exe: PathBuf, config: Value, log: PathBuf) -> PluginHost`
  - `PluginHost::with_timeout(self, d: Duration) -> Self`
  - `async fn PluginHost::describe(&mut self) -> Result<Describe, HostError>` (gerekirse başlatır, protokol denetler, `configure` gönderir, sonucu saklar)
  - `async fn PluginHost::call<P: Serialize, R: DeserializeOwned>(&mut self, method: &str, params: &P) -> Result<R, HostError>` (süreç yoksa önce `describe` adımlarını yapar)
  - `worker::BoxCmd { Refresh(View), Action { item: String, action: String, args: Value }, Drop { item: Item, from: Option<Source> } }`
  - `worker::BoxEvent { Described { idx: usize, describe: Describe }, Listed { idx: usize, view: View, result: Result<Vec<Item>, String> }, Finished { idx: usize, what: String, result: Result<(), String> } }`
  - `worker::spawn(idx: usize, host: PluginHost, rx: mpsc::Receiver<BoxCmd>, tx: mpsc::Sender<BoxEvent>) -> JoinHandle<()>`

- [ ] **Step 1: Sahte plugin**

`fake-plugin` `focusdeck_protocol::serve` kullanır. Davranışı `FAKE_MODE` ortam değişkeninden alır: `ok` (2 öğe listeler, drop'ta öğeyi `id = "yeni"` ile döner), `hang` (list'te 60 sn uyur), `crash` (list'te `process::exit(3)`), `garbage` (list'te stdout'a `merhaba` yazıp çıkar), `oldproto` (`protocol: 0` bildirir), `stderr` (list'te stderr'e `gizli-değil` yazar). Her `configure` çağrısını `FAKE_LOG` dosyasına bir satır olarak ekler.

- [ ] **Step 2: Başarısız testleri yaz**

Sahte plugin başka pakette olduğu için `env!("CARGO_BIN_EXE_fake-plugin")` kullanılamaz. `tests/host.rs` içindeki `fn fake_plugin() -> PathBuf` yardımcısı bir kez `cargo build -p fake-plugin` çalıştırır ve `<workspace>/target/debug/fake-plugin` yolunu döndürür (`CARGO_TARGET_DIR` tanımlıysa onu kullanır). Kısa zaman aşımı için `with_timeout(500ms)`.

```rust
#[tokio::test] async fn lists_items()                    // ok: call::<_, ListResult>("list", Summary) → 2 öğe
#[tokio::test] async fn timeout_is_reported()             // hang: Err(HostError::Timeout)
#[tokio::test] async fn crash_restarts_on_next_call()     // crash: Err(Crashed); FAKE_MODE dosyadan okunur, ok'a çevrilir, sonraki call Ok ve FAKE_LOG'da 2 configure satırı
#[tokio::test] async fn garbage_line_is_error_and_restarts() // garbage: Err(BadLine("merhaba")), sonraki call yeni süreçle denenir
#[tokio::test] async fn old_protocol_rejected()           // oldproto: describe → Err(Protocol{expected:1, got:0})
#[tokio::test] async fn stderr_goes_to_log()              // stderr: log dosyasında "[sozlu] gizli-değil" satırı
#[tokio::test] async fn worker_emits_listed()             // spawn + Refresh(Summary) → BoxEvent::Described sonra Listed{result: Ok(2 öğe)}
#[tokio::test] async fn worker_survives_plugin_error()    // crash modunda Refresh → Listed{result: Err("plugin kapandı")}, ardından gelen ikinci Refresh de bir Listed üretir
```

`FAKE_MODE` testin içinde değiştirilebilsin diye sahte plugin modu her istekte `FAKE_MODE_FILE` dosyasından okur, dosya yoksa `FAKE_MODE` değişkenine bakar.

- [ ] **Step 3: Düştüğünü gör** — `cargo build -p fake-plugin && cargo test -p focusdeck --test host` → FAIL

- [ ] **Step 4: Yaz**

`tokio::process::Command`, `stdin`/`stdout` piped, `stderr` piped ve ayrı bir görevde `[label] ` önekiyle log dosyasına satır satır eklenir. `kill_on_drop(true)`. İstek kimliği artan sayaç; cevap satırının `id`'si eşleşmezse `BadLine`. Her hata türünde (Plugin hariç) süreç öldürülür ve `None` yapılır. İşçi komutları sırayla işler; bir kutunun takılması diğer kutuları bekletmez çünkü her kutunun kendi görevi var.

- [ ] **Step 5: Geçsin** — PASS

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/fake-plugin crates/focusdeck/src/host.rs crates/focusdeck/src/worker.rs crates/focusdeck/tests/host.rs
git commit -m "Add plugin host, box worker and fake test plugin"
```

### Task 5: Kutular arası bağ kuralı

**Review:** per-task — destructive

**Files:**
- Create: `crates/focusdeck/src/links.rs`

**Interfaces:**
- Consumes: `Item` (Task 1)
- Produces:
  - `links::linked_ids(target_items: &[Item], source_plugin: &str) -> HashSet<String>`
  - `links::linked_item<'a>(target_items: &'a [Item], source_plugin: &str, source_id: &str) -> Option<&'a Item>`
  - `links::items_to_close(source_plugin: &str, source_items: &[Item], target_items: &[Item]) -> Vec<String>` (hedef öğe kimlikleri)

- [ ] **Step 1: Başarısız testleri yaz**

```rust
linked_ids: hedefte source redmine:1, redmine:2, zulip:9 → redmine için {"1","2"}
linked_item(hedef, "redmine", "2") → source'u redmine:2 olan öğe
items_to_close: kaynak [1], hedef [redmine:1 (id a), redmine:2 (id b), source yok (id c)] → ["b"]
items_to_close: kaynak boş liste, hedefte bağlı öğe yok → []
items_to_close: başka plugin'den bağlı öğe (zulip:5) kapanmaz
```

- [ ] **Step 2: Düştüğünü gör**, **Step 3: Yaz**, **Step 4: Geçsin**

`items_to_close` yalnız başarılı listeyle çağrılır; hata durumunu çağıran taraf ayırır (Task 7 `App::on_event` yalnız `Ok` sonuçta çağırır).

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck/src/links.rs crates/focusdeck/src/main.rs
git commit -m "Add plan link rules"
```

### Task 6: Plugin kurulumu ve `plugin` alt komutları

**Review:** per-task — security

**Files:**
- Create: `crates/focusdeck/src/install.rs`, `crates/focusdeck/src/cli.rs`
- Modify: `crates/focusdeck/src/main.rs`
- Test: `crates/focusdeck/tests/install.rs`

**Interfaces:**
- Consumes: `Paths`, `Config`, `profile::load` (Task 2), `PROTOCOL_VERSION`
- Produces:
  - `install::Index { plugin: Vec<IndexEntry> }`, `IndexEntry { name, description, version: String, protocol: u32, targets: BTreeMap<String, Target> }`, `Target { url: String, sha256: String }`
  - `install::current_target() -> &'static str` (`x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu`, `cfg(target_arch)`)
  - `async fn install::fetch(url: &str) -> anyhow::Result<Vec<u8>>` (`http(s)://` reqwest, `file://` dosya)
  - `install::verify_sha256(bytes: &[u8], hex: &str) -> anyhow::Result<()>`
  - `async fn install::install(registries: &[String], name: &str, plugin_dir: &Path) -> anyhow::Result<IndexEntry>`; çalıştırılabilir `plugin_dir/<name>` (0755), yanında `plugin_dir/<name>.toml` (`version`, `sha256`, `registry`)
  - `install::installed(plugin_dir: &Path) -> Vec<(String, String)>` (ad, sürüm)
  - `install::remove(plugin_dir: &Path, name: &str) -> anyhow::Result<()>`
  - `cli::Cli` (clap derive): `--profile <ad>` genel bayrak; alt komutlar `capture [METİN]...`, `remind [--send]`, `plugin install <ad>`, `plugin list`, `plugin remove <ad>`; alt komut yoksa TUI

- [ ] **Step 1: Başarısız testleri yaz**

Testler `file://` dizinleri kullanır, ağ yok. `tempdir` içine sahte ikili (`#!/bin/sh` içerikli dosya) ve onun sha256'sıyla `index.toml` yazılır.

```rust
#[tokio::test] async fn installs_and_verifies()      // dosya 0755, içerik aynı, yanında .toml version "0.1.0"
#[tokio::test] async fn checksum_mismatch_writes_nothing() // yanlış sha → Err metni "sağlama tutmadı: redmine", plugin_dir/redmine yok
#[tokio::test] async fn missing_target_writes_nothing()    // yalnız "riscv64gc-unknown-linux-gnu" hedefi → Err "redmine için bu makineye uygun sürüm yok (x86_64-unknown-linux-gnu)", dosya yok
#[tokio::test] async fn first_registry_wins()        // iki dizinde aynı ad, farklı url → ilk dizinin dosyası kurulur
#[tokio::test] async fn unknown_plugin_error()       // "dizinlerde böyle bir plugin yok: yok"
#[tokio::test] async fn protocol_mismatch_rejected() // protocol = 2 → "redmine protokol 2 istiyor, çekirdek 1 biliyor"
#[test] fn remove_deletes_binary_and_manifest()
#[test] fn installed_lists_name_and_version()
```

- [ ] **Step 2: Düştüğünü gör**, **Step 3: Yaz**

İndirilen içerik önce `plugin_dir/.<name>.part` dosyasına yazılır, sağlama tutarsa `rename` edilir; tutmazsa `.part` silinir. `plugin remove`, adı herhangi bir profilde `plugin = "<ad>"` olarak geçiyorsa silmeden önce stderr'e `uyarı: <profil> profili bu plugin'i kullanıyor` yazar ve siler. `plugin list` her kurulu plugin için `ad  sürüm  (dizinde: son sürüm)` satırı basar; dizine ulaşılamazsa son sütun `dizine ulaşılamadı` olur.

- [ ] **Step 4: Geçsin**, ardından elle: `cargo run -p focusdeck -- plugin list` → boş liste, çıkış 0

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck
git commit -m "Add plugin install, list and remove"
```

### Task 7: Uygulama durumu (saf UI mantığı)

**Files:**
- Create: `crates/focusdeck/src/ui/mod.rs`, `src/ui/app.rs`, `src/ui/hint.rs`

**Interfaces:**
- Consumes: `Profile`, `BoxSpec` (Task 2), `Item`, `Describe`, `View`, `Source` (Task 1), `BoxEvent` (Task 4), `links::*` (Task 5)
- Produces:
  - `ui::app::BoxState { spec: BoxSpec, describe: Option<Describe>, items: Vec<Item>, error: Option<String>, selected: usize, scroll: usize }`
  - `ui::app::Mode { Board, Zoom(usize), Capture { input: String }, MovePicker { from: usize, item: String }, Search { zoom: usize, query: String }, Help }`
  - `ui::app::Hit { box_idx: usize, row: Option<usize> }` (`row` görünen listedeki sıra)
  - `ui::app::Effect { Refresh(usize, View), Action { idx: usize, item: String, action: String, args: Value }, Drop { idx: usize, item: Item, from: Option<Source> }, Open(String), Capture(String), Quit }`
  - `App::new(profile: &Profile) -> App`; alanlar `boxes: Vec<BoxState>`, `focus: usize`, `mode: Mode`, `flash: Option<String>`
  - `App::on_key(&mut self, key: KeyEvent) -> Vec<Effect>`
  - `App::on_mouse(&mut self, ev: MouseEvent, now: Instant, hit: impl Fn(u16, u16) -> Option<Hit>) -> Vec<Effect>`
  - `App::on_event(&mut self, ev: BoxEvent) -> Vec<Effect>`
  - `App::visible(&self, idx: usize) -> Vec<&Item>`
  - `App::planned_count(&self, idx: usize) -> Option<(usize, usize)>` (planlı, toplam; `plan_into` yoksa `None`)
  - `App::is_planned(&self, idx: usize, item: &Item) -> bool`
  - `hint::text(app: &App) -> String`

- [ ] **Step 1: Başarısız testleri yaz**

Testlerde üç kutulu profil: `sozlu` (vikunja, describe.actions `["done","delete","move","reorder"]`, accepts_drop), `redmine` (redmine, `plan_into = "sozlu"`, actions boş), `zulip` (zulip, actions `["unstar"]`). Öğeler `BoxEvent::Listed` ile yüklenir.

```rust
// görünürlük ve bağ
redmine Board'da visible → yalnız sozlu'da source redmine:<id> olanlar; Zoom(redmine)'da hepsi
planned_count(redmine) == Some((1, 3))
Search{zoom: redmine, query:"tüyap"} → başlığında "TÜYAP" geçen öğe (Türkçe büyük/küçük harf duyarsız: "İ"/"i", "I"/"ı")
redmine Full listesi Ok gelince, sozlu'da bağlı ama artık listede olmayan öğe için Effect::Action{idx: sozlu, action:"done"}
redmine listesi Err gelince hiçbir Action üretilmez, eski öğeler kalır, error Some
Summary listesi Ok gelince bağ kapatma yapılmaz (yalnız Full liste tam listedir)

// tuşlar
'a' → Mode::Capture; harf yaz; Enter → Effect::Capture("yazılan"), Mode::Board; Esc → Board, efekt yok
'x' sozlu'da → Action{action:"done", item: seçili id}; zulip'te 'x' → efekt yok, flash "bu kutuda bitirme yok"
'u' zulip'te → Action{action:"unstar"}
'o' url'li öğede → Open(url); url yoksa flash "bu öğenin linki yok"
Enter Board'da → Zoom(focus) ve Effect::Refresh(focus, Full); Esc → Board
' ' Zoom(redmine)'da planlı olmayan öğe → Drop{idx: sozlu, item, from: Some(Source{plugin:"redmine", id})}
' ' Zoom(redmine)'da planlı öğe → Action{idx: sozlu, item: bağlı görevin id'si, action:"delete"}
'm' → MovePicker; '1' → ilk uygun hedef kutuya Drop (kaynak kutu ve accepts_drop false kutular listede yok)
Tab → focus sonraki kutu, sonda başa döner; '?' → Help; 'q' → Quit
Down/Up ve tekerlek seçimi/kaydırmayı listenin sınırında durdurur, boş listede panik yok

// fare (hit fonksiyonu testte sabit tablo)
tıklama → focus ve selected değişir
aynı öğeye 400 ms içinde iki tıklama → Zoom; 500 ms arayla → Zoom değil
zulip öğesini sozlu kutusuna sürükle bırak → Drop{idx: sozlu, from: Some(zulip:<id>)}
sozlu içinde 0. öğeyi 2. satıra sürükle → Action{action:"reorder", args: {"position": 2}}
accepts_drop false kutuya bırakma → efekt yok, flash "redmine kutusu bırakılan öğe kabul etmiyor"

// ipucu metni (birebir)
Board, focus sozlu: "a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · x bitti · o aç · m taşı · Tab kutu · ? yardım · q çık"
Board, focus redmine: "a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · o aç · m taşı · Tab kutu · ? yardım · q çık"
Zoom(redmine): "boşluk planıma al · o aç · / ara · m taşı · Esc geri"
Capture: "Enter kaydet · Esc vazgeç · @kişi #etiket R123 · sonda 14:30, yarın 09:00, 30dk"
MovePicker: "1-9 hedef kutu · Esc vazgeç"
Search: "yaz ara · Enter tamam · Esc temizle"
```

İpucu kuralı: `x bitti` yalnız `done`, `u yıldızı kaldır` yalnız `unstar`, `boşluk planıma al` yalnız `plan_into` olan kutuda görünür. Board, focus zulip: `"a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · u yıldızı kaldır · o aç · m taşı · Tab kutu · ? yardım · q çık"`.

- [ ] **Step 2: Düştüğünü gör**, **Step 3: Yaz**, **Step 4: Geçsin**

Türkçe duyarsız karşılaştırma için `fn fold_tr(s: &str) -> String` (`İ→i`, `I→ı`, sonra `to_lowercase`) `app.rs` içinde.

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck/src/ui crates/focusdeck/src/main.rs
git commit -m "Add UI state machine with keys, mouse and drag"
```

### Task 8: Çizim, yerleşim ve TUI döngüsü

**Files:**
- Create: `crates/focusdeck/src/ui/layout.rs`, `src/ui/render.rs`, `src/ui/run.rs`
- Modify: `crates/focusdeck/src/main.rs`

**Interfaces:**
- Consumes: `App`, `Effect`, `Hit`, `hint::text` (Task 7), `PluginHost`, `worker::spawn` (Task 4), `capture::parse` (Task 3), `secrets::plugin_config`, `Paths`, `Config`, `profile` (Task 2)
- Produces:
  - `layout::grid(area: Rect, boxes: &[BoxSpec]) -> Vec<Rect>` (satır/sütun sayısı en büyük `row+height`/`col+width`, eşit pay)
  - `layout::hit(rects: &[Rect], app: &App, x: u16, y: u16) -> Option<Hit>`
  - `render::truncate(s: &str, width: usize) -> String` (görüntü genişliği, kesilirse sonda `…`)
  - `render::draw(frame: &mut Frame, app: &App)`
  - `async fn run::tui(paths: &Paths, cfg: &Config, profile: Profile) -> anyhow::Result<()>`

- [ ] **Step 1: Başarısız testleri yaz** (`ratatui::backend::TestBackend`)

```rust
grid: 6 kutulu 2x3 profil, 120x40 alan → 6 dikdörtgen, hiçbiri çakışmaz, toplam genişlik 120
grid: width=2 kutu iki sütunu kaplar
#[test] fn truncate_respects_display_width() // truncate("İğüşçö 🚀 çok uzun başlık", 10) görüntü genişliği ≤ 10 ve "…" ile biter; truncate("kısa", 10) == "kısa"
#[test] fn renders_in_tiny_terminal()        // 20x5 TestBackend'de draw panik atmaz
başlık: 3 öğe, 1 gecikmiş → kutu başlığında "Sözlü işler 3 · 1 gecikmiş"
plan kutusu Board'da başlık "Redmine planım 1 / 3"
hata durumunda kutuda kırmızı "hata: plugin kapandı" satırı ve eski öğeler soluk
gecikmiş öğe satırı kırmızı (Style fg Red)
Zoom'da planlı öğe "★ " önekiyle
son satır hint::text ile birebir aynı
üst satır "focusdeck · iş · " ile başlar
```

- [ ] **Step 2: Düştüğünü gör**, **Step 3: Yaz**

`run::tui`: crossterm raw mode + alternate screen + `EnableMouseCapture`; çıkışta ve panikte geri alınır (panic hook). Her kutu için `PluginHost` (`exe = paths.plugin_dir()/<plugin>`, ikili yoksa kutuda `hata: <plugin> kurulu değil, focusdeck plugin install <plugin>`), `worker::spawn`, kutunun `refresh` aralığında `Refresh` gönderen `tokio::time::interval`. Ana döngü `tokio::select!` ile crossterm `EventStream`, `BoxEvent` kanalı ve 1 sn'lik saat tikini dinler. `Effect::Open` `xdg-open` başlatır, `Effect::Capture(text)` `capture::parse` → `Drop` olarak `capture_into` kutusuna gider, parse hatası `flash`'a yazılır. Her başarılı `Action`/`Drop` sonrası ilgili kutulara `Refresh` gönderilir. `plan_into` kutuları her zaman `Full`, diğerleri Board'da `Summary`, Zoom'da `Full` ister.

- [ ] **Step 4: Geçsin**, ardından elle: `FOCUSDECK_CONFIG_DIR=$(mktemp -d)` ile tek kutulu, `fake-plugin`'i `FOCUSDECK_DATA_DIR/plugins/fake` olarak kopyalanmış bir profil açılır; tmux 120x40 bölmesinde `cargo run -p focusdeck -- --profile deneme` iki öğeyi gösterir, `q` ile çıkar, terminal bozulmaz.

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck
git commit -m "Add layout, rendering and TUI event loop"
```

### Task 9: `capture` ve `remind` alt komutları

**Files:**
- Create: `crates/focusdeck/src/notify.rs`, `crates/focusdeck/src/remind.rs`, `crates/focusdeck/src/capture_cmd.rs`
- Modify: `crates/focusdeck/src/main.rs`
- Test: `crates/focusdeck/tests/commands.rs`

**Interfaces:**
- Consumes: `capture::parse`, `Queue` (Task 3), `PluginHost` (Task 4), `Paths`, `Config`, `profile`, `secrets` (Task 2)
- Produces:
  - `notify::send(title: &str, body: &str, critical: bool)` (`notify-send`, `-u critical` yalnız `critical`; hata log'a)
  - `remind::State` (`BTreeMap<String, DateTime<FixedOffset>>`, `load(path)`, `save(path)`)
  - `remind::due<'a>(box_id: &str, reminders: &'a [Reminder], now: DateTime<FixedOffset>, state: &State) -> Vec<(String, &'a Reminder)>`
  - `remind::prune(state: &mut State, now: DateTime<FixedOffset>)`
  - `async fn remind::run(paths: &Paths, profile: &Profile, send: bool, now: DateTime<FixedOffset>) -> anyhow::Result<usize>`
  - `async fn capture_cmd::run(paths: &Paths, cfg: &Config, profile: &Profile, text: Option<String>, now: DateTime<FixedOffset>) -> anyhow::Result<i32>` (çıkış kodu)

- [ ] **Step 1: Başarısız testleri yaz**

```rust
// remind::due, now = 10:00
at 09:59 → döner, anahtar "sozlu:5@2026-11-12T09:59:00+03:00"
at 10:01 → dönmez; at 21:59 dünden (12 saatten eski) → dönmez; state'te anahtar varsa → dönmez
prune: 8 gün önceki kayıt silinir, 6 gün önceki kalır

// capture_cmd (fake-plugin ile, FOCUSDECK_*_DIR tempdir, notify-send yerine PATH'e konan sahte betik çağrıları dosyaya yazar)
metin "log at 14:30" → 0 döner, fake-plugin drop'u aldı, bildirim "eklendi" / "log at · 14:30"
metin "@ali 14:30" → 1 döner, bildirim başlığı "focusdeck: eklenmedi" kritik, kuyruk boş
plugin crash modunda → 1 döner, kuyrukta 1 kayıt, bildirim "focusdeck: kuyruğa alındı"
kuyrukta 2 kayıt varken başarılı yakalama → 3 drop, kuyruk dosyası yok, bildirim gövdesinde "kuyruktan 2 not gönderildi"
capture_into tanımsız profil → 1 döner, stderr "bu profilde capture_into yok"
text None ve capture_command boş satır döndürür → 0, drop yok
```

- [ ] **Step 2: Düştüğünü gör**, **Step 3: Yaz**

`remind --send` olmadan saati gelenleri `SS:DD  başlık` olarak yazar, bildirim göndermez, durumu değiştirmez. `--send` ile bildirim başlığı `⏰ <başlık>`, gövde `SS:DD`, kritik. Durum dosyası `state_dir/reminded.json`.

- [ ] **Step 4: Geçsin**

- [ ] **Step 5: Commit**

```bash
git add crates/focusdeck
git commit -m "Add capture and remind commands"
```

### Task 10: Vikunja plugin'i

**Review:** per-task — destructive

**Files (repo `~/Desktop/personal/focusdeck-plugins`, yeni):**
- Create: `Cargo.toml` (workspace, üye `plugins/vikunja`), `.gitignore`, `README.md`, `plugins/vikunja/Cargo.toml` (ikili adı `focusdeck-vikunja`, `focusdeck-protocol = { path = "../../../focusdeck/crates/focusdeck-protocol" }`), `plugins/vikunja/src/main.rs`, `src/client.rs`, `src/plugin.rs`, `scripts/local-index.sh`
- Test: `plugins/vikunja/tests/vikunja.rs`

**Interfaces:**
- Consumes: `focusdeck_protocol::{Plugin, serve, Item, ...}` (Task 1)
- Produces: `focusdeck-vikunja` ikilisi; config anahtarları `url`, `token_file`, `project` (başlık), isteğe bağlı `bucket` (sütun başlığı)
  - `plugin::Vikunja::default()`, `impl Plugin for Vikunja`; describe `name "vikunja"`, `actions ["done","delete","move","reorder"]`, `accepts_drop true`, `reminders true`

Vikunja v2.8 uçları (canlı `/api/v1/docs.json` ile doğrulandı):
- `GET /projects?per_page=200` → başlığa göre proje (büyük/küçük harf duyarsız)
- `GET /projects/{p}/views` → `view_kind` kanban olan görünüm; `default_bucket_id`, `done_bucket_id`
- `GET /projects/{p}/views/{v}/buckets` → sütun başlığı → id
- `GET /projects/{p}/views/{v}/tasks?per_page=200` → sütunlar, her birinde `tasks`
- `PUT /projects/{p}/tasks` görev açar; `POST /tasks/{id}/labels/bulk` `{"labels":[{"id":n}]}`; `GET /labels?s=<başlık>`, `PUT /labels`
- `POST /projects/{p}/views/{v}/buckets/{b}/tasks` `{"task_id","bucket_id","project_view_id"}` sütun taşır
- `POST /tasks/{id}/position` `{"task_id","project_view_id","position"}` (float)
- `GET /tasks/{id}`, `POST /tasks/{id}` (tam nesneyle günceller, eksik alan sıfırlanır), `DELETE /tasks/{id}`

- [ ] **Step 1: `view_kind`'in JSON biçimini canlıda doğrula**

Run: `curl -s -H "Authorization: Bearer $(cat ~/.config/pano/vikunja-token)" https://vikunja.omertahaoztop.tr/api/v1/projects/9/views | jq '.[] | {id, view_kind}'`
Expected: `id 37` olan satırda `view_kind` değeri. Sayıysa (`3`) ve metinse (`"kanban"`) iki biçimi de kabul eden serde çözümü yazılır; görülen değer testteki fixture'a konur.

- [ ] **Step 2: Başarısız testleri yaz** (mockito sunucusu, token tempfile 0600)

```rust
configure: proje "Mono" → id 9, kanban görünüm 37, default 59, done 61; bucket "Karşı tarafı bekliyorum" → 62
configure proje yok → Err "Vikunja'da proje yok: Mono"; 401 → Err "Vikunja 401: token geçersiz"
list: done sütunundaki görev yok; due "0001-01-01T00:00:00Z" → None; due geçmiş → overdue true
list: açıklamada "focusdeck-source: redmine:45539" → source Some(redmine/45539)
list: "kimden:ali" etiketi → subtitle "@ali" ile başlar, labels'ta yok; url "<url>/tasks/<id>"
list bucket ayarlıyken yalnız o sütun
action done → POST .../buckets/61/tasks gövdesinde task_id
action delete → DELETE /tasks/<id>
action reorder position 1, liste pozisyonları [100, 200, 300], taşınan 0. öğe → POST /tasks/<id>/position position 250
action reorder position 0 → ilk öğenin pozisyonunun yarısı; son sıraya → son pozisyon + 65536
drop (from None): PUT gövdesi title, due_date ve reminders aynı zaman; description "<p>ayrıntı</p><p><a href=\"https://destek.mono.tr/issues/45539\">R45539</a></p>"; labels bulk "mail" ve "kimden:ali" id'leri
drop description HTML kaçışı: "<b>" → "&lt;b&gt;"
drop source Some(redmine:45539) → açıklamada "<p>focusdeck-source: redmine:45539</p>"
drop bucket ayarlı kutuda → görev açıldıktan sonra o sütuna taşınır
drop from Some(vikunja:77), aynı proje → yeni görev açılmaz, 77 hedef sütuna taşınır
drop from Some(vikunja:77), başka proje → GET /tasks/77, POST /tasks/77 gövdesinde project_id yeni proje ve diğer alanlar GET'teki gibi
reminders since 09:00: 09:30 hatırlatmalı görev döner, 08:00'lik dönmez
```

- [ ] **Step 3: Düştüğünü gör** — `cargo test -p focusdeck-vikunja` → FAIL

- [ ] **Step 4: Yaz**

`client.rs` engelleyici `reqwest` (`blocking`, `json`, `rustls-tls`), `Authorization: Bearer`, `User-Agent: focusdeck-vikunja/<sürüm>`, 10 sn zaman aşımı; hata metni `Vikunja <kod>: <kısa neden>`, gövde ve token metne girmez. Plugin son listeyi saklar; `reorder` pozisyonu bu listeden hesaplar, liste yoksa önce listeler.

- [ ] **Step 5: Geçsin** — PASS

- [ ] **Step 6: `scripts/local-index.sh`**

Tek satırlık başlık yorumu. `cargo build --release`, her ikili için sha256 alıp `dist/index.toml` yazar; `url` `file://<mutlak yol>/target/release/focusdeck-<ad>`, `targets` anahtarı `rustc -vV`'deki `host`. Çıkışta yazdığı dosyanın yolunu basar.

Run: `bash scripts/local-index.sh` → `dist/index.toml` içinde `name = "vikunja"` ve 64 haneli sha256

- [ ] **Step 7: Commit**

```bash
git init -b main
git add .
git commit -m "Add Vikunja plugin and local index script"
```

### Task 11: Bu makinede kurulum ve canlı deneme

**Files:**
- Create (repo dışı, kullanıcı ayarı): `~/.config/focusdeck/config.toml`, `~/.config/focusdeck/profiles/is.toml`, `~/.config/focusdeck/secrets/vikunja` (0600)
- Create (Vikunja'da): "Mono Fikirler" projesi

**Interfaces:**
- Consumes: her şey

- [ ] **Step 1: Kur**

```bash
cargo install --path ~/Desktop/personal/focusdeck/crates/focusdeck --locked
install -D -m 600 ~/.config/pano/vikunja-token ~/.config/focusdeck/secrets/vikunja
```
`config.toml`: `default_profile = "is"`, `registries = ["file://$HOME/Desktop/personal/focusdeck-plugins/dist/index.toml"]` (genişletilmiş mutlak yol).
Run: `focusdeck plugin install vikunja && focusdeck plugin list`
Expected: `vikunja  0.1.0`

- [ ] **Step 2: "Mono Fikirler" projesini aç**

Run: `curl -s -X PUT -H "Authorization: Bearer $(cat ~/.config/focusdeck/secrets/vikunja)" -H 'Content-Type: application/json' -d '{"title":"Mono Fikirler"}' https://vikunja.omertahaoztop.tr/api/v1/projects | jq '{id, title}'`
Expected: yeni `id` ve `"title": "Mono Fikirler"`

- [ ] **Step 3: İş profili**

`is.toml`: `capture_into = "sozlu"`, `capture_refs = { R = "https://destek.mono.tr/issues/{}" }`, `capture_label_aliases = { tel = "telefon", telefon = "telefon", yuz = "yuz-yuze", "yüz" = "yuz-yuze" }`; kutular `sozlu` (row 0 col 0, project "Mono", refresh "1m"), `bekliyor` (row 0 col 1, project "Mono", bucket "Karşı tarafı bekliyorum"), `fikir` (row 1 col 0, width 2, project "Mono Fikirler").

- [ ] **Step 4: Canlı deneme**

```bash
focusdeck capture "focusdeck deneme @ali #tel R45539 yarın 09:00"
```
Expected: çıkış 0, "eklendi" bildirimi; Vikunja Mono'da görev: due ve hatırlatma yarın 09:00, etiketler `telefon` ve `kimden:ali`, açıklamada Redmine linki.

`hyprctl dispatch 'hl.dsp.exec_cmd("ghostty -e focusdeck", {workspace="6 silent", float=true, size="1800 1000", center=true})'` ile aç, `grim` ile ekran görüntüsü al. Görüntüde üç kutu, deneme görevi Sözlü işler'de, alt satırda ipucu. Deneme görevini fareyle Bekliyorum kutusuna sürükle, Vikunja'da sütunu değişmiş olsun; sonra `x` ile bitir, Vikunja'da done olsun.

Run: `focusdeck remind` → saati gelmiş hatırlatma yoksa boş çıktı, çıkış 0.

- [ ] **Step 5: Temizlik**

Deneme görevi Vikunja'dan silinir (`DELETE /tasks/<id>`), ekran görüntüsü kullanıcıya gösterildikten sonra `trash-put` ile atılır. `pano` ve wtfutil çalışmaya devam eder, kısayollara ve systemd'ye dokunulmadı.
