# focusdeck çok dillilik: uygulama planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Çekirdek ve dört plugin kullanıcının dilini konuşur (ilk sürümde `en` ve `tr`). Çeviriler topluluğun düzenleyebileceği Fluent dosyalarında durur, iki repo da anonimleşir.

**Architecture:** Çekirdek reposunda yeni `crates/focusdeck-i18n` crate'i dil seçimini, Fluent yüklemeyi ve İngilizceye düşmeyi yapar. Her ikili kendi `locales/*.ftl` dosyalarını build script'le gömer ve mesaj kimliklerini sabit olarak üretir; yazım hatalı kimlik derlenmez. Çekirdek plugin'e istenen dili `FOCUSDECK_LANG` ile verir, protokol değişmez.

**Tech Stack:** Rust 2024, `fluent-bundle` 0.16 (`concurrent::FluentBundle`), `fluent-syntax` 0.12, `unic-langid` 0.9, clap 4, ratatui.

**Spec:** `docs/superpowers/specs/2026-11-12-focusdeck-i18n-design.md`

## Global Constraints

- Çekirdek repo: `/home/omertahaoztop/Desktop/personal/focusdeck`, dal `focusdeck-i18n`. Plugins repo: `/home/omertahaoztop/Desktop/personal/focusdeck-plugins`, dal `focusdeck-i18n`.
- Plugin protokolü sürüm 1 kalır, `focusdeck-protocol` tiplerine alan eklenmez.
- `tr.ftl` metinleri bugünkü Türkçe metinlerle karakteri karakterine aynıdır; Türkçe kullanan hiçbir fark görmez.
- İngilizce metin şu sözlüğe uyar (README.md tuş tablosu bununla yazıldı): not al → `note`, tıkla seç → `click select`, çift tıkla büyüt → `double click zoom`, sürükle taşı → `drag move`, bitti → `done`, yıldızı kaldır → `unstar`, aç → `open`, taşı → `move`, kutu → `box`, yardım → `help`, çık → `quit`, geri → `back`, ara → `search`, planıma al → `add to plan`, gecikmiş → `overdue`, Nereye taşınsın → `Move to`, Tuşlar → `Keys`, vazgeç → `cancel`, kaydet → `save`.
- Fluent bundle'larında `set_use_isolating(false)`; terminale Unicode yön işaretleri basılmaz.
- Testler hiçbir zaman `i18n::init` çağırmaz. Başlatılmamış dil İngilizcedir, yani kütüphane testleri İngilizce metin bekler. Belirli bir dili sınayan test `i18n::localizer(Some("tr"))` gibi açık bir `Localizer` kullanır. İkiliyi çalıştıran testler ortamı `LC_ALL` ile sabitler (`C` = İngilizce, `tr_TR.UTF-8` = Türkçe).
- Log dosyasına yazılan satırlar İngilizcedir ve çevrilmez.
- Yeni test ve çevirilerde gerçek kişi, şirket ya da iş verisi yok; örnekler `example.com`, `Home`, `12345`.
- Commit: İngilizce, emir kipi, tek satır, en çok 72 karakter, gövde ve önek yok. Yorum yalnız nedeni söyler, tek satır.
- Her görevden sonra ilgili repoda `cargo test --workspace` ve `cargo clippy --workspace --all-targets -- -D warnings` temiz geçer.

## Review Focus

1. `LANG=C.UTF-8`, `POSIX` ya da boş `LANG`: panik yok, İngilizce. (Task 1 testi)
2. `language = "pt-BR"` ve `pt` dosyası yok: İngilizce; `tr-TR`: Türkçe. (Task 1 testi)
3. Topluluktan gelen bozuk `.ftl`: `check_locales` dosya adı ve satırla düşer; çalışırken bozuk dosyanın geçerli mesajları kullanılır, kalanı İngilizceye düşer, panik yok. (Task 1 testi)
4. Çeviride değişken adı farklı (`{ $count }` yerine `{ $n }`): `check_locales` yakalar. (Task 1 testi)
5. Eski kuyruk kaydı (`lang` alanı yok) yeni sürümde: okunur ve o anki dille ayrıştırılır; yeni kayıt kendi diliyle ayrıştırılır. (Task 5 testi)

