# Seenboard'a geçiş uygulama planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** focusdeck'in üç reposu, CT 147'deki sunucusu, Caddy adresi ve masaüstü kurulumu Seenboard adıyla, veri kaybı olmadan çalışır.

**Architecture:** Kod değişikliği üç repoda ayrı branch'lerde yapılır (Task 1, 2, 4). Çekirdek `v0.5.0` olarak yayımlanmadan plugin'ler yeni crate'lere bağlanamaz, sıralama buna göre. Canlı adımlar (Task 3 ve 5-10) ana ajanda koşar: her makinede önce arşiv, sonra `mv` ve yeniden kurulum, sonra kanıtla doğrulama. Uyumluluk katmanı yok.

**Tech Stack:** Rust 2024 workspace (reqwest, axum, clap, fluent), Kotlin + Compose (OkHttp 5.3.2, Gradle 9.8.2, JDK 21), GitHub Actions, Forgejo Actions (Caddy), Proxmox `pct`, systemd, nftables.

**Spec:** `docs/superpowers/specs/2026-11-14-seenboard-rename-design.md`

## Global Constraints

- Görünen ad `Seenboard`, her tanımlayıcı `seenboard` (binary, crate, env, XDG dizini, paket, birim, Android paketi).
- `PROTOCOL_VERSION` 1 → 2. Env adları `SEENBOARD_LANG`, `SEENBOARD_PROFILE`, `SEENBOARD_CONFIG_DIR`, `SEENBOARD_DATA_DIR`, `SEENBOARD_STATE_DIR`.
- Sürümler: çekirdek `v0.5.0`, plugin'ler `0.3.0` (`seenboard-protocol`/`seenboard-i18n` `tag = "v0.5.0"`), Android `v0.1.0`.
- Eski adla uyumluluk yok: `focusdeck` dizini, `FOCUSDECK_*` env, takma ad, sembolik bağ okunmaz ya da üretilmez.
- REST API değişmez: `/api/v1/...`, JSON alanları, token.
- Tarihli belgeler (`docs/plans/`, `docs/superpowers/specs/`, `.superpowers/`, ops-journal kayıtları) değişmez.
- Yeni adres `https://seenboard.omertahaoztop.tr` (+ `.biz.tr`), eski adres 308 ile yönlendirir, istemciler yönlendirme izlemez.
- Türkçe metinde ek okunuşa göre: `Seenboard'a`, `Seenboard'un`, `Seenboard'da`.
- Commit: İngilizce, emir kipi, tek satır ≤72 karakter, gövde yok, dosyalar yoluyla stage edilir. `focusdeck/docs/plans/2026-11-12-focusdeck-gowa-whatsapp.md` hiç stage edilmez.
- Gizli dosyalar 0700 dizinde 0600 kalır. Secret terminale basılmaz.
- Public push, `gh repo rename` ve Caddy push'undan hemen önce Ömer'e tek satır haber verilir.

## Review Focus

1. **Taşımadan sonra eski plugin kalmış makine (ofis laptopu, unutulan bir kutu):** kutu `protocol mismatch: core 2, plugin 1` yazıp duruyor. Kullanıcı ne yapacağını bilmeli: mesaj `seenboard plugin install` ile yeniden kurmayı söyler (Task 1 Step 1).
2. **Yeni plugin eski indeks girdisiyle:** indekste protokol 1 olarak kalmış bir girdiyi `plugin install` kurmamalı, sürüm uyuşmazlığı vermeli. `install.rs:256` bunu zaten yapıyor, Task 4'te indeks aracının protokolü çekirdekten alması bunu kalıcı kılar (Task 4 Step 1).
3. **Taşınan dizinlerin izinleri:** `mv` ve arşivden sonra `secrets/` 0700, dosyaları 0600, sahibi değişmemiş. Task 6 ve 7'nin doğrulamasında `stat` var.
4. **Eski adresi kullanan istemci:** 308 aldığında token'ı yeni hosta taşımamalı, anlaşılır bir hata göstermeli. Masaüstünde `remote.rs` `redirects_are_not_followed` testi var. Android'de `HttpSeenboardApiTest`'e 308 durumu eklenir (Task 2 Step 1).
5. **Yeniden adlandırmadan sonra unutulmuş `focusdeck` dizesi:** bir test fikstürü ya da demo plugin'i eski env adını okursa, test sessizce varsayılana düşüp geçebilir. Her repo görevinin sonunda `rg -i focusdeck` koşulur, izin verilen eşleşmeler listelenir.

