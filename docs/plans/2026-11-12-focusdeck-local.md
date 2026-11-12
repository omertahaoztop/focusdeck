# focusdeck yerel kayıt: uygulama planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `plugin = "local"` yazan kutu, hiçbir kurulum ve hesap istemeden notları `~/.local/share/focusdeck/local.json` dosyasında tutar. Hiç profil yoksa focusdeck tek "Notlar" kutulu başlangıç profilini kendisi açar.

**Architecture:** Yerel kayıt kodu `focusdeck` crate'inde yeni `local` modülüne girer (`store.rs` dosya ve kilit, `plugin.rs` protokol eylemleri). `build_host` `local` için plugin dizinine bakmaz; `current_exe()`'yi `serve-local` argümanıyla başlatır ve kutunun `list` ayarını yoksa kutu `id`'siyle doldurur. `main` ilk argüman `serve-local` ise clap'e ve tokio'ya girmeden `focusdeck_protocol::serve` döngüsünü çalıştırır. Profil seçimi `profile::resolve_name` ile tek yerde toplanır.

**Tech Stack:** Rust 2024 (rustc ≥ 1.89, std `File::lock`), serde_json, chrono, clap 4, tempfile (test).

**Spec:** `docs/superpowers/specs/2026-11-12-focusdeck-local-design.md`

## Global Constraints

- Repo: `/home/omertahaoztop/Desktop/personal/focusdeck`, dal `focusdeck-local`.
- `focusdeck-protocol` tiplerine dokunulmaz; protokol sürümü 1 kalır. Plugins reposuna dokunulmaz.
- Yeni bağımlılık yok. Workspace `Cargo.toml` `[workspace.package]` altına `rust-version = "1.89"` eklenir; crate'ler `rust-version.workspace = true` alır.
- Mevcut Vikunja/Redmine/Gmail/Zulip profillerinin davranışı değişmez. `local` dışındaki plugin adları için `build_host` bugünkü gibi çalışır.
- Kayıt dosyası bozuksa ya da daha yeni bir biçimdeyse focusdeck dosyaya hiçbir koşulda yazmaz.
- Yeni kullanıcı metinleri `locales/en.ftl` ve `locales/tr.ftl`'ye birlikte girer (parite testi). Türkçe metinler doğru karakterle yazılır.
- Testler `i18n::init` çağırmaz; kütüphane testleri İngilizce metin bekler. İkiliyi çalıştıran testler `LC_ALL` ile dili sabitler.
- Testlerde gerçek kişi, şirket ya da iş verisi yok.
- Commit: İngilizce, emir kipi, tek satır, en çok 72 karakter, gövde ve önek yok. Yorum yalnız nedeni söyler, tek satır.
- Her görevden sonra `cargo test --workspace` ve `cargo clippy --workspace --all-targets -- -D warnings` temiz geçer.

## Review Focus

1. Aynı anda yazan iki süreç (TUI kutusu ve `focusdeck capture`) not kaybetmez. (Task 1 eşzamanlı yazma testi)
2. Bozuk JSON ya da `version > 1` olan dosya: okuma hata verir, `update` hiçbir şey yazmaz, dosyanın baytları aynı kalır. (Task 1 testi)
3. Bilinmeyen alanlar (kökte ve öğede) yazma sonrası korunur. (Task 1 testi)
4. `x` notu silmez, `done_at` doldurur; `delete` yalnız plandan çıkarmada çağrılır. (Task 2 testi)
5. `reorder` yalnız taşınan öğenin `position`'ını değiştirir. (Task 2 testi)
6. Profili olan kullanıcıda ilk açılış kuralı hiçbir dosya yaratmaz. (Task 5 testi)

---

### Task 1: Kayıt dosyası ve kilit (`local::store`)

**Review:** per-task — concurrency / persisted format