---

### Task 1: `focusdeck-i18n` crate'i (çekirdek repo)

**Files:**
- Create: `crates/focusdeck-i18n/Cargo.toml`, `crates/focusdeck-i18n/src/lib.rs`, `crates/focusdeck-i18n/src/build.rs`, `crates/focusdeck-i18n/src/check.rs`
- Create: `crates/focusdeck-i18n/tests/i18n.rs`, `crates/focusdeck-i18n/tests/fixtures/` (test dil dosyaları)
- Modify: `Cargo.toml` (workspace üyesi, `fluent-bundle = "0.16"`, `fluent-syntax = "0.12"`, `unic-langid = "0.9"`)

**Interfaces:**
- Produces:
  - `pub type Sources = &'static [(&'static str, &'static str)];` (dil etiketi, dosya içeriği)
  - `pub const CORE_ENV: [&str; 3] = ["LC_ALL", "LC_MESSAGES", "LANG"];`
  - `pub const PLUGIN_ENV: [&str; 4] = ["FOCUSDECK_LANG", "LC_ALL", "LC_MESSAGES", "LANG"];`
  - `pub fn normalize(raw: &str) -> Option<String>`: `tr_TR.UTF-8` → `tr-TR`, `de_DE@euro` → `de-DE`, `tr` → `tr`, `pt-BR` → `pt-BR`; `""`, `C`, `C.UTF-8`, `POSIX` → `None`.
  - `pub fn requested(config_language: Option<&str>, get: impl Fn(&str) -> Option<String>, vars: &[&str]) -> Option<String>`: önce `config_language`, sonra `vars` sırasıyla; `normalize` sonucu `Some` olan ilk değer.
  - `pub struct Localizer` (Send + Sync), `impl Localizer`:
    - `pub fn new(requested: Option<&str>, sources: Sources) -> Localizer`: zincir tam etiket, dil kısmı, `en`; dosyası olmayan atlanır, `en` hep sonda.
    - `pub fn lang(&self) -> &str`: zincirin ilk etiketi (`"tr"`, `"en"`).
    - `pub fn t(&self, id: &str) -> String`
    - `pub fn t_args(&self, id: &str, args: &[(&str, Arg<'_>)]) -> String`
    - `pub fn words(&self, id: &str) -> Vec<String>`: `t(id)` boşlukla bölünür, küçük harfe çevrilir.
  - `pub enum Arg<'a> { Str(&'a str), Num(i64) }`, `From<&'a str>`, `From<i64>`, `From<usize>`.
  - `pub fn check_locales(dir: &std::path::Path) -> Result<(), String>`
  - `pub mod build { pub fn generate(locales_dir: &str) }`: build script'ten çağrılır.
  - `#[macro_export] macro_rules! localized`: aşağıda.