---

### Task 1: Çekirdek repo (`focusdeck` → `seenboard`)

**Review:** per-task — persisted format (XDG dizinleri, env adları, protokol sürümü)

**Files:**
- Rename: `crates/focusdeck` → `crates/seenboard`, `crates/focusdeck-protocol` → `crates/seenboard-protocol`, `crates/focusdeck-i18n` → `crates/seenboard-i18n` (`git mv`)
- Modify: `Cargo.toml` (workspace üyeleri), üç crate'in `Cargo.toml`'u, `crates/fake-plugin/Cargo.toml`, `crates/fake-plugin/src/main.rs:218`
- Modify: `crates/seenboard-protocol/src/types.rs:5` (`PROTOCOL_VERSION: u32 = 2`)
- Modify: `crates/seenboard/src/paths.rs:29-108`, `src/main.rs:95,124,153`, `src/host.rs:210-211,414`, `src/config.rs:9`
- Modify: `crates/seenboard-i18n/src/lib.rs:13,161-188`, `src/build.rs:58,62`, `crates/seenboard/build.rs:2`
- Modify: `crates/seenboard/locales/en.ftl`, `tr.ftl` (satır 116-204 arası komut ve başlık metinleri, `host-protocol-mismatch`)
- Modify: `README.md`, `README.tr.md`, `docs/screenshots/demo/README.md`, `docs/screenshots/demo/data/plugins/demo`
- Test: `crates/seenboard/src/paths.rs` (iç testler), `crates/seenboard/tests/*.rs`, `crates/seenboard-protocol/tests/serve.rs`

**Interfaces:**
- Produces: crate'ler `seenboard`, `seenboard-protocol`, `seenboard-i18n`; Rust adları `seenboard_protocol::PROTOCOL_VERSION == 2`, `seenboard_i18n::PLUGIN_ENV == ["SEENBOARD_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]`, `seenboard_protocol::serve`. Task 4 bunlara `tag = "v0.5.0"` ile bağlanır.
- Produces: varsayılan indeks `https://raw.githubusercontent.com/omertahaoztop/seenboard-plugins/main/index.toml`.

- [ ] **Step 1: Testleri yeni adlara çevir, yeni beklentileri ekle**

`paths.rs` testleri: `SEENBOARD_CONFIG_DIR=/c` vb. kullanılır; varsayılanlar `/h/.config/seenboard`, `/h/.local/share/seenboard`, `/h/.local/state/seenboard`; `log_file() == /s/seenboard.log`; hata metni `SEENBOARD_STATE_DIR is not set and no home directory was found`. Ek test: yalnız `FOCUSDECK_CONFIG_DIR=/eski` verildiğinde `config_dir == /h/.config/seenboard` (eski env yok sayılır).

`crates/seenboard/tests/host.rs`: protokol 1 bildiren fake plugin `HostError::Protocol { expected: 2, got: 1 }` döner ve ekrana çıkan metin `seenboard plugin install` içerir. `host-protocol-mismatch` metni (en): `protocol mismatch: core { $expected }, plugin { $got }; reinstall it with seenboard plugin install`. tr karşılığı aynı anlamda.

Diğer testlerdeki `FOCUSDECK_*` env adları `SEENBOARD_*` olur.

- [ ] **Step 2: Testlerin derlenmediğini ya da kırıldığını gör**

Run: `cargo test --workspace --locked 2>&1 | tail -20`
Expected: FAIL (crate adları ve env adları henüz eski).

- [ ] **Step 3: Yeniden adlandırmayı uygula**

