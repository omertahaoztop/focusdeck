# focusdeck redmine, zulip, gmail plugin'leri ve GitHub yayını: uygulama planı (Plan 2/4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redmine, Zulip ve Gmail kutularını yazıp iki repoyu GitHub'da herkese açık yayınlamak; plugin'ler etiketle derlenip `index.toml` üzerinden kurulur hale gelir ve iş profili altı kutuyla çalışır.

**Architecture:** Üç yeni plugin `focusdeck-plugins/plugins/<ad>` altında Vikunja plugin'iyle aynı biçimde yazılır: `lib.rs` (`client`, `plugin` modülleri), ince `main.rs` (`focusdeck_protocol::serve`), mockito ile kayıtlı HTTP cevabı testleri. Yeni `tools/focusdeck-index` crate'i `index.toml` girdisini yazar ve etiket sürümünü denetler; `release.yml` bunu kullanarak iki mimari için ikili yayınlar ve `main` üzerindeki `index.toml`'u günceller. Çekirdek reposu yayına hazırlanır, `v0.1.0` etiketi plugin reposunun protokol bağımlılığı olur.

**Tech Stack:** Rust 2024 (rustc 1.99), reqwest 0.13 blocking + rustls, serde/serde_json, chrono 0.4, sha2 0.11, base64 0.22, getrandom 0.3, toml 1, clap 4; testlerde mockito 1, tempfile 3. GitHub Actions (`ubuntu-24.04`, `ubuntu-24.04-arm`), `gh` CLI.

**Spec:** `docs/superpowers/specs/2026-11-12-focusdeck-design.md`