- [ ] **Step 1: Testleri yaz** (`tests/i18n.rs`, fixture'lar `tests/fixtures/{en,tr,broken}.ftl`):
  - `normalize_maps_posix_locales`: yukarıdaki dönüşümlerin hepsi.
  - `requested_prefers_config_then_env_order`: `requested(Some("tr"), …)` → `tr`; config yok, `LC_ALL` yok, `LANG=tr_TR.UTF-8` → `tr-TR`; `LC_ALL=C`, `LANG=tr_TR.UTF-8` → `tr-TR` (C atlanır); hiçbiri → `None`.
  - `falls_back_to_language_then_english`: `Localizer::new(Some("tr-TR"), S).lang() == "tr"`; `Some("pt-BR")` → `"en"`; `None` → `"en"`.
  - `missing_message_uses_english`: `tr` dosyasında olmayan kimlik `en` metniyle döner.
  - `plural_follows_language`: en'de `items = { $count -> [one] one item *[other] { $count } items }`; `t_args("items", &[("count", 1.into())]) == "one item"`, `5` → `"5 items"`; çıktıda `\u{2068}` ya da `\u{2069}` yok.
  - `words_split_and_lowercase`: `capture-words-tomorrow = Yarın yarin` → `["yarın", "yarin"]`.
  - `broken_file_keeps_valid_messages`: sözdizimi hatalı bir mesaj ve geçerli bir mesaj taşıyan fixture ile `Localizer::new` panik yapmaz, geçerli mesaj o dilde gelir, bozuk olan İngilizce gelir.
  - `check_locales_reports_missing_extra_variable_and_syntax`: geçici dizinde `en.ftl` + eksik kimlikli, fazla kimlikli, değişken adı farklı, sözdizimi hatalı dosyalar; hata metni dosya adını ve sorunlu kimliği (sözdiziminde satırı) içerir. Uyumlu dizinde `Ok(())`.
- [ ] **Step 2:** `cargo test -p focusdeck-i18n` çalıştır, derleme hatasıyla düştüğünü gör.
- [ ] **Step 3: Uygula.** `Localizer` her dil için bir `fluent_bundle::concurrent::FluentBundle` tutar; `add_resource` hatalarında `FluentResource::try_new`'in kısmi sonucunu kullan. Mesaj hiçbir bundle'da yoksa kimliği döndür (üretilen sabitler bunu fiilen imkânsız kılar).
- [ ] **Step 4: `build::generate` ve `localized!`.** `generate(dir)`: `dir/*.ftl` dosyalarını sıralı okur, `cargo:rerun-if-changed=<dir>` ve her dosya için ayrıca basar, `$OUT_DIR/focusdeck_i18n.rs` yazar:

  ```rust
  pub const SOURCES: focusdeck_i18n::Sources = &[("en", include_str!("/abs/locales/en.ftl")), ("tr", include_str!("/abs/locales/tr.ftl"))];
  pub mod msg { pub const HINT_BOARD_NOTE: &str = "hint-board-note"; /* en.ftl'deki her mesaj; - yerine _, büyük harf */ }
  ```

  `en.ftl` yoksa ya da ayrıştırılamıyorsa build panik yapar. `localized!()` çağrıldığı modülde şunları tanımlar: `include!` ile üretilen dosya, `pub fn init(requested: Option<String>)` (ikinci çağrı yok sayılır), `pub fn requested() -> Option<&'static str>`, `pub fn current() -> &'static focusdeck_i18n::Localizer` (başlatılmadıysa İngilizce), `pub fn localizer(requested: Option<&str>) -> focusdeck_i18n::Localizer`, `pub fn t(id: &str) -> String`, `pub fn t_args(id: &str, args: &[(&str, focusdeck_i18n::Arg<'_>)]) -> String`.
- [ ] **Step 5:** `cargo test -p focusdeck-i18n` geçer; clippy temiz.
- [ ] **Step 6: Commit** `Add focusdeck-i18n crate`

### Task 2: Çekirdeği dil altyapısına bağla

**Files:**
- Create: `crates/focusdeck/build.rs`, `crates/focusdeck/locales/en.ftl`, `crates/focusdeck/locales/tr.ftl`, `crates/focusdeck/src/i18n.rs`
- Modify: `crates/focusdeck/Cargo.toml`, `crates/focusdeck/src/lib.rs`, `src/main.rs`, `src/cli.rs` (yalnız `pub fn command() -> clap::Command`), `src/config.rs`, `src/host.rs`, `src/ui/run.rs` (`build_host`), `crates/fake-plugin/src/main.rs`, `crates/focusdeck-protocol/src/serve.rs`
- Test: `crates/focusdeck/tests/host.rs`, `src/config.rs` testleri, `crates/focusdeck/tests/locales.rs`

**Interfaces:**
- Consumes: Task 1'in hepsi.
- Produces:
  - `focusdeck::i18n` modülü (`focusdeck_i18n::localized!()` ile): `init`, `requested`, `current`, `localizer`, `t`, `t_args`, `msg::*`, `SOURCES`.
  - `Config { pub language: Option<String>, pub capture_words: CaptureWords, …mevcut alanlar }`
  - `#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)] #[serde(default, deny_unknown_fields)] pub struct CaptureWords { pub tomorrow: Vec<String>, pub minute: Vec<String>, pub hour: Vec<String>, pub mon: Vec<String>, pub tue: Vec<String>, pub wed: Vec<String>, pub thu: Vec<String>, pub fri: Vec<String>, pub sat: Vec<String>, pub sun: Vec<String> }` (`config.rs`)
  - `PluginHost::with_lang(self, lang: Option<String>) -> Self`: `Some` ise süreç `FOCUSDECK_LANG` ile başlar.
  - `cli::command() -> clap::Command` (bu görevde yalnız `Cli::command()` döner; Task 4 metinleri ekler).

- [ ] **Step 1: Testleri yaz.**
  - `config.rs`: `language_and_capture_words_parse`: `language = "tr"` ve `[capture_words] tomorrow = ["tmrw"]` okunur; `[capture_words] tomorow = ["x"]` hata verir; dosya yokken `language == None`.
  - `tests/host.rs`: `host_passes_requested_language`: fake-plugin `FAKE_LANG_LOG=<yol>` ile çalışır, `with_lang(Some("tr-TR".into()))` sonrası ilk çağrıda dosyada `tr-TR` yazar; `with_lang(None)` ve ortamdan `FOCUSDECK_LANG` kaldırılmışken dosyada `-` yazar.
  - `tests/locales.rs`: `locales_match_english`: `focusdeck_i18n::check_locales(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/locales")))` `Ok`.
- [ ] **Step 2:** Testlerin düştüğünü gör.
- [ ] **Step 3: Uygula.** `build.rs`: `focusdeck_i18n::build::generate("locales")` (`focusdeck-i18n` hem dependency hem build-dependency, path ile). `en.ftl`/`tr.ftl` şimdilik `capture-words-*` mesajlarını taşır: tr `tomorrow = yarın yarin`, `mon = pzt pts`, `tue = sal`, `wed = çar car`, `thu = per prş prs`, `fri = cum`, `sat = cmt`, `sun = paz`, `minute = dk`, `hour = s`; en `tomorrow = tomorrow`, `mon = mon monday`, `tue = tue tuesday`, `wed = wed wednesday`, `thu = thu thursday`, `fri = fri friday`, `sat = sat saturday`, `sun = sun sunday`, `minute = min m`, `hour = h`. Dosya başındaki yorum biçimi çevirmene anlatır. `main.rs` sırası: `Paths::from_env()`, `Config::load` (hata olsa da devam), `focusdeck_i18n::requested(config.language, env, &CORE_ENV)`, `i18n::init`, `Cli::from_arg_matches(&cli::command().get_matches())`; komutlar config'i bugünkü gibi yeniden yükler, hata metni artık doğru dilde oluşur. fake-plugin: `FAKE_LANG_LOG` varsa başlarken `FOCUSDECK_LANG` değerini (yoksa `-`) bu dosyaya yazar. `build_host` `.with_lang(i18n::requested().map(str::to_owned))` ekler. `focusdeck-protocol` `drop_item` varsayılan hatası: `"this box does not accept dropped items"`.
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz.
- [ ] **Step 5: Commit** `Wire language selection into the core`

### Task 3: TUI metinleri

**Files:**
- Modify: `crates/focusdeck/src/ui/hint.rs`, `src/ui/render.rs`, `src/ui/app.rs`, `src/ui/run.rs` (flash ve kutu hata metinleri), `crates/focusdeck/locales/en.ftl`, `locales/tr.ftl`
- Test: aynı dosyaların test modülleri

**Interfaces:**
- Consumes: `focusdeck::i18n::{t, t_args, msg}`.
- Produces: kimlik önekleri `hint-`, `help-`, `box-`, `picker-`, `flash-`, `drag-`; kutu başlığı sayacı `box-count` (`$total`), `box-overdue` (`$count`), plan sayacı `box-planned` (`$planned`, `$total`).

- [ ] **Step 1: Testleri İngilizceye çevir.** Bu dosyalarda Türkçe metin bekleyen her assertion sözlükteki İngilizce karşılığı bekler (ör. ana ekran ipucu `"a note · click select · double click zoom · drag move · o open · m move · Tab box · ? help · q quit"`). Yeni test `tr_ui_texts_unchanged`: `i18n::localizer(Some("tr"))` ile `hint-board-note` → `"a not al"`, `picker-title` → `"Nereye taşınsın"`, `box-overdue` (`count=2`) → `"2 gecikmiş"`, `help-title` → `"Tuşlar"`. Yeni test `en_overdue_plural`: `box-overdue` en `count=1` → `"1 overdue"`, `count=3` → `"3 overdue"`.
- [ ] **Step 2:** Testlerin düştüğünü gör.
- [ ] **Step 3: Uygula.** `ui/` altındaki kullanıcıya görünen her metin `t`/`t_args` ile gelir; `&'static str` dönen yardımcılar `String` döner. `tr.ftl`'e bugünkü metinler aynen girer.
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz; `rg -n '"[^"]*[çğıöşüÇĞİÖŞÜ][^"]*"' crates/focusdeck/src/ui` test modülleri dışında sonuç vermez.
- [ ] **Step 5: Commit** `Translate TUI texts through Fluent`

### Task 4: CLI, kurulum ve hata metinleri

**Files:**
- Modify: `crates/focusdeck/src/cli.rs`, `src/main.rs`, `src/install.rs`, `src/profile.rs`, `src/secrets.rs`, `src/config.rs`, `src/duration.rs`, `src/capture_cmd.rs`, `src/remind.rs`, `src/notify.rs`, `src/host.rs`, `src/capture.rs` (yalnız `"başlık boş"`), `src/ui/run.rs` (`build_host` hatası), `locales/en.ftl`, `locales/tr.ftl`
- Test: bu dosyaların testleri, `crates/focusdeck/tests/{commands,install,host}.rs`

**Interfaces:**
- Consumes: `focusdeck::i18n`, `cli::command()`.
- Produces: kimlik önekleri `cli-`, `install-`, `profile-`, `config-`, `capture-`, `remind-`, `notify-`, `host-`, `error-`.

- [ ] **Step 1: Testleri güncelle.** Kütüphane testleri İngilizce metin bekler. İkiliyi çalıştıran testler `LC_ALL=C` verir ve İngilizce bekler. Yeni testler:
  - `tests/commands.rs` `turkish_locale_keeps_turkish_output`: `LC_ALL=tr_TR.UTF-8` ile `focusdeck capture` (profilde `capture_into` yok) stderr'de bugünkü Türkçe metni (`bu profilde capture_into yok`) basar.
  - `config_language_overrides_env`: config'te `language = "tr"`, `LC_ALL=C`; aynı komut Türkçe basar.
  - `help_is_localized`: `LC_ALL=C focusdeck --help` `"Focus board"` içerir; `LC_ALL=tr_TR.UTF-8` `"Odak panosu"` içerir.
  - `config.rs`: varsayılan config'te `capture_command == None`; `capture_cmd.rs` `default_prompt_is_localized`: komut yokken kurulan istem `i18n::localizer(Some("tr"))` ile `["rofi", "-dmenu", "-p", "Not"]`, en ile `[…, "Note"]`.
- [ ] **Step 2:** Testlerin düştüğünü gör.
- [ ] **Step 3: Uygula.** thiserror `#[error("…")]` metinleri `Display`'i elle yazarak `t_args`'a taşınır. `cli::command()` `about`, argüman ve alt komut yardımlarını `mut_arg`/`mut_subcommand` ile `t()`'den verir; `value_name` (`AD`/`NAME`, `METİN`/`TEXT`) da çevrilir. `main.rs`'teki `hata:` öneki `error-prefix`. `Config.capture_command` `Option<Vec<String>>` olur (config, dil seçilmeden okunur); `None` iken `capture_cmd` istemi `capture-prompt` metniyle kurar. `notify::log_line`'a giden metinler İngilizce sabit kalır, `t()` kullanmaz.
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz; `rg -n '"[^"]*[çğıöşüÇĞİÖŞÜ][^"]*"' crates/focusdeck/src` test modülleri dışında yalnız `capture.rs`'teki gün ve birim kelimelerini bulur (Task 5 onları kaldırır). Bu arama yalnız Türkçe harf taşıyan metni yakalar; `"Not"`, `"hata: "` gibi ASCII metinler elle taranır.
- [ ] **Step 5: Commit** `Translate CLI and error texts through Fluent`

### Task 5: `capture` kelimeleri ve kuyruk dili

**Review:** per-task — persisted format

**Files:**
- Modify: `crates/focusdeck/src/capture.rs`, `src/queue.rs`, `src/capture_cmd.rs`, `src/ui/run.rs`
- Test: `capture.rs`, `queue.rs` testleri, `tests/commands.rs`

**Interfaces:**
- Consumes: `Config.capture_words: CaptureWords`, `i18n::{current, localizer, msg}`.
- Produces:
  - `pub struct Words` (`capture.rs`), `impl Words { pub fn new(l: &focusdeck_i18n::Localizer, extra: &CaptureWords) -> Words }`
  - `pub fn parse(text: &str, now: DateTime<FixedOffset>, words: &Words, refs: &BTreeMap<String, String>, aliases: &BTreeMap<String, String>) -> Result<Capture, String>`
  - `Queued { pub text: String, pub at: DateTime<FixedOffset>, #[serde(default, skip_serializing_if = "Option::is_none")] pub lang: Option<String> }`

- [ ] **Step 1: Testleri yaz.** Mevcut `capture.rs` testleri `Words::new(&i18n::localizer(Some("tr")), &CaptureWords::default())` ile çalışır ve hepsi değişmeden geçer (Türkçe davranış korunur). Yeniler:
  - `english_words_parse`: en kelimeleriyle `"call back tomorrow 14:00"` yarın 14:00; `"water plants 30min"` 30 dakika sonra; `"x 2h"` 2 saat sonra; `"x mon 09:00"` ve `"x monday 09:00"` aynı sonuç.
  - `english_does_not_take_turkish_units`: en'de `"x 2s"` son tarih yok, başlık `"x 2s"`.
  - `user_words_extend_locale`: `CaptureWords { tomorrow: vec!["tmrw".into()], hour: vec!["sa".into()], ..Default::default() }` ile tr'de `"x tmrw 10:00"` ve `"x 3sa"` çalışır, `"x yarın 10:00"` hâlâ çalışır.
  - `units_match_case_insensitively_longest_first`: en'de `"x 30MIN"` 30 dakika (`min` `m`'den önce denenir).
  - `queue.rs` `old_entries_without_lang_still_load`: `{"text":"a","at":"2026-11-12T10:00:00+03:00"}` satırı `lang: None` ile okunur; `lang: Some("tr")` olan kayıt yazılıp okunduğunda alan korunur, `None` iken satırda `lang` anahtarı yok.
  - `tests/commands.rs` `queued_note_parses_in_its_own_language`: kuyruk dosyasına `lang: "tr"` ile `"ara yarın 10:00"` yaz, `LC_ALL=C` ile erişilebilir bir hedefe `focusdeck capture` çalıştır; fake-plugin'in `FAKE_LOG`'undaki bırakılan öğenin `due` alanı dolu, başlığı `"ara"`.