`git mv` ile dizinler, ardından crate adları, `use` yolları, env sabitleri, `paths.rs:29` varsayılan alt dizini, log adı, `__FOCUSDECK_I18N_*` → `__SEENBOARD_I18N_*`, üretilen dosya `seenboard_i18n.rs`, `config.rs:9` indeks adresi. Locale'lerde komutlar `seenboard …`, bildirim başlıkları `Seenboard: …`. `Cargo.lock` `cargo update -w` ile yenilenir, dış bağımlılık sürümü değişmez. README'ler: başlık `# Seenboard`, komutlar, yollar, `cargo install --git https://github.com/omertahaoztop/seenboard --tag v0.5.0 --locked seenboard`, plugin örneği `seenboard-protocol = { git = "https://github.com/omertahaoztop/seenboard", tag = "v0.5.0" }`, Caddy örneği. Sona "focusdeck'ten geçiş" / "Migrating from focusdeck" bölümü: eski ad, üç `mv` komutu, profil ve `config.toml`'daki yolların düzeltilmesi, `seenboard plugin install <ad>` ile yeniden kurulum.

- [ ] **Step 4: Tam takım**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
Expected: PASS, 0 failed.

- [ ] **Step 5: Kalan eski ad**

Run: `git grep -i -l focusdeck -- . ':!docs/plans' ':!docs/superpowers' ':!.superpowers'` (yeni dosyalar `git add` sonrası görünür, yerleşik `rg` `--glob '!…'` dışlamasını uygulamıyor)
Expected: yalnız `README.md` ve `README.tr.md` (geçiş bölümü), `crates/seenboard/src/paths.rs` (eski env'in yok sayıldığı test).

- [ ] **Step 6: Commit**

`git mv` sonuçları ve değişen dosyalar yoluyla stage edilir. Mesaj: `Rename focusdeck to Seenboard and bump protocol to 2`

### Task 2: Android repo (`focusdeck-android` → `seenboard-android`)

**Review:** per-task — security (imza anahtarı, Keystore alias'ı)

**Files:**
- Rename: `app/src/{main,test,androidTest}/java/tr/omertahaoztop/focusdeck` → `…/seenboard` (`git mv`)
- Rename: `FocusdeckApi.kt`, `HttpFocusdeckApi.kt`, `FocusdeckRoot` içeren dosya, `FocusdeckApp`, test dosyaları `HttpFocusdeckApiTest.kt` vb.
- Modify: `settings.gradle.kts:23`, `app/build.gradle.kts:8,13`, `app/src/main/AndroidManifest.xml:13,19`, `res/values/themes.xml`, `res/values-night/themes.xml`, `res/values/strings.xml:3,5`, `res/values-tr/strings.xml:3,5`
- Modify: `settings/TokenCipher.kt:19` (alias `seenboard-token`), `reminders/AndroidAlarms.kt:104` (`seenboard://reminder/`)
- Modify: `.github/workflows/release.yml:56-61`, `README.md`, `README.tr.md`
- Test: `app/src/test/java/tr/omertahaoztop/seenboard/api/ServerUrlTest.kt:12`, `HttpSeenboardApiTest.kt`

**Interfaces:**
- Produces: `applicationId = "tr.omertahaoztop.seenboard"`, sınıflar `SeenboardApi`, `HttpSeenboardApi`, `SeenboardRoot`, `SeenboardApp`, tema `Theme.Seenboard`, APK adı `dist/seenboard-$version.apk`. Task 8 ve Task 12 bu adları kullanır.

- [ ] **Step 1: Testleri yeni adlara çevir, 308 testini ekle**

`ServerUrlTest`: `https://seenboard.omertahaoztop.tr/` kabul edilir. `HttpSeenboardApiTest`'e: MockWebServer bir isteğe `308` ve `Location: https://evil.example/api/v1/profiles` dönerse istemci ikinci isteği yapmaz (`server.requestCount == 1`) ve sonuç hata olur, başarılı değil. Diğer testlerde paket ve sınıf adları.

- [ ] **Step 2: Testlerin kırıldığını gör**

Run: `./gradlew testDebugUnitTest 2>&1 | tail -20`
Expected: FAIL (derleme hatası, sınıflar henüz eski adda).

- [ ] **Step 3: Yeniden adlandırmayı uygula**

`git mv` ile paket dizinleri ve sınıf dosyaları, `package`/`import` satırları, Gradle `namespace`/`applicationId`, `rootProject.name = "seenboard-android"`, manifest, temalar. Metinler: `app_name` her iki dilde `Seenboard`; `setup_title` en `Connect to Seenboard`, tr `Seenboard\'a bağlan`. Release iş akışında APK adı `seenboard-$version.apk`.

- [ ] **Step 4: Yeni imza anahtarı**

`~/.cache/seenboard-android-signing/` (0700) altında `keytool -genkeypair -storetype PKCS12 -keyalg RSA -keysize 4096 -validity 10000 -alias seenboard -dname "CN=seenboard-android"`, parola `openssl rand -base64 32` ile üretilip yalnız dosyada durur (0600), terminale basılmaz. README'lerdeki sertifika SHA-256 paragrafı yeni sertifikanın özetiyle değişir (`keytool -list -v` çıktısından). Eski `~/.cache/focusdeck-android-signing` Task 8'den sonra `trash-put` ile silinir.

- [ ] **Step 5: Tam takım**

Run: `./gradlew lint testDebugUnitTest assembleDebug compileDebugAndroidTestKotlin`
Expected: BUILD SUCCESSFUL, 0 failed.

- [ ] **Step 6: Kalan eski ad**

Run: `git grep -i -l focusdeck -- . ':!.superpowers'`
Expected: boş.

- [ ] **Step 7: Commit ve yerel dizin**

Mesaj: `Rename the app to Seenboard`. Commit'ten sonra ana ajan dizini taşır: `mv ~/Desktop/personal/focusdeck-android ~/Desktop/personal/seenboard-android`.

### Task 3: Çekirdeği yayımla (ana ajan)

**Files:** yok (git ve GitHub işlemleri)

**Interfaces:**
- Consumes: Task 1'in branch'i.
- Produces: `https://github.com/omertahaoztop/seenboard` üzerinde `v0.5.0` etiketi. Task 4 buna bağlanır.

- [ ] **Step 1:** Task 1 branch'i `main`'e birleşir, tam takım `main` üzerinde bir kez daha yeşil.
- [ ] **Step 2:** Ömer'e haber, sonra `gh repo rename seenboard --repo omertahaoztop/focusdeck --yes`; `git remote set-url origin https://github.com/omertahaoztop/seenboard`; `mv ~/Desktop/personal/focusdeck ~/Desktop/personal/seenboard`.
- [ ] **Step 3:** `main` ve `v0.5.0` (annotated değil, mevcut etiketlerle aynı tür) HTTPS ile itilir.
- [ ] **Step 4:** Doğrulama: `gh api repos/omertahaoztop/focusdeck --jq .full_name` `omertahaoztop/seenboard` döner (yönlendirme), `git ls-remote --tags origin v0.5.0` etiketi gösterir, `cargo install --git https://github.com/omertahaoztop/seenboard --tag v0.5.0 --locked seenboard --root /tmp/sb-check` derlenir ve `/tmp/sb-check/bin/seenboard --version` `0.5.0` basar. `/tmp/sb-check` `trash-put` ile silinir.

### Task 4: Plugin reposu (`focusdeck-plugins` → `seenboard-plugins`)

**Review:** per-task — persisted format (indeks `protocol` alanı)

**Files:**
- Rename: `tools/focusdeck-index` → `tools/seenboard-index` (`git mv`)
- Modify: `Cargo.toml` (workspace, `seenboard-plugin-common`), `crates/plugin-common/Cargo.toml:2,4`, `plugins/*/Cargo.toml` (paket adı `seenboard-<ad>`, `version = "0.3.0"`, bağımlılıklar), `plugins/*/src/*.rs`, `plugins/*/tests/*.rs`, `plugins/*/build.rs`, `plugins/gmail/locales/*.ftl`
- Modify: `tools/seenboard-index/Cargo.toml`, `src/lib.rs:6,102,208`, `src/main.rs:6,9`
- Modify: `.github/workflows/release.yml:41-52,124-141`, `scripts/local-index.sh`, `README.md`, `README.tr.md`
- `index.toml` elle değişmez: release iş akışı günceller.

**Interfaces:**
- Consumes: `seenboard-protocol`, `seenboard-i18n` `{ git = "https://github.com/omertahaoztop/seenboard", tag = "v0.5.0" }`.
- Produces: paketler `seenboard-gmail`, `seenboard-redmine`, `seenboard-vikunja`, `seenboard-whatsapp`, `seenboard-zulip`, `seenboard-plugin-common`, `seenboard-index`; release dosya adları değişmez (`<ad>-<triple>`).

- [ ] **Step 1: Testleri çevir**

`seenboard-index` testi: `set_entry` ile yazılan girdinin `protocol` alanı `seenboard_protocol::PROTOCOL_VERSION` (2) olur. `const PROTOCOL` silinir, araç `seenboard-protocol`'e bağlanır. Plugin testlerindeki `FOCUSDECK_LANG` → `SEENBOARD_LANG`, describe testlerinde beklenen protokol 2.

- [ ] **Step 2: Testlerin kırıldığını gör**

Run: `cargo test --workspace --locked 2>&1 | tail -20`
Expected: FAIL.

- [ ] **Step 3: Yeniden adlandırmayı uygula**

Paket adları, `use` yolları, `User-Agent` dizileri (`seenboard-gmail/…`), release iş akışında `seenboard-$name`/`seenboard-index`, yerel indeks script'inde `target/release/seenboard-$name`. README'lerde ad, kurulum komutu `seenboard plugin install`, repo bağlantıları, kısa geçiş notu. `Cargo.lock` yenilenir.

- [ ] **Step 4: Tam takım**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && bash scripts/local-index.sh`
Expected: PASS, `dist/index.toml` beş girdi, hepsi `protocol = 2`, `version = "0.3.0"`.

- [ ] **Step 5: Kalan eski ad**

Run: `git grep -i -l focusdeck -- . ':!index.toml'`
Expected: yalnız README'ler (geçiş notu). `index.toml` eski URL'leri Task 5'teki release'ler yazınca değişir.

- [ ] **Step 6: Commit**

Mesaj: `Rename plugins to seenboard and target protocol 2`. `dist/` stage edilmez.

### Task 5: Plugin'leri yayımla (ana ajan)

**Files:** yok

**Interfaces:**
- Consumes: Task 4'ün branch'i, Task 3'ün `v0.5.0` etiketi.
- Produces: `seenboard-plugins` `main` üzerindeki `index.toml`'da beş girdi, `0.3.0`, `protocol = 2`, URL'ler `…/seenboard-plugins/releases/download/<ad>-v0.3.0/…`.

- [ ] **Step 1:** Task 4 branch'i `main`'e birleşir. Ömer'e haber, `gh repo rename seenboard-plugins --repo omertahaoztop/focusdeck-plugins --yes`, `git remote set-url origin git@github.com:omertahaoztop/seenboard-plugins.git`, `mv ~/Desktop/personal/focusdeck-plugins ~/Desktop/personal/seenboard-plugins`. `main` itilir.
- [ ] **Step 2:** Etiketler iki push'ta (bir push'ta en çok 3): önce `gmail-v0.3.0 redmine-v0.3.0 vikunja-v0.3.0`, release iş akışları bitince `whatsapp-v0.3.0 zulip-v0.3.0`.
- [ ] **Step 3:** Doğrulama: `gh run list --workflow release.yml --limit 5` beşi de `success`. `git pull`, `index.toml` beş girdi `0.3.0`/`protocol = 2`. Her girdinin URL'si 200 döner (`curl -sIL -o /dev/null -w '%{http_code}'`), indirilen dosyanın `sha256sum`'ı indeksle aynı.

### Task 6: Sunucu (CT 147) ve Caddy (ana ajan)

**Review:** per-task — data migration (komut listesi çalıştırılmadan önce reviewer'a gider)

**Files:**
- Create: `local://seenboard-server-migration.md` (adım adım komutlar, review için)
- Rename: `~/Desktop/personal/Caddy/vhost.d/deck.conf` → `vhost.d/seenboard.conf`
- Uzak: CT 147 içinde `/etc/systemd/system/focusdeck.service` → `seenboard.service`, `/etc/nftables.conf`, `/home/seenboard/.config/seenboard/{config.toml,profiles/pano.toml}`

**Interfaces:**
- Consumes: Task 3'ün `v0.5.0` etiketi, Task 5'in indeksi.
- Produces: `https://seenboard.omertahaoztop.tr/api/v1/...` aynı token'la çalışır. Eski adres 308.

- [ ] **Step 1: Önce ölç.** Token'lı `GET /api/v1/profiles/pano` ile kutu listesi, her kutu için `…/items` öğe sayısı `local://seenboard-before.json`'a. CT içinde `sha256sum` ile `local.json` ve `idempotency.json` özetleri.
- [ ] **Step 2: Komut listesini yaz, reviewer'a ver.** `local://seenboard-server-migration.md`: snapshot, derleme, durdurma, arşiv, `usermod`/`groupmod`, `mv`, yol düzeltmeleri, plugin kurulumu, birim, nft, hostname, başlatma, geri dönüş (`pct rollback 147 pre-seenboard`). Reviewer onayı gelmeden Step 3'e geçilmez.
- [ ] **Step 3: Derle.** pve11'de `docker run --rm -v /tmp/sb-build:/out rust:1-trixie cargo install --git https://github.com/omertahaoztop/seenboard --tag v0.5.0 --locked seenboard --root /out`, ardından `pct push 147 /tmp/sb-build/bin/seenboard /usr/local/bin/seenboard --perms 0755`.
- [ ] **Step 4: Taşı.** Host: pve11 (192.168.11.11), CT 147. Sıra: `pct snapshot 147 pre-seenboard`; `systemctl stop focusdeck`; `tar -C /home -cpf /root/seenboard-migrate-20261114.tar focusdeck` (0600); `usermod -l seenboard -d /home/seenboard -m focusdeck`; `groupmod -n seenboard focusdeck`; `runuser -u seenboard --` ile `mv ~/.config/focusdeck ~/.config/seenboard`, `~/.local/share` ve `~/.local/state` için aynısı, `mv …/focusdeck.log …/seenboard.log`; `config.toml` ve `profiles/pano.toml`'da `~/.config/focusdeck/` → `~/.config/seenboard/`.
- [ ] **Step 5: Birim, ağ, ad.** `seenboard.service` eski birimin kopyası, yalnız `User=seenboard`, `ExecStart=/usr/local/bin/seenboard server`, `ReadWritePaths=/home/seenboard` değişir; `Description=` eklenmez. Eski birim `disable` ve sil, `daemon-reload`, `enable --now seenboard`. nft tablosu `inet seenboard`, `nft -c -f /etc/nftables.conf` temizse `systemctl reload nftables`. `pct set 147 --hostname seenboard`, `pct reboot 147`.
- [ ] **Step 6: Plugin'ler.** `runuser -u seenboard -- seenboard plugin install gmail redmine vikunja whatsapp zulip` (komut tek adı alıyorsa beş ayrı çağrı), `seenboard plugin list` hepsi `0.3.0`.
- [ ] **Step 7: Caddy.** `git mv vhost.d/deck.conf vhost.d/seenboard.conf`; ana blok `seenboard.omertahaoztop.tr seenboard.omertahaoztop.biz.tr`, `import log seenboard`, gerisi aynı; ikinci blok `deck.omertahaoztop.tr deck.omertahaoztop.biz.tr { import common  redir https://seenboard.omertahaoztop.tr{uri} 308 }`; dosya başındaki tek satırlık yorumda `focusdeck` → `Seenboard`. Commit `Move the board vhost to seenboard.omertahaoztop.tr`. Ömer'e haber, Forgejo'ya push, CI'nin `success` bitmesini bekle.
- [ ] **Step 8: Doğrula.** CT 147: `systemctl is-active seenboard nftables` ikisi `active`, `hostname` `seenboard`, `stat -c '%a %U' ~/.config/seenboard/secrets ~/.config/seenboard/secrets/*` dizin `700`, dosyalar `600 seenboard`, `local.json`/`idempotency.json` özetleri Step 1'le aynı. Dışarıdan: `https://seenboard.omertahaoztop.tr/api/v1/profiles` token'sız 401, token'la 200; dokuz kutunun `items` çağrısı 200 ve yerel kutu öğe sayıları Step 1'le aynı; `https://deck.omertahaoztop.tr/api/v1/profiles` 308 ve `Location` yeni adres. 8740 ev ağından zaman aşımı.
- [ ] **Step 9: Temizlik.** `pct delsnapshot 147 pre-seenboard`, CT'de `/usr/local/bin/focusdeck` ve `/root/seenboard-migrate-20261114.tar` silinir, pve11'de `/tmp/sb-build` silinir.

### Task 7: Masaüstü (archlinux-home, ana ajan)

**Review:** per-task — data migration (komut listesi çalıştırılmadan önce reviewer'a gider)

**Files:**
- Create: `local://seenboard-desktop-migration.md`
- Rename: `~/.config/focusdeck` → `~/.config/seenboard`, `~/.local/share/focusdeck` → `~/.local/share/seenboard`, `~/.local/state/focusdeck` → `~/.local/state/seenboard`, `~/.local/share/noctalia/plugins/focusdeck` → `…/seenboard`
- Rename: `~/.config/systemd/user/focusdeck-remind.{service,timer}` → `seenboard-remind.{service,timer}`
- Modify: `~/.config/seenboard/config.toml` (`[server] url`, `token_file`), `~/.config/seenboard/profiles/pano.toml:46,56,94`, `~/.config/hypr/keybind.lua:83,86`, Noctalia `plugin.toml:1,2,10`, `widget.luau:18,22`
- Delete: `~/.config/seenboard/config.toml.bak-20261114` (Plan B'den kalan yedek, `trash-put`)

**Interfaces:**
- Consumes: Task 3'ün `v0.5.0` etiketi, Task 5'in indeksi, Task 6'nın yeni adresi.

- [ ] **Step 1: Komut listesini yaz, reviewer'a ver.** Sıra: timer'ı durdur; `tar -cpf ~/seenboard-migrate-20261114.tar -C ~ .config/focusdeck .local/share/focusdeck .local/state/focusdeck` (0600); `cargo install --git https://github.com/omertahaoztop/seenboard --tag v0.5.0 --locked seenboard --root ~/.local`; üç `mv`; log adı; yol ve URL düzeltmeleri; plugin kurulumu; birimler; Hyprland; Noctalia; geri dönüş (arşivi aç, eski birimi etkinleştir).
- [ ] **Step 2: Uygula.** Reviewer onayından sonra. Birimler: `ExecStart=%h/.local/bin/seenboard remind --send`, `systemctl --user daemon-reload`, `enable --now seenboard-remind.timer`. Hyprland: `~/.local/bin/seenboard capture`, `kitty --class seenboard --title Seenboard ~/.local/bin/seenboard`, `hyprctl reload`. Noctalia: `id = "omertahaoztop/seenboard"`, `name = "Seenboard"`, açıklama ve iki komut `seenboard`. Noctalia'yı yeniden yükleme yöntemi `skill://noctalia-plugin-live-testing`'den alınır.
- [ ] **Step 3: Doğrula.** `seenboard --version` `0.5.0`; `stat` ile `secrets/` 700, dosyalar 600; `seenboard plugin list` beşi `0.3.0`; `systemctl --user is-active seenboard-remind.timer` `active`; TUI kitty'de açılır ve ekran görüntüsü Task 6 Step 1'deki kutu sayılarıyla aynı panoyu gösterir; `seenboard capture "seenboard geçiş denemesi"` ile yakalanan not API'de görünür, sonra TUI'den bitirilir; Noctalia düğmesi sol tıkta yakalama penceresini, sağ tıkta panoyu açar (ekran görüntüsü).
- [ ] **Step 4: Temizlik.** `~/.local/bin/focusdeck`, eski birim dosyaları, `~/seenboard-migrate-20261114.tar`, `config.toml.bak-20261114` `trash-put` ile silinir. `systemctl --user list-unit-files 'focusdeck*'` boş.

### Task 8: Android emülatör smoke'u ve Task 12'ye devir (ana ajan)

**Files:** yok

**Interfaces:**
- Consumes: Task 2'nin APK'sı, Task 6'nın adresi.

- [ ] **Step 1:** Deneme sunucusu `seenboard server` (v0.5.0, profil `deneme`, `127.0.0.1:8740`) olarak yeniden başlatılır. `/tmp/fd-emu` script'leri yeni binary'ye çevrilir.
- [ ] **Step 2:** Emülatörde eski `tr.omertahaoztop.focusdeck` kaldırılır, `tr.omertahaoztop.seenboard` kurulur. Kontrol listesi: kurulum ekranı başlığı `Seenboard'a bağlan`, launcher adı `Seenboard`, pano, not ekleme, sola kaydırarak bitirme + undo, uzun bas + sürükle, hatırlatıcı alarmı (`dumpsys alarm` içinde `tr.omertahaoztop.seenboard`).
- [ ] **Step 3:** Android planının Task 12'si yeni adlarla yürür: `gh repo create omertahaoztop/seenboard-android --public` öncesi Ömer'e haber, secret'lar yeni imza anahtarından, `v0.1.0`, telefona kurulumda adres `https://seenboard.omertahaoztop.tr`. Bitwarden kaydı yeni anahtarla, Ömer yapar.
- [ ] **Step 4:** Emülatör ve deneme sunucusu temizliği (hub `fd-test-server`, `fd-emulator`, `/tmp/fd-emu`, `~/.local/share/android-sdk-emu`, `~/.android/avd/fd-fold*`), eski imza dizini `~/.cache/focusdeck-android-signing` `trash-put` ile silinir.

### Task 9: ops-journal (ana ajan)

**Files:**
- Move: `wiki/focusdeck-sunucu.md` → `wiki/seenboard-sunucu.md` (`bm_move`)
- Modify (`bm_edit`): `wiki/seenboard-sunucu.md`, `wiki/kisisel-repolar.md`, `wiki/kisisel-sunucular.md`, `wiki/gowa.md`, `wiki/musteri-iletisim.md`, `runbooks/2026-11-13-focusdeck-ofis-laptopu-kurulumu.md`, `NOW.md`

- [ ] **Step 1:** `bm_move`, ardından her sayfada `bm_read output_format=json` → `bm_edit` (`expected_checksum` ile). İçerik: ad, CT hostname/kullanıcı/yollar, birim, nft, adres, plugin sürümleri `0.3.0`, repo adları. `gowa.md`'de GOWA'nın kendi `focusdeck` kullanıcısı ve cihaz kimliği kalır, yalnız pano tarafı ve `[[seenboard-sunucu]]` bağlantısı değişir. `## Kaynak` bölümüne bu plan ve commit'ler.
- [ ] **Step 2:** Karar kaydı `journal_write kind=decision`: ad değişikliği, gerekçe, kapsam dışı bırakılanlar.
- [ ] **Step 3:** `ssh brain 'cd /srv/ops-journal && runuser -u brain -- python3 tools/wiki/check.py'` temiz; `bm_search query="focusdeck-sunucu"` kırık bağlantı göstermez.

### Task 10: Ofis laptopu ve eski adresin kapatılması (laptop açılınca, ana ajan)

**Review:** per-task — data migration

**Files:**
- Delete: Caddy `vhost.d/seenboard.conf` içindeki `deck.*` bloğu
- Uzak: laptop `192.168.10.63` (`~/.local/share/focusdeck/local.json`, `~/.config/focusdeck/`)

- [ ] **Step 1:** Laptop erişilebilir olunca: `systemctl stop seenboard` (CT 147); laptoptaki `local.json` ile sunucudaki `jq` ile birleştirilir, aynı `id` iki tarafta varsa durulur ve Ömer'e sorulur; `systemctl start seenboard`.
- [ ] **Step 2:** Laptopta arşiv, `seenboard` 0.5.0 kurulumu, dizinlerin taşınması, `config.toml`'a `[server] url = "https://seenboard.omertahaoztop.tr"` ve `token_file`, kısayollar (`rofi`, `ghostty`, Noctalia `Mindset` bar'ı) yeni adla. Doğrulama Task 7 Step 3 ile aynı.
- [ ] **Step 3:** Caddy'den `deck.*` bloğu silinir (commit `Drop the deck redirect`), Ömer'e haber, push, CI `success`, `https://deck.omertahaoztop.tr` artık bu vhost'a düşmez. Wiki'de laptop ve adres satırları güncellenir, `NOW.md`'deki laptop maddesi kapanır.