**Files:**
- Modify: `Cargo.toml` (`rust-version = "1.89"`), `crates/*/Cargo.toml` (`rust-version.workspace = true`)
- Create: `crates/focusdeck/src/local/mod.rs`, `crates/focusdeck/src/local/store.rs`
- Modify: `crates/focusdeck/src/lib.rs` (`pub mod local;`)
- Modify: `crates/focusdeck/locales/en.ftl`, `crates/focusdeck/locales/tr.ftl`

**Arayüz (sonraki görevler buna dayanır):**

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub list: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<DateTime<FixedOffset>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub position: f64,
    pub created: DateTime<FixedOffset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<DateTime<FixedOffset>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

pub enum StoreError { Corrupt { path, error }, Newer { path, version }, Io { path, error } }
impl StoreError { pub fn localized(&self, l: &Localizer) -> String }  // Display = current()

pub struct Store { path: PathBuf }
impl Store {
    pub fn new(path: PathBuf) -> Store;               // local.json yolu
    pub fn from_paths(paths: &Paths) -> Store;         // paths.data_dir.join("local.json")
    pub fn read(&self) -> Result<Vec<Record>, StoreError>;
    pub fn update<T>(&self, f: impl FnOnce(&mut Vec<Record>) -> Result<T, String>)
        -> Result<Result<T, String>, StoreError>;     // f Err dönerse dosya yazılmaz
}
pub fn new_id(existing: &[Record]) -> String;
```

Dosya kökü `struct FileData { version: u32, items: Vec<Record>, #[serde(flatten)] extra: Map<String, Value> }`. Kilit dosyası `local.json.lock`, geçici dosya `local.json.tmp`, ikisi de `local.json` ile aynı dizinde (EXDEV olmasın diye `/tmp` kullanılmaz).

- [ ] **Step 1: Başarısız testleri yaz** (`store.rs` içinde `#[cfg(test)]`, `tempfile::tempdir()`):
  - `missing_file_reads_empty`: dosya yokken `read()` boş döner, `local.json` yaratılmaz.
  - `update_then_read_round_trips`: iki kayıt ekle, oku, alanlar aynı; dosya `{"version":1,"items":[...]}` biçiminde.
  - `unknown_fields_survive_a_write`: elle `{"version":1,"color":"red","items":[{...,"pin":true}]}` yaz, başka bir öğeyi değiştiren `update` çağır; kökte `color`, öğede `pin` duruyor.
  - `corrupt_file_is_never_overwritten`: `local.json`'a `{"version":1,"items":[` yaz; `read()` `Corrupt` döner, `update(|_| Ok(()))` `Corrupt` döner, dosyanın baytları değişmedi, `.tmp` yok.
  - `newer_version_is_never_overwritten`: `{"version":2,"items":[]}` için `read`/`update` `Newer { version: 2 }` döner, baytlar aynı.
  - `closure_error_writes_nothing`: `update(|items| { items.clear(); Err("no".into()) })` sonrası dosya eskisi gibi.
  - `concurrent_updates_keep_every_record`: 8 thread, her biri ayrı `Store::new(aynı yol)` ile 25 kez `update` içinde `new_id` ile kayıt ekler; sonunda 200 kayıt, id'ler benzersiz. (Linux'ta `flock` açık dosya başına olduğundan aynı süreçteki ayrı `File`'lar birbirini bekler.)
  - `new_id_is_unique_against_existing`: `new_id` mevcut id'lerle çakışmaz; art arda 1000 çağrı benzersiz.
  - `file_is_private` (`#[cfg(unix)]`): yazılan `local.json` izni `0o600`.
- [ ] **Step 2: Çalıştır, düştüklerini gör:** `cargo test -p focusdeck local::store` → derleme hatası/FAIL.
- [ ] **Step 3: Uygula:**
  - `lock_file(shared: bool) -> Result<File, StoreError>`: `create_dir_all(parent)`, `OpenOptions::new().create(true).truncate(false).write(true).read(true).open(lock_path)`, sonra `file.lock_shared()` ya da `file.lock()`. Kilit `File` düşünce kalkar.
  - `load(&self) -> Result<FileData, StoreError>`: `NotFound` → `FileData { version: 1, items: vec![], extra: empty }`; JSON hatası → `Corrupt`; `version > 1` → `Newer`.
  - `read`: paylaşımlı kilit, `load`, `items`.
  - `update`: yazma kilidi, `load` (her seferinde diskten, bellekteki kopya kullanılmaz), `f(&mut data.items)`; `Err` ise yazmadan dön; `Ok` ise `local.json.tmp`'yi `OpenOptionsExt::mode(0o600)` ile `create+truncate` aç, `serde_json::to_writer_pretty`, `sync_all`, `rename(tmp, local.json)`, üst dizini açıp `sync_all`.
  - `new_id`: `format!("{:x}{:x}{:x}", unix_nanos, std::process::id(), COUNTER.fetch_add(1))` ile `static COUNTER: AtomicU64`; `existing` içinde varsa sayaçla yeniden üret.
  - Metinler (en / tr):
    - `local-file-corrupt = { $path } could not be read ({ $error }); focusdeck will not write to it` / `{ $path } okunamadı ({ $error }); focusdeck bu dosyaya yazmayacak`
    - `local-file-newer = { $path } was written by a newer focusdeck (format { $version }); update focusdeck` / `{ $path } daha yeni bir focusdeck ile yazılmış (biçim { $version }); focusdeck'i güncelle`
    - `local-file-io = { $path }: { $error }` / `{ $path }: { $error }`
- [ ] **Step 4: Testler geçer:** `cargo test -p focusdeck local::store` → PASS. Sonra `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 5: Commit:** `git add` yeni dosyalar; `git commit -m "Add the local note store with file locking" -- Cargo.toml crates`

---

### Task 2: Yerel plugin eylemleri (`local::plugin`)

**Files:**
- Create: `crates/focusdeck/src/local/plugin.rs`
- Modify: `crates/focusdeck/src/local/mod.rs`, `locales/en.ftl`, `locales/tr.ftl`

**Arayüz:**

```rust
pub struct Local { store: Store, list: Option<String> }
impl Local { pub fn new(store: Store) -> Local }
impl focusdeck_protocol::Plugin for Local { ... }
pub fn serve_stdio() -> std::io::Result<()>;  // Store::from_paths(&Paths::from_env())
```

Davranış (spec "Eylemler"):

- `describe`: `name = "local"`, `version = env!("CARGO_PKG_VERSION")`, `protocol = PROTOCOL_VERSION`, `actions = ["done","delete","move","reorder"]`, `accepts_drop = true`, `reminders = true`.
- `configure`: `{"list": "<ad>"}` zorunlu ve boş olmayan metin (core doldurur); başka anahtarlar yok sayılır. Yoksa `local-invalid-settings`.
- `list`: `list` eşleşen ve `done_at` boş kayıtlar, `(position, created)` sırasıyla `Item`'a: `subtitle`/`url` boş, `overdue = due < now`.
- `action`:
  - `done`: `done_at = now`, `Ok(None)`.
  - `delete`: kaydı çıkarır.
  - `reorder` ve `move` (`move` `position` taşır, kutunun kovası yok): `args.position` (u64) görünen öğeler arasındaki hedef sıra; `others` = aynı listedeki bitmemiş öğelerin taşınan hariç `position`'ları, sıralı; yeni değer `place_between(&others, wanted)` (Vikunja'daki gövde, `POSITION_STEP = 1024.0`). `position` yoksa `local-position-required`.
  - Öğe yoksa ya da başka listedeyse `local-item-missing`; bilinmeyen eylem `local-unknown-action`.
- `drop_item`: `p.from` `plugin == "local"` ve o id dosyada varsa `list = self.list`, `position = son + POSITION_STEP`; yoksa yeni kayıt: `id = new_id`, `title`, `description`, `due`, `labels`, `from`, `links` `p.item`'dan, `source = p.item.source.or(p.from)`, `created = now`, `position = son + POSITION_STEP`. İkisinde de üretilen `Item` döner. "Son" = hedef listedeki bitmemiş öğelerin en büyük `position`'ı, liste boşsa `0.0`.
- `reminders`: listedeki bitmemiş, `due > p.since` kayıtlar, `Reminder { at: due, item }`.
- Kayıt hataları `StoreError`'ın yerelleştirilmiş metniyle `Err(String)` olur.

- [ ] **Step 1: Başarısız testleri yaz** (`plugin.rs` içinde, tempdir'de `Store`):
  - `describe_offers_move_drop_and_reminders`.
  - `configure_requires_a_list`.
  - `lists_only_open_items_of_its_own_list_in_position_order`: iki listede kayıtlar, biri bitmiş; `overdue` geçmiş `due` için `true`.
  - `done_keeps_the_record_and_hides_it`: `done` sonrası `list` görmez, dosyada `done_at` dolu.
  - `delete_removes_the_record`.
  - `reorder_moves_only_the_dragged_item`: üç öğe (1024, 2048, 3072); üçüncüyü 0'a taşı → sıra 3,1,2 ve diğer iki `position` aynı. Sona taşıma ve boş listede tek öğe kenarları.
  - `drop_from_another_plugin_creates_with_source`: `from = Source { plugin: "redmine", id: "12345" }`, `item.source = None` → kayıt `source` redmine:12345, listenin sonunda.
  - `drop_from_local_moves_without_copying`: `ev` listesindeki kaydı `is` kutusuna bırak → dosyada tek kayıt, `list = "is"`, id aynı, `is`'in sonunda.
  - `capture_fields_survive`: `due`, `labels`, `from`, `links`, `description` kayda ve `list` çıktısına geçer.
  - `reminders_fire_at_due_after_since`.
  - `corrupt_store_turns_into_an_error_line`: bozuk dosyada `list` `Err` döner, metinde dosya yolu var.
- [ ] **Step 2: Çalıştır, düştüklerini gör:** `cargo test -p focusdeck local::plugin` → FAIL.
- [ ] **Step 3: Uygula.** Metinler (en / tr):
  - `local-invalid-settings = invalid local box settings: { $error }` / `yerel kutu ayarı geçersiz: { $error }`
  - `local-item-missing = note { $id } is not in this box` / `{ $id } notu bu kutuda yok`
  - `local-position-required = a position is required` / `sıra numarası gerekli`
  - `local-unknown-action = unknown action: { $action }` / `bilinmeyen eylem: { $action }`
- [ ] **Step 4: Testler geçer**, ardından workspace test ve clippy.
- [ ] **Step 5: Commit:** `git commit -m "Implement the local box actions" -- crates/focusdeck`

---

### Task 3: `local` kutuyu programa bağla

**Files:**
- Modify: `crates/focusdeck/src/main.rs`, `crates/focusdeck/src/host.rs`, `crates/focusdeck/src/ui/run.rs`, `crates/focusdeck/src/remind.rs`
- Modify: `crates/focusdeck/tests/commands.rs`

Adımlar:

- `PluginHost::with_args(self, args: Vec<String>) -> Self`; `command()` `Command::new(&self.exe).args(&self.args)`.
- `main`: `#[tokio::main]` kalkar. `fn main()` önce `std::env::args_os().nth(1)` `"serve-local"` mi diye bakar; öyleyse dili `focusdeck_i18n::requested(None, env, &focusdeck_i18n::PLUGIN_ENV)` ile başlatıp `local::serve_stdio()` çalıştırır, hata olursa `ExitCode::FAILURE`. Değilse bugünkü akış `tokio::runtime::Runtime::new()?.block_on(...)` içinde. `serve-local` clap'e girmediği için `--help`'te görünmez.
- `ui::run::build_host`: `spec.plugin == LOCAL` (`pub const LOCAL: &str = "local"` `local/mod.rs`'te) ise `exe = std::env::current_exe()`, `args = ["serve-local"]`; `secrets::plugin_config` sonucu bir JSON nesnesi, `list` yoksa `spec.id` eklenir. Diğer adlar bugünkü yoldan.
- `remind.rs:85`: dosya kontrolü `spec.plugin != LOCAL &&` ile sınırlanır.

- [ ] **Step 1: Başarısız testleri yaz:**
  - `ui/run.rs` birim testleri: `local_box_needs_no_plugin_file` (`build_host` boş plugin dizininde `local` için `Ok`, başka bir kurulu olmayan ad için bugünkü `ERROR_PLUGIN_NOT_INSTALLED`); `local_config_defaults_list_to_box_id` (saf `fn local_config(spec: &BoxSpec, value: Value) -> Value`: `list` yoksa `spec.id`, varsa dokunulmaz).
  - `host.rs` birim testi: `with_args` verilen argümanlar `command()`'a geçer (`lang_env` testindeki gibi `Command::get_args`).
  - `tests/commands.rs` (gerçek `serve-local` süreci; `FOCUSDECK_DATA_DIR` test ortamından devralınır):
    - `capture_into_a_local_box_needs_no_plugin`: profilde tek `local` kutu (`capture_into = "notes"`), plugin dizininde dosya yok; `focusdeck capture "süt al #ev"` → çıkış 0, `data/local.json`'da `title = "süt al"`, `labels = ["ev"]`, `list = "notes"`.
    - `remind_sends_due_local_notes`: `local.json`'a `due` 5 dk önce olan bitmemiş kayıt elle yazılır; `focusdeck remind --send` → tek bildirim, başlık kaydın `title`'ı.
    - `serve_local_is_hidden_from_help`: `focusdeck --help` çıktısında `serve-local` yok.
- [ ] **Step 2: Çalıştır, düştüklerini gör.**
- [ ] **Step 3: Uygula.**
- [ ] **Step 4: Testler geçer**, workspace test ve clippy.
- [ ] **Step 5: Commit:** `git commit -m "Run local boxes from the focusdeck binary" -- crates/focusdeck`

---

### Task 4: `local` adını ayır

**Files:**
- Modify: `crates/focusdeck/src/cli.rs`, `crates/focusdeck/src/install.rs`, `locales/en.ftl`, `locales/tr.ftl`
- Modify: `crates/focusdeck/tests/commands.rs` (ya da `tests/install.rs`)

- `install::install` ve `install::remove`, `validate_name`'den sonra `name == LOCAL` ise ağa ve diske dokunmadan hata döner:
  - `install-local-builtin = local comes with focusdeck, there is nothing to install` / `local focusdeck ile birlikte gelir, kurulacak bir şey yok`
  - `install-local-not-removable = local comes with focusdeck and cannot be removed` / `local focusdeck ile birlikte gelir, kaldırılamaz`
- `cli::list`: kurulu plugin satırlarından sonra her zaman `local  (built-in)` / `local  (yerleşik)` satırı (`cli-builtin = (built-in)` / `(yerleşik)`). Kurulu plugin yoksa yalnız bu satır basılır; registry yine de yalnız kurulu plugin varsa sorgulanır.

- [ ] **Step 1: Başarısız testleri yaz:** `plugin install local` çıkış ≠ 0, metin `comes with focusdeck`, registry'ye istek yok (mockito'da beklenen çağrı 0) ve plugin dizini boş; `plugin remove local` aynı; `plugin list` boş dizinde tek satır `local  (built-in)`, `LC_ALL=tr_TR.UTF-8` ile `local  (yerleşik)`; kurulu `vikunja` varken `local` satırı en sonda.
- [ ] **Step 2: Düştüklerini gör.**
- [ ] **Step 3: Uygula.**
- [ ] **Step 4: Testler geçer**, workspace test ve clippy.
- [ ] **Step 5: Commit:** `git commit -m "Reserve the local plugin name" -- crates/focusdeck`

---

### Task 5: İlk açılış ve başlangıç profili

**Files:**
- Modify: `crates/focusdeck/src/profile.rs`, `crates/focusdeck/src/main.rs`, `locales/en.ftl`, `locales/tr.ftl`
- Modify: `crates/focusdeck/tests/commands.rs`

**Arayüz:**

```rust
pub fn resolve_name(paths: &Paths, cli: Option<&str>, env: Option<&str>, cfg: &Config)
    -> Result<String, ProfileError>;
pub fn starter_profile(title: &str) -> String;  // TOML metni
```

- Sıra (spec "İlk açılış"): `select_name` bir ad bulursa o (doğrulama dahil, bugünkü davranış). `NoProfileSelected` dönerse `profiles/*.toml` sayılır (`.` ile başlayanlar hariç): tek dosya → onun adı; hiç dosya yok ya da dizin yok → `profiles/` yaratılır, `notes.toml` yazılır (`create_new`, yarışta var olan dosya kullanılır), `"notes"` döner; birden çok → `NoProfileSelected`.
- `starter_profile(title)`: `name` ve kutu `title` = `title` (TOML'a `toml::Value::String` ile kaçışlı yazılır), `capture_into = "notes"`, tek `[[box]]`: `id = "notes"`, `plugin = "local"`, `row = 0`, `col = 0`. Üretilen metin `profile::load` ile geri okunabilir.
- `main::load_profile` `resolve_name` kullanır; `capture`, `remind` ve TUI aynı yoldan geçer. `config.toml` yazılmaz.
- Metin: `starter-title = Notes` / `Notlar`.

- [ ] **Step 1: Başarısız testleri yaz:**
  - Birim: `resolve_name` dört dal (açık ad, tek profil, hiç profil, çok profil); "tek profil" ve "açık ad" dallarında `profiles/` içinde yeni dosya oluşmaz; `starter_profile("Notlar")` `load` ile okunur, kutu `plugin = "local"`, `capture_into = "notes"`; başlıkta `"` olan metin de geçerli TOML.
  - `commands.rs` `first_capture_creates_the_starter_profile`: boş config dizini, `FOCUSDECK_PROFILE` verilmeden `LC_ALL=tr_TR.UTF-8 focusdeck capture "ilk not"` → çıkış 0, `profiles/notes.toml` var ve başlık `Notlar`, `data/local.json`'da `ilk not`, `config/config.toml` yok. (`Env::run_in` `FOCUSDECK_PROFILE`'ı her zaman koyduğu için profil koymayan bir yardımcı eklenir.)
  - `two_profiles_without_a_choice_still_error`: iki profil, seçim yok → bugünkü `no profile selected` hatası.
- [ ] **Step 2: Düştüklerini gör.**
- [ ] **Step 3: Uygula.**
- [ ] **Step 4: Testler geçer**, workspace test ve clippy.
- [ ] **Step 5: Commit:** `git commit -m "Open a starter notes profile on first run" -- crates/focusdeck`

---

### Task 6: Belgeler

**Files:**
- Modify: `README.md`, `README.tr.md`

- "Install"/"Kurulum" bölümünden sonra hızlı başlangıç: `cargo install` ardından doğrudan `focusdeck`; ilk açılışta "Notes/Notlar" kutusu gelir, `a` ile not alınır.
- "Profile"/"Profil" bölümüne `local` kutu örneği (`plugin = "local"`, isteğe bağlı `config = { list = "ideas" }`) ve `list` anahtarının anlamı: aynı `list` adını kullanan kutular aynı notları görür.
- Tek paragraf: notlar `~/.local/share/focusdeck/local.json` (`FOCUSDECK_DATA_DIR`) dosyasındadır, yedek için bu dosyayı kopyalamak yeter; bitmiş notlar dosyada kalır.
- `focusdeck plugin list` örnek çıktısında `local  (built-in)` satırı.

- [ ] **Step 1:** İki README'yi aynı içerikle güncelle; Türkçe metin doğru karakterle.
- [ ] **Step 2:** `cargo test --workspace` (README'ye bağlı test varsa) geçer.
- [ ] **Step 3: Commit:** `git commit -m "Document the built-in local box" -- README.md README.tr.md`