- [ ] **Step 2:** Testlerin düştüğünü gör.
- [ ] **Step 3: Uygula.** `parse_day`/`parse_spec` sabit kelimeler yerine `Words` kullanır; karşılaştırma küçük harfle, birimlerde en uzun sonek önce (eşitlikte dakika). `capture_cmd` kuyruğa yazarken `lang: Some(i18n::current().lang().to_owned())`; gönderimde `Words::new(&i18n::localizer(entry.lang.as_deref().or(i18n::requested())), &config.capture_words)`. TUI `Runtime` başlarken `Words::new(i18n::current(), &cfg.capture_words)` kurar.
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz.
- [ ] **Step 5: Commit** `Read capture words from the active language`

### Task 6: Çekirdek anonimleştirme, README ve sürüm

**Files:**
- Modify: `README.md`, `README.tr.md`, `crates/*/Cargo.toml` (`version = "0.2.0"`: focusdeck, focusdeck-protocol, focusdeck-i18n), `Cargo.lock`, `crates/focusdeck/src/{profile,secrets,capture}.rs` testleri, `crates/focusdeck/tests/*.rs`, `docs/plans/*.md`, `docs/superpowers/specs/*.md`

**Interfaces:** yok.

- [ ] **Step 1: Anonimleştir.** Değişimler: iş alan adları → `redmine.example.com`, `zulip.example.com`, `vikunja.example.com`; iş e-postası → `user@example.com`; iş proje adı → `"Home"`, onun fikir projesi → `"Home Ideas"`; müşteri adı → `Depo`; iş numarası → `12345`; plan belgelerindeki Vikunja proje/görünüm/sütun kimlikleri → `1`, `2`, `3`… Tasarım belgesinin "Anonimleştirme" bölümü değiştirilen değerleri adıyla saymaz, "iş alan adları, proje adı, müşteri adı, iş numarası" diye genel yazar.
- [ ] **Step 2: Doğrula.** Eski iş değerlerini arayan yerel `git grep` (kalıp listesi depoya yazılmaz) `Cargo.lock` dışında sonuç vermez. `cargo test --workspace` geçer.
- [ ] **Step 3: README.** İki README'ye (aynı içerik, kendi dilinde): Settings'e `language` ve `[capture_words]` (anahtarlar ve örnek), "cron/timer/kısayol `LANG` taşımayabilir, `language` yaz" notu; capture bölümünde dile göre kelimeler (en ve tr listesi); protokol bölümüne "plugin istenen dili `FOCUSDECK_LANG` ile alır"; yeni "Translating" / "Çeviri" bölümü: `crates/focusdeck/locales/<kod>.ftl` ekle, `en.ftl` temel, `cargo test` eksik ya da fazla kimliği söyler, `capture-words-*` biçimi, plugin çevirileri plugins reposunda. Kurulum ve protokol örneklerindeki etiket `v0.2.0`. Ekran görüntüsü bağlantıları `docs/screenshots/en/` (README.md) ve `docs/screenshots/tr/` (README.tr.md) altındaki `board.png`, `zoom.png`, `move.png`, `help.png` dosyalarını gösterir; görüntüleri kontrolcü sonra ekler.
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz.
- [ ] **Step 5: Commit** `Anonymize examples and document languages`