**Önceki plan:** `docs/plans/2026-11-12-focusdeck-core-vikunja.md` (Plan 1, `main`'e birleşti). **Sonraki planlar:** Plan 3 GOWA LXC ve whatsapp plugin'i. Plan 4 geçiş (F9, zamanlayıcı, şifre yenileme, `pano` emekliliği). Bu plan kısayollara, systemd birimlerine, `pano`'ya ve `~/.config/pano` dosyalarına dokunmaz; şifreler oradan kopyalanır, taşınmaz.

## Global Constraints

- Linux x86_64 ve aarch64. Hedef üçlüleri `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`.
- `edition = "2024"`, workspace `resolver = "3"`.
- Kullanıcıya görünen her metin Türkçe ve tam karakterli (ç ğ ı İ ö ş ü). Kod tanımlayıcıları İngilizce.
- Yorum yalnız kodun gösteremediği neden için, tek satır. Dosya başlığı en fazla tek satır. Bölüm ayırıcı yorum yok.
- Commit: İngilizce, imperatif, tek satır ≤72 karakter, gövde yok, önek yok.
- Şifre değeri (API anahtarı, OAuth token, client secret, authorization code, PKCE verifier) hiçbir yerde loglanmaz, yazdırılmaz, hata mesajına girmez. Hata mesajı yalnız dosya yolunu ya da HTTP durumunu yazar.
- Kişiye özel bilgi (alan adı, e-posta, hesap) kodda ve testlerde yer almaz; testler `example.com`/`example.org` kullanır. Kişiye özel değerler yalnız kullanıcının profilinde durur.
- Plugin ikili adı `focusdeck-<ad>`, crate adı `focusdeck-<ad>`, `describe().name` = `<ad>`, `version` = `env!("CARGO_PKG_VERSION")`, `protocol` = `PROTOCOL_VERSION` (1).
- Şifre dosyası profilde `*_file` anahtarıyla gelir; izin denetimini çekirdek yapar, plugin dosyayı okuyup `trim` eder, boşsa `"<Ad> anahtar dosyası boş: <yol>"` döner.
- Her HTTP isteği 15 saniye zaman aşımlı; HTTP hatası `"<Kaynak>: HTTP <kod>"` olarak döner (gövde yok).
- Plugin reposunda üç plugin task'ı ve Task 4 aynı `Cargo.toml`/`Cargo.lock`'a dokunur, sırayla koşar. Task 5 çekirdek reposundadır, onlarla paralel koşabilir.
- Her task sonunda kendi reposunda `cargo test --workspace` ve `cargo clippy --workspace --all-targets -- -D warnings` temiz.

## Review Focus

1. Redmine sayfalama sırasında bir iş sayfa değiştirir ya da sunucu eksik sayfa döner: plugin eksik liste yerine hata döner, çünkü eksik liste `plan_into` bağlı Vikunja görevlerini kapattırır. Test: Task 1 `incomplete_listing_is_an_error`.
2. Gmail refresh token iptal edilmiş (400/401): kutuda `Gmail oturumu düştü: focusdeck-gmail login` yazar, plugin çökmez. Test: Task 3 `revoked_refresh_token_tells_to_login`.
3. Zulip mesajı yalnız resim, alıntı ya da boşluk: başlık `(boş mesaj)` olur, satır kaymaz. Test: Task 2 `blank_content_gets_placeholder_title`.
4. Gmail mesajında `Subject` ya da `From` başlığı yok: `(konu yok)` ve `(gönderen yok)` görünür. Test: Task 3 `missing_headers_fall_back`.
5. Sürüm etiketi `Cargo.toml` sürümüyle uyuşmuyor (`redmine-v0.2.0` ama paket `0.1.0`): iş akışı yayından önce durur. Test: Task 4 `tag_must_match_package_version`.

---

### Task 1: redmine plugin'i

**Files:**
- Modify: `focusdeck-plugins/Cargo.toml` (`members = ["plugins/*", "tools/*"]`)
- Create: `focusdeck-plugins/plugins/redmine/Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/client.rs`, `src/plugin.rs`
- Test: `focusdeck-plugins/plugins/redmine/tests/redmine.rs`

**Interfaces:**
- Consumes: `focusdeck_protocol::{Plugin, Describe, Item, ListParams, ActionParams, PROTOCOL_VERSION, serve}`.
- Produces: `focusdeck_redmine::plugin::Redmine` (`Default`), `pub fn to_item(issue: &Issue, base: &str, today: NaiveDate) -> Item`.

Ayar: `url` (zorunlu), `key_file` (zorunlu). `describe`: `actions: []`, `accepts_drop: false`, `reminders: false`. `action` her zaman `"bu kutuda eylem yok"` hatası döner.

İstek: `GET {url}/issues.json?assigned_to_id=me&status_id=open&sort=priority:desc,updated_on:desc&limit=100&offset=N`, başlık `X-Redmine-API-Key`. `offset` 0'dan başlar, gelen sayı kadar artar; boş sayfa ya da `offset >= total_count` olunca durur. Benzersiz kimlik sayısı son sayfadaki `total_count`'tan azsa `"Redmine: liste eksik geldi (<alınan>/<toplam>)"` hatası döner. `configure` anahtarı okur ve `GET {url}/users/current.json` ile doğrular (401 → `"Redmine: anahtar reddedildi"`).

Öğe: `id` = iş numarası, `title` = `subject`, `url` = `{url}/issues/{id}` (sondaki `/` kırpılır), `due` = `due_date` günü saat 17:00 yerel saat (spec örneği), `overdue` = `due_date < today`, `subtitle` = öncelik adının ` - ` öncesi (`"Ö1 - Kritik Etki"` → `Ö1`), gecikmişse ` · <gün>g gecikmiş` eklenir. `due_date` yoksa `due: None`, `overdue: false`. Sıra sunucunun sırasıdır.

- [ ] **Step 1: Testleri yaz** (`tests/redmine.rs`, mockito, anahtar `tempfile` 0600)
  - `lists_assigned_open_issues`: tek sayfa, iki iş; istek `X-Redmine-API-Key` başlığını ve yukarıdaki sorguyu taşır; öğeler sırayı korur.
  - `pages_until_total_count`: `total_count: 150`, 100 + 50 iş, iki istek (`offset=0`, `offset=100`), 150 öğe.
  - `incomplete_listing_is_an_error`: `total_count: 150`, ilk sayfa 100, ikinci sayfa boş → `Err("Redmine: liste eksik geldi (100/150)")`.
  - `duplicate_across_pages_counts_once`: ikinci sayfa ilk sayfadaki bir işi tekrar eder → eksik hatası.
  - `to_item_marks_overdue_with_days`: `today` = 2026-11-12, `due_date` 2026-11-08, öncelik `"Ö1 - Kritik Etki"` → `subtitle == "Ö1 · 4g gecikmiş"`, `overdue`, `due` = `2026-11-08T17:00` yerel.
  - `due_today_is_not_overdue`: `due_date` = `today` → `overdue == false`, `subtitle == "Ö1"`.
  - `rejected_key_fails_configure`: `users/current.json` 401 → `configure` `Err("Redmine: anahtar reddedildi")`, hata metninde anahtar değeri yok.
  - `empty_key_file_fails_configure`.
- [ ] **Step 2: Testleri koş, derlenmediğini gör**: `cargo test -p focusdeck-redmine` → derleme hatası.
- [ ] **Step 3: `client.rs` (`Client::new(url, key) -> Result<Client, String>`, `issues() -> Result<Vec<Issue>, String>`, `current_user() -> Result<(), String>`), `plugin.rs` ve `main.rs`'i yaz.** `Cargo.toml` Vikunja'nınkini izler: `description = "Bana atanmış açık Redmine işleri"`, protokol bağımlılığı şimdilik yerel yol.
- [ ] **Step 4: Testleri koş**: `cargo test -p focusdeck-redmine` → PASS; workspace testleri ve clippy temiz.
- [ ] **Step 5: Commit**: `Add Redmine plugin`

### Task 2: zulip plugin'i

**Files:**
- Create: `focusdeck-plugins/plugins/zulip/Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/client.rs`, `src/plugin.rs`
- Test: `focusdeck-plugins/plugins/zulip/tests/zulip.rs`

**Interfaces:**
- Consumes: Task 1 ile aynı protokol tipleri.
- Produces: `focusdeck_zulip::plugin::Zulip` (`Default`), `pub fn to_item(msg: &Message, base: &str) -> Item`.

Ayar: `url`, `email`, `key_file` (üçü zorunlu). Kimlik doğrulama HTTP Basic `email:anahtar`. Her istek `User-Agent: focusdeck-zulip/<sürüm>` taşır (Cloudflare imzasız istemciyi 1010 ile reddediyor). `describe`: `actions: ["unstar"]`, `accepts_drop: false`, `reminders: false`. `configure` `GET {url}/api/v1/users/me` ile doğrular.

Liste: `GET {url}/api/v1/messages` sorgusu `anchor=newest`, `num_before=1000`, `num_after=0`, `apply_markdown=false`, `narrow=[{"operator":"is","operand":"starred"}]`. `found_oldest` false ise bir sonraki istek `anchor=<en küçük id>`, `include_anchor=false`. Öğeler en yeni üstte.

Öğe: `id` = mesaj kimliği, `title` = içeriğin ilk boş olmayan satırı (baştaki `>` ve boşluk kırpılır), yoksa `(boş mesaj)`. `subtitle` = kanal mesajında `<gönderen> · #<kanal> > <konu>`, özel mesajda `<gönderen> · özel`. `url` = `{url}/#narrow/id/{id}`. `due: None`, `overdue: false`.

`unstar`: `POST {url}/api/v1/messages/flags`, form `messages=[<id>]`, `op=remove`, `flag=starred`. Başarıda `Ok(None)`.

- [ ] **Step 1: Testleri yaz** (`tests/zulip.rs`)
  - `lists_starred_newest_first`: istek Basic başlığını, `User-Agent` `focusdeck-zulip/` önekini ve yukarıdaki sorguyu taşır; kanal ve özel mesaj alt satırları beklenen metin.
  - `follows_pages_until_found_oldest`: ilk cevap `found_oldest: false`, ikinci istek `anchor=<en küçük>` ve `include_anchor=false`; toplam öğe iki sayfanın toplamı.
  - `blank_content_gets_placeholder_title`: içerik `"\n  \n"` → `(boş mesaj)`; içerik `"> alıntı\nasıl metin"` → `alıntı`.
  - `unstar_posts_remove_flag`: form gövdesi üç alanı taşır.
  - `unknown_action_is_error`.
  - `rejected_key_fails_configure`: 401 → `Err("Zulip: anahtar reddedildi")`.
- [ ] **Step 2: Testleri koş, derlenmediğini gör**: `cargo test -p focusdeck-zulip`.
- [ ] **Step 3: `client.rs` (`Client::new(url, email, key)`, `starred() -> Result<Vec<Message>, String>`, `unstar(id: u64)`, `me()`), `plugin.rs`, `main.rs`'i yaz.** `description = "Yıldızlı Zulip mesajları"`.
- [ ] **Step 4: Testleri koş**: PASS; workspace testleri ve clippy temiz.
- [ ] **Step 5: Commit**: `Add Zulip plugin`

### Task 3: gmail plugin'i ve `login` alt komutu

**Review:** per-task — security

**Files:**
- Create: `focusdeck-plugins/plugins/gmail/Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/oauth.rs`, `src/client.rs`, `src/plugin.rs`, `src/login.rs`
- Test: `focusdeck-plugins/plugins/gmail/tests/gmail.rs`, `src/oauth.rs` içinde birim testleri

**Interfaces:**
- Consumes: protokol tipleri.
- Produces: `focusdeck_gmail::plugin::Gmail` (`Default`), `pub fn to_item(msg: &Message, now: DateTime<Local>) -> Item`, `focusdeck_gmail::login::run(args: LoginArgs) -> Result<(), String>`, `oauth::pkce_challenge(verifier: &str) -> String`, `oauth::write_token(path: &Path, refresh_token: &str) -> io::Result<()>`.

Dosyalar bugünkü `pano` biçimini aynen okur, böylece geçiş kopyalamadır: istemci dosyası Google'ın `{"installed": {client_id, client_secret, auth_uri, token_uri}}` JSON'u (`web` anahtarı da kabul), token dosyası `{"refresh_token": "..."}`.

Ayar: `client_file`, `token_file` (zorunlu), `api_url` (isteğe bağlı, varsayılan `https://gmail.googleapis.com/gmail/v1/users/me`, testler için). Erişim token'ı yalnız bellekte durur, süresine 60 saniyeden az kalınca `token_uri`'ye `grant_type=refresh_token` ile yenilenir. Yenileme 400/401 dönerse hata `"Gmail oturumu düştü: focusdeck-gmail login"`. `describe`: `actions: []`, `accepts_drop: false`, `reminders: false`; `action` `"bu kutuda eylem yok"` döner.

Liste: `GET {api}/messages?q=is:starred&maxResults=100` ve `nextPageToken` bitene kadar `pageToken`; her mesaj için `GET {api}/messages/{id}?format=metadata&metadataHeaders=From&metadataHeaders=Subject`. Öğeler en eski üstte (`internalDate` artan).

Öğe: `id` = mesaj kimliği, `title` = `Subject` ya da `(konu yok)`, `subtitle` = `<From görünen adı, yoksa adres, ikisi de yoksa (gönderen yok)> · <yaş>`; yaş 60 dakikadan azsa `<n>dk`, 24 saatten azsa `<n>s`, değilse `<n>g`. `url` = `https://mail.google.com/mail/#all/{threadId}`. `overdue` = yaş ≥ 3 gün. `due: None`.

`login`: ikili `focusdeck-gmail login --client-file <yol> --token-file <yol> [--hint <e-posta>]` ile çağrılınca `serve` yerine bunu çalıştırır. Akış: 32 bayt `getrandom` → base64url verifier, `S256` challenge, 16 bayt state; `127.0.0.1:0` dinler; `auth_uri`'ye `client_id`, `redirect_uri=http://127.0.0.1:<port>/`, `response_type=code`, `scope=https://www.googleapis.com/auth/gmail.readonly`, `access_type=offline`, `prompt=consent`, `state`, `code_challenge`, `code_challenge_method=S256`, varsa `login_hint`. Adres stdout'a yazılır ve `xdg-open` ile açılır (açılmazsa sessiz geçer). 5 dakika içinde `code` ya da `error` taşıyan istek gelmezse `"5 dakikada onay gelmedi"`; `state` tutmazsa ya da `error` gelirse `"onay reddedildi"`. Kod `code_verifier` ile değiştirilir, cevapta `refresh_token` yoksa `"Google refresh token vermedi"`. Token dosyası `write_token` ile yazılır: üst klasör yoksa 0700 açılır, `<yol>.tmp` 0600 izinle oluşturulur, `rename` edilir. Tarayıcıya dönen sayfa düz metin `focusdeck: Gmail bağlandı, bu sekmeyi kapatabilirsin.`

- [ ] **Step 1: Testleri yaz**
  - `oauth.rs`: `pkce_challenge_matches_rfc7636_vector` (verifier `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk` → `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuWJ6S5jWLc`); `write_token_creates_0600_file` (izin 0600, içerik `{"refresh_token":"r"}`, `.tmp` kalmaz).
  - `tests/gmail.rs`: `configure_refreshes_access_token` (token isteği form alanları doğru); `lists_starred_oldest_first_across_pages` (iki sayfa, `Authorization: Bearer` başlığı); `missing_headers_fall_back`; `three_day_old_mail_is_overdue` (`to_item` saf testi, 71 saat değil, 72 saat evet); `revoked_refresh_token_tells_to_login` (400 → beklenen hata, metinde token yok); `access_token_is_renewed_near_expiry` (`expires_in: 30` → ikinci `list` yeniden yeniler).
  - `login.rs` içinde `callback_parses_code_and_state` (istek satırından `code`, `state`, `error` ayrıştırma) ve `mismatched_state_is_rejected`.
- [ ] **Step 2: Testleri koş, derlenmediğini gör**: `cargo test -p focusdeck-gmail`.
- [ ] **Step 3: Modülleri yaz.** Workspace bağımlılıklarına `sha2 = "0.11.1"`, `base64 = "0.22"`, `getrandom = "0.3"` eklenir. Callback sunucusu `std::net::TcpListener`, yalnız `GET` satırını okur. `description = "Yıldızlı Gmail mailleri"`.
- [ ] **Step 4: Testleri koş**: PASS; workspace testleri ve clippy temiz.
- [ ] **Step 5: Commit**: `Add Gmail plugin with login command`

### Task 4: index aracı, CI ve sürüm iş akışı (plugin reposu)

**Review:** per-task — security

**Files:**
- Create: `focusdeck-plugins/tools/focusdeck-index/Cargo.toml`, `src/main.rs`, `src/lib.rs`
- Create: `focusdeck-plugins/.github/workflows/ci.yml`, `.github/workflows/release.yml`, `index.toml` (boş dosya), `LICENSE` (MIT, `Ömer Taha Öztop`)
- Modify: `focusdeck-plugins/README.md`, `focusdeck-plugins/plugins/vikunja/tests/vikunja.rs` (`redmine.example.com` → `redmine.example.com`)

**Interfaces:**
- Produces: `focusdeck_index::{Entry, Target, set_entry(index: &str, entry: Entry) -> Result<String, String>, check_tag(tag: &str, name: &str, version: &str) -> Result<(), String>}`; ikili alt komutları `focusdeck-index set --index <dosya> --name <ad> --description <metin> --version <sürüm> --target <üçlü>=<url>,<sha256>...` (dosyayı yerinde yazar) ve `focusdeck-index check-tag --tag <etiket> --name <ad> --version <sürüm>`.

`set_entry` var olan girdiyi aynı adla değiştirir, ötekileri korur, çıktıyı ada göre sıralar; biçim spec'teki `index.toml` örneğiyle aynı (`[[plugin]]` + `[plugin.targets.<üçlü>]`), `protocol = 1`. sha256 64 küçük onaltılık değilse, url `https://` ile başlamıyorsa ya da hedef listesi boşsa hata. `check_tag` etiketin `<ad>-v<sürüm>` olmasını ister.

`ci.yml`: `push` ve `pull_request`; matris `ubuntu-24.04` ve `ubuntu-24.04-arm`; adımlar checkout, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`. `permissions: contents: read`.

`release.yml`: `on: push: tags: ['*-v*']`. `build` işi aynı iki koşucu matrisiyle: ad ve sürümü etiketten ayırır, `cargo metadata` ile paket sürümünü ve açıklamasını okur, `focusdeck-index check-tag` çalıştırır, `cargo build --release --locked -p focusdeck-<ad>`, ikiliyi `<ad>-<üçlü>` adıyla ve `sha256sum` çıktısını yükler (`actions/upload-artifact`). `publish` işi (`needs: build`, `permissions: contents: write`, `concurrency: index`): ikilileri indirir, `gh release create <etiket> <dosyalar> --title <etiket> --notes ""`, `main`'i çekip `focusdeck-index set` ile `index.toml`'u günceller (url `https://github.com/<repo>/releases/download/<etiket>/<ad>-<üçlü>`), `github-actions[bot]` adıyla `Update index for <etiket>` commit'leyip `main`'e gönderir. Üçüncü taraf eylemler yalnız `actions/checkout`, `actions/upload-artifact`, `actions/download-artifact`, sürüm etiketleriyle (`@v4`/`@v5` güncel olanı). Etiket değeri kabuğa doğrudan gömülmez, `env:` ile verilir.

README: kurulum (`focusdeck plugin install <ad>`), dört plugin'in ayar anahtarları, `focusdeck-gmail login` kullanımı, sürüm alma adımı (`git tag <ad>-v<sürüm> && git push origin <ad>-v<sürüm>`), mevcut "kum havuzu yok" uyarısı korunur.

- [ ] **Step 1: `focusdeck-index` testlerini yaz** (`src/lib.rs`): `set_into_empty_index`, `set_replaces_same_name_keeps_others_sorted`, `output_parses_as_index` (çıktı `toml` ile `plugin[].targets.<üçlü>.url/sha256` olarak okunur), `rejects_bad_sha256`, `rejects_non_https_url`, `tag_must_match_package_version` (`redmine-v0.1.0` + `0.1.0` geçer, `redmine-v0.2.0` ve `zulip-v0.1.0` hata).
- [ ] **Step 2: Koş, derlenmediğini gör**: `cargo test -p focusdeck-index`.
- [ ] **Step 3: Aracı yaz** (`toml`, `serde`, `clap` workspace bağımlılığı, sürümler çekirdekle aynı).
- [ ] **Step 4: Koş**: PASS.
- [ ] **Step 5: İş akışlarını, `index.toml`, `LICENSE`, README ve Vikunja test adresini yaz.** Doğrulama: `actionlint` kuruluysa `actionlint .github/workflows/*.yml` temiz; değilse `python3 -c 'import yaml,sys; [yaml.safe_load(open(f)) for f in sys.argv[1:]]' .github/workflows/*.yml` hatasız. iş alan adlarını arayan `grep -rIn` (`target` ve `.git` hariç) boş (`dist/` gitignore'da).
- [ ] **Step 6: Workspace testleri ve clippy temiz, commit**: `Add index tool and release workflows`

### Task 5: çekirdek reposunu yayına hazırla

**Files:**
- Create: `focusdeck/README.md`, `focusdeck/LICENSE` (MIT, `Ömer Taha Öztop`), `focusdeck/.github/workflows/ci.yml`
- Modify: `focusdeck/crates/focusdeck/src/capture.rs:215,317`, `focusdeck/crates/focusdeck/src/profile.rs:206-246` (`redmine.example.com` → `redmine.example.com`, `vikunja.example.com` → `vikunja.example.com`)

**Interfaces:**
- Produces: GitHub'a gönderilmeye hazır çekirdek; `focusdeck-protocol` sürümü `0.1.0` kalır.

README: ne olduğu (iki cümle), kurulum `cargo install --git https://github.com/omertahaoztop/focusdeck --tag v0.1.0 --locked focusdeck`, `~/.config/focusdeck/config.toml` ve örnek profil (spec'teki örnek, `example.com` adresleriyle), tuş ve fare tablosu (spec "Ekran ve etkileşim"), `capture` ve `remind` kullanımı, plugin'lerin kullanıcının yetkisiyle çalıştığı ve kum havuzu olmadığı uyarısı, protokol özeti ve Rust plugin yazarları için `focusdeck-protocol` bağımlılık satırı (`git`, `tag = "v0.1.0"`). `ci.yml` Task 4'teki CI ile aynı biçim.

- [ ] **Step 1: Adresleri değiştir, testleri koş**: `cargo test --workspace` PASS; iş alan adlarını arayan `grep -rIn` `crates` altında boş.
- [ ] **Step 2: README, LICENSE, `ci.yml` yaz**; `ci.yml` Task 4 Step 5'teki gibi doğrulanır.
- [ ] **Step 3: Clippy temiz, commit**: `Prepare repository for publishing`

### Task 6: GitHub'a gönder ve ilk sürümleri yayınla (controller, satır içi)

Kullanıcının makinesinde ve hesabında iş yaptığı için dağıtılmaz, controller yürütür. Plan onayı iki herkese açık repo açmayı ve göndermeyi kapsar.

- [ ] **Step 1: Sızıntı denetimi**: iki repoda `git log -p | grep -nE "api[_-]?key|refresh_token\"\s*:\s*\"[A-Za-z0-9]|Bearer [A-Za-z0-9]{20}"` gerçek değer göstermez; `~/.config/focusdeck/secrets` ve `~/.config/pano` içindeki değerlerin hiçbiri iki reponun geçmişinde geçmez (her değer için `git log -p -S` sayısı 0, değer ekrana basılmadan).
- [ ] **Step 2: Çekirdek**: `gh repo create omertahaoztop/focusdeck --public --source . --push`, `git tag v0.1.0 && git push origin v0.1.0`; CI iki koşucuda yeşil (`gh run watch`).
- [ ] **Step 3: Protokol bağımlılığını gite çevir**: dört plugin'in `Cargo.toml`'unda `focusdeck-protocol = { git = "https://github.com/omertahaoztop/focusdeck", tag = "v0.1.0" }`; `cargo test --workspace` PASS; commit `Depend on published focusdeck-protocol`.
- [ ] **Step 4: Plugin reposu**: `gh repo create omertahaoztop/focusdeck-plugins --public --source . --push`; CI yeşil. Etiketler `vikunja-v0.1.0`, `redmine-v0.1.0`, `zulip-v0.1.0`, `gmail-v0.1.0` tek tek gönderilir, her `release.yml` koşusu yeşil.
- [ ] **Step 5: Dizini doğrula**: `curl -s https://raw.githubusercontent.com/omertahaoztop/focusdeck-plugins/main/index.toml` dört girdi, her biri iki hedefli; bir ikili indirilip `sha256sum` dizindekiyle aynı.

### Task 7: canlı kurulum ve iş profili (controller, satır içi)

- [ ] **Step 1: Kayıt adresi**: `~/.config/focusdeck/config.toml` içindeki `file://` kaydı silinir, varsayılan GitHub dizini kullanılır. `focusdeck plugin install` ile `vikunja`, `redmine`, `zulip`, `gmail` kurulur; `focusdeck plugin list` dördünü `0.1.0` gösterir.
- [ ] **Step 2: Şifreler kopyalanır (0600, değer basılmadan)**: `~/.config/redmine/api-key` → `secrets/redmine`, `~/.config/pano/zulip-key` → `secrets/zulip`, `~/.config/pano/client_secret.json` → `secrets/gmail-client.json`, `~/.config/pano/gmail-token.json` → `secrets/gmail-token.json`. Kaynaklar yerinde kalır.
- [ ] **Step 3: Profil**: `is.toml` 3×2 ızgara. Satır 0: `sozlu` (0,0), `bekliyor` (0,1), `redmine` (0,2; `title = "Redmine planım"`, `plan_into = "sozlu"`, `url = "https://redmine.example.com"`). Satır 1: `mail` (1,0; `title = "Mail"`), `zulip` (1,1; `title = "Zulip"`, `url = "https://zulip.example.com"`, `email = "user@example.com"`), `fikir` (1,2, `width` kalkar).
- [ ] **Step 4: Canlı denetim (tmux 150×40, `env -u NO_COLOR`)**: altı kutu hatasız açılır; Redmine büyütülünce `total_count` kadar iş; boşluk bir işi Sözlü işler'e alır, ikinci boşluk görevi siler; Zulip öğesinde `o` doğru mesajı açar (`#narrow/id/<id>`), bir yıldız `u` ile kalkar ve API ile geri eklenir; Mail kutusu yıldızlı mailleri en eski üstte gösterir. ghostty ekran görüntüsü alınır.
- [ ] **Step 5: Kayıt**: brain'e focusdeck karar kaydı (`journal_write kind=decision`), `wiki/musteri-iletisim.md` içindeki `pano` satırı focusdeck'i anlatacak şekilde güncellenir, wiki denetimi koşar.