### Task 7: Vikunja ve Redmine plugin'leri (plugins repo)

**Files:**
- Create: `plugins/{vikunja,redmine}/build.rs`, `plugins/{vikunja,redmine}/locales/{en,tr}.ftl`, `plugins/{vikunja,redmine}/src/i18n.rs`, `plugins/{vikunja,redmine}/tests/locales.rs`
- Modify: `plugins/{vikunja,redmine}/Cargo.toml`, `src/*.rs`, `tests/*.rs`, `Cargo.lock`

**Interfaces:**
- Consumes: Task 1 (`focusdeck-i18n = { path = "../../../focusdeck/crates/focusdeck-i18n" }`, dependency ve build-dependency).
- Produces: plugin başına `i18n` modülü; `main` ilk iş `i18n::init(focusdeck_i18n::requested(None, |k| std::env::var(k).ok(), &focusdeck_i18n::PLUGIN_ENV))`.

- [ ] **Step 1: Testleri yaz/güncelle.** Kullanıcıya görünen metin bekleyen testler İngilizce bekler. Yeniler: her plugin'de `locales_match_english`; redmine `late_subtitle_by_language`: `i18n::localizer(Some("tr"))` ile gecikme alt satırı `"3g gecikmiş"`, en ile `"3d overdue"`. Vikunja testlerindeki iş proje adı → `Home`, iş numarası → `12345`.
- [ ] **Step 2:** Testlerin düştüğünü gör.
- [ ] **Step 3: Uygula.** Hata ve alt satır metinleri `t`/`t_args`'tan gelir. Vikunja'nın `kimden:` etiket öneki ve `focusdeck-source: ` satırı veri biçimidir, çevrilmez.
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz.
- [ ] **Step 5: Commit** `Translate Vikunja and Redmine plugins`

### Task 8: Zulip ve Gmail plugin'leri, index aracı, README

**Files:**
- Create: `plugins/{zulip,gmail}/build.rs`, `plugins/{zulip,gmail}/locales/{en,tr}.ftl`, `plugins/{zulip,gmail}/src/i18n.rs`, `plugins/{zulip,gmail}/tests/locales.rs`
- Modify: `plugins/{zulip,gmail}/Cargo.toml`, `src/*.rs` (gmail `login.rs` dahil), `tests/*.rs`, `plugins/*/Cargo.toml` (`description` İngilizce), `tools/focusdeck-index/src/*.rs`, `README.md`, `README.tr.md`, `Cargo.lock`

**Interfaces:**
- Consumes: Task 1, Task 7'deki kalıp.

- [ ] **Step 1: Testleri yaz/güncelle.** Kullanıcıya görünen metin bekleyen testler İngilizce. Yeniler: her plugin'de `locales_match_english`; gmail `age_by_language`: tr `40dk`, `5s`, `4g`; en `40m`, `5h`, `4d`.
- [ ] **Step 2:** Testlerin düştüğünü gör.
- [ ] **Step 3: Uygula.** Zulip ve Gmail metinleri (gmail `login` çıktısı dahil) Fluent'ten. `focusdeck-index` metinleri İngilizce sabit olur, Fluent kullanmaz. Açıklamalar: vikunja `"Open tasks of a Vikunja project"`, redmine `"Open Redmine issues assigned to me"`, zulip `"Starred Zulip messages"`, gmail `"Starred Gmail messages"`. İki README'ye "Plugins speak the language focusdeck passes in `FOCUSDECK_LANG`; run directly they use `LC_ALL`/`LC_MESSAGES`/`LANG`" notu ve "Translating" / "Çeviri" bölümü (`plugins/<ad>/locales/<kod>.ftl`).
- [ ] **Step 4:** `cargo test --workspace` ve clippy temiz; eski iş değerlerini arayan yerel `git grep` sonuç vermez.
- [ ] **Step 5: Commit** `Translate Zulip and Gmail plugins and document languages`

---

## Yayın (kontrolcü, görev değil)

1. Çekirdek dalını `main`'e ileri sar, push, CI iki hedefte yeşil; `v0.2.0` etiketi.
2. Plugins'te dört plugin'in `focusdeck-protocol` ve `focusdeck-i18n` bağımlılığı `{ git = "https://github.com/omertahaoztop/focusdeck", tag = "v0.2.0" }`, sürümler `0.2.0`; `cargo test --workspace`; commit `Depend on focusdeck v0.2.0`; `main`'e ileri sar, push, CI yeşil.
3. Etiketleri tek push'ta en çok üç olacak şekilde gönder; dört release yeşil, `index.toml`'da sürümler `0.2.0`.
4. Anonim demo ile tam ekran `en` ve `tr` görüntüleri, commit `Add screenshots`.
5. Bu makinede `config.toml`'a `language = "tr"`, plugin'leri `focusdeck plugin install` ile güncelle.
