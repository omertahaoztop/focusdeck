# focusdeck'in adını Seenboard yapma tasarımı

Tarih: 2026-11-14 · Durum: kullanıcı onayı bekliyor · Kapsam: `focusdeck`, `focusdeck-plugins`, `focusdeck-android` repoları, CT 147'deki sunucu, masaüstü ve ofis laptopu

## Neden

"FocusDeck" adı başkasında: aynı işi yapan bir günlük görev önceliklendirme uygulaması var (focus.wisdomdeck.com), yanında focusdeck.us ve GitHub'da `focus-deck` adlı bir repo duruyor. Ömer projeyi herkese açık yayımlıyor (GitHub, ileride Android APK). Bu yüzden ad çakışmasız ve küresel olmalı.

Yeni ad **Seenboard**. Fikri Ömer anlattı: DEHB'li biri için işin tek yerde ve gözle görülür durması gerekir, görünmeyen iş akıldan da çıkar. "Seen board" üzerindeki her şeyin görüldüğü pano demek. Ad 2026-11-14'te tarandı: crates.io, AUR ve GitHub'da aynı adlı yazılım yok, `seenboard.app` ve `seenboard.dev` kayıtsız. Web aramasında tek eşleşme saat bataryalarının bir devre kartı.

Başarı: üç repo, sunucu ve masaüstü yeni adla çalışır. Pano, plugin'ler, hatırlatıcılar ve yakalama kısayolu eskisi gibi davranır, `local.json` içindeki yerel kutularla gizli dosyaların hiçbiri kaybolmaz. Eski ad yalnız tarihli kayıtlarda kalır.

## Hedefler

- Kullanıcıya görünen ad "Seenboard". Makinenin okuduğu her tanımlayıcı `seenboard` olur: binary, crate'ler, env değişkenleri, XDG dizinleri, plugin paketleri, Android paketi, systemd birimleri.
- GitHub repoları yeniden adlandırılır, geçmiş ve etiketler korunur. Eski URL'ler GitHub yönlendirmesiyle çalışmaya devam eder.
- Sunucu ve masaüstü verisi taşınır, yeniden kurulmaz.
- Android uygulaması ilk yayınını doğrudan yeni adla yapar.

## Hedef olmayanlar

- Eski adla uyumluluk katmanı. Binary `focusdeck` dizinlerini ya da `FOCUSDECK_*` env değişkenlerini okumaz, eski isimle sembolik bağ ya da takma ad yoktur. Kullanan tek kişi Ömer, her makine bir kez elle taşınır.
- REST API'de değişiklik. Yollar (`/api/v1/...`), JSON alanları ve token aynı kalır. İstemci ile sunucu hangi sırayla güncellenirse güncellensin birbirini anlar.
- Tarihli belgeleri yeniden yazmak. `docs/plans/` ve `docs/superpowers/specs/` altındaki eski planlar, ops-journal'daki karar, olay ve runbook kayıtları olduğu gibi kalır.
- Alan adı satın almak, crates.io'ya yayımlamak.
- Dış hesap adları. GOWA'daki `focusdeck` kullanıcısı gibi başka servislerdeki hesaplar bu işin parçası değil.

## Kaçınılacaklar

- Gizli dosyaların, `local.json`'ın ya da `idempotency.json`'ın kaybolması veya izinlerinin genişlemesi (0700 dizin, 0600 dosya korunur).
- Eski plugin ile yeni sunucunun sessizce yanlış çalışması (dil, profil ya da dizin bilgisi gelmediği için).
- Taşıma sırasında masaüstünde ve sunucuda aynı anda iki sürümün aynı veriye yazması.

## Kararlar

### Protokol sürümü 2 olur

Ana uygulama plugin'e dil, profil ve dizinleri `FOCUSDECK_LANG`, `FOCUSDECK_PROFILE`, `FOCUSDECK_CONFIG_DIR`, `FOCUSDECK_DATA_DIR`, `FOCUSDECK_STATE_DIR` ile verir. Bunlar `SEENBOARD_*` olunca env adları protokolün parçası olarak değişmiş olur. `PROTOCOL_VERSION` 1'den 2'ye çıkar. Eski plugin yeni ana uygulamada eksik bilgiyle çalışmak yerine sürüm uyuşmazlığıyla reddedilir, `index.toml`'daki `protocol = 2` alanı da yanlış kurulumun önüne geçer. Çocuk süreç işareti `__FOCUSDECK_I` da `__SEENBOARD_I` olur.

### Sürümler

- Seenboard `v0.5.0`. Dizinler ve env adları değiştiği için geriye uyumlu değil.
- Plugin'ler `0.3.0` (gmail, redmine, vikunja, whatsapp, zulip), `seenboard-protocol` ve `seenboard-i18n`'e `v0.5.0` etiketiyle bağlanır.
- Android `v0.1.0`, değişmez.

### Adlandırma tablosu

| Eski | Yeni |
|---|---|
| repo `focusdeck`, `focusdeck-plugins` | `seenboard`, `seenboard-plugins` (`gh repo rename`) |
| yerel repo `focusdeck-android` (henüz GitHub'da yok) | `seenboard-android`, GitHub'da ilk kez bu adla açılır |
| crate `focusdeck`, `focusdeck-protocol`, `focusdeck-i18n` | `seenboard`, `seenboard-protocol`, `seenboard-i18n` |
| Rust modül adları `focusdeck_protocol`, `focusdeck_i18n` | `seenboard_protocol`, `seenboard_i18n` |
| plugin paketleri `focusdeck-gmail`… , `focusdeck-plugin-common`, `focusdeck-index` | `seenboard-gmail`…, `seenboard-plugin-common`, `seenboard-index` |
| `FOCUSDECK_*` | `SEENBOARD_*` |
| `~/.config/focusdeck`, `~/.local/share/focusdeck`, `~/.local/state/focusdeck` | `~/.config/seenboard`, `~/.local/share/seenboard`, `~/.local/state/seenboard` |
| `focusdeck.log` | `seenboard.log` |
| varsayılan plugin index | `https://raw.githubusercontent.com/omertahaoztop/seenboard-plugins/main/index.toml` |
| `focusdeck-remind.service/.timer` (masaüstü, kullanıcı birimi) | `seenboard-remind.service/.timer` |
| Hyprland: `~/.local/bin/focusdeck capture`, kitty `--class focusdeck` | `~/.local/bin/seenboard capture`, `--class seenboard` |
| Noctalia bar plugin'i `~/.local/share/noctalia/plugins/focusdeck` (`id = "omertahaoztop/focusdeck"`, `widget.luau` içinde `focusdeck capture` ve kitty komutu) | `~/.local/share/noctalia/plugins/seenboard`, `omertahaoztop/seenboard`, komutlar `seenboard` |
| Android `applicationId` ve Kotlin paketi `tr.omertahaoztop.focusdeck` | `tr.omertahaoztop.seenboard` |
| Android sınıfları `FocusdeckApi`, `HttpFocusdeckApi`, `FocusdeckRoot` | `SeenboardApi`, `HttpSeenboardApi`, `SeenboardRoot` |
| Android imza sertifikası `CN=focusdeck-android` | yeni anahtar, `CN=seenboard-android` |
| CT 147: hostname `focusdeck`, kullanıcı `focusdeck`, `/home/focusdeck`, `focusdeck.service`, nft tablosu `inet focusdeck` | `seenboard` (hepsi) |
| wiki `focusdeck-sunucu.md` | `seenboard-sunucu.md` |

Android imza anahtarı yeniden üretilir, çünkü uygulama henüz hiçbir cihaza kurulmadı ve anahtar ömür boyu kimlik demek. Bitwarden kaydı yeni anahtarla yapılır.

### Repo dışındaki bağımlılıklar

2026-11-14'te repolar, `~/.config`, `~/.local`, Caddy reposu ve ops-journal wiki'si tarandı. Eski ada bağlı olanlar adlandırma tablosunda. Geri kalanlar:

- **Caddy:** `vhost.d/deck.conf` adres değişikliğiyle birlikte değişir (aşağıda "Adres").
- **Yedek:** pve11'in gecelik işi CT'yi numarasıyla (147) alır, hostname değişikliğinden etkilenmez.
- **İzleme:** Alloy her LXC'ye `Monitoring/logs/install.sh` ile kurulur, `host` etiketi kurulum anındaki hostname'dir. CT 147 o koşudan sonra açıldı: Loki'nin `host` değerlerinde ve Prometheus hedeflerinde yok. Yeniden adlandırma izlemeyi kırmaz. CT 147'yi izlemeye almak bu işin dışında.
- **Gmail OAuth:** giriş `127.0.0.1` üzerinde rastgele porta dönen loopback adresiyle yapılır, adreste ad geçmez. Google Cloud'daki istemcinin görünen adı kozmetik, değişmesi gerekmez. Plugin'lerin `User-Agent` dizileri (`focusdeck-gmail/…`) kodla birlikte değişir.
- **GOWA:** Basic Auth kullanıcısı `focusdeck` ve cihaz kimliği `focusdeck` (`APP_OS`) GOWA'daki dış kayıtlar. Değiştirmek QR ile yeniden eşleme, `/etc/gowa.env` ve `Caddy/vhost.d/gowa-ui.conf`'taki base64 önekine dokunmayı gerektirir. Bu işin dışında kalır, profildeki `user = "focusdeck"` satırı olduğu gibi kalır.
- **Vikunja, Redmine, Zulip:** anahtarlar dosyadan okunur, servislerde ada bağlı bir kayıt yok.

### Hangi kopya asıl

Masaüstü 14.11'den beri sunucunun istemcisi (`[server]` bölümü). Asıl veri sunucudaki `local.json` ve plugin anahtarları. Masaüstündeki `local.json` ve yerel gizli dosyalar yalnız geri dönüş kopyası olarak durur, taşınırken yine yerinde kalır. Bu yüzden önce sunucu taşınır, sonra masaüstü. Sunucu dururken masaüstü panosu "sunucuya ulaşılamıyor" gösterir. Kesinti birkaç dakika, kişisel kullanımda kabul edilebilir.

Ofis laptopu kapalı ve sunucuya hiç bağlanmadı: 0.3.0 ile yerel kipte çalışıyor. Sunucu verisine yazamadığı için taşımayla çakışmaz. İsim değişikliği laptop beklenmeden biter, laptop ayrı bir adım olarak kalır (aşağıda 7. adım).

### Kurulu plugin'ler

Kurulu plugin'ler `<data>/plugins/<ad>` (binary) ve `<data>/plugins/<ad>.toml` (manifest) olarak durur, adları ana uygulamadan bağımsızdır (`gmail`, `redmine`…). Veri dizini taşınınca bunlar da taşınır. Protokol 2'li ana uygulama eski manifestleri sürüm uyuşmazlığıyla reddeder. Beş plugin `seenboard plugin install <ad>` ile yeniden kurulup aynı dosyaların üzerine yazılır. `focusdeck-*` adlı paket dizini ya da kayıt yok.

### Adres

Yeni adres `https://seenboard.omertahaoztop.tr` (ve `.biz.tr`). Ömer 2026-11-14'te seçti, eski adres yönlendirir.

- **DNS:** iş yok. `*.omertahaoztop.tr` Cloudflare üzerinden, `*.omertahaoztop.biz.tr` doğrudan 192.168.11.100'e çözülüyor. `seenboard.omertahaoztop.tr` bugün de çözülüyor.
- **Caddy:** `vhost.d/deck.conf` → `vhost.d/seenboard.conf`. Ana blok `seenboard.omertahaoztop.tr seenboard.omertahaoztop.biz.tr`, `import log seenboard`, ayarlar aynı (`common`, `geofilter-tr`, `192.168.11.147:8740`). Eski adlar için ayrı bir blok `redir https://seenboard.omertahaoztop.tr{uri} 308` yapar. Forgejo `Caddy` reposuna push CI ile canlıya gider (rsync, `caddy validate`, reload), yani push bir prod değişikliği.
- **Yönlendirme yalnız tarayıcı içindir.** İki istemci de yönlendirme izlemez, token başka bir hosta gitmesin diye (`remote.rs` `Policy::none()`, `HttpFocusdeckApi.kt` `followRedirects(false)`). Eski adrese giden istemci 308 alır ve hata gösterir. Bu yüzden her istemci doğrudan yeni adrese çevrilir: masaüstü `config.toml` `[server] url`, laptop 7. adımda, Android testlerindeki örnek adres (`ServerUrlTest.kt`). Telefona uygulama henüz kurulmadı, ilk kurulumda yeni adres girilir.
- Eski adların bloğu 7. adımla (laptop) birlikte kaldırılır, o noktada eski adresi kullanan istemci kalmaz.
- Cloudflare'in Türkiye dışı ülke kuralının yeni adı da kapsadığı `deck` için olduğu gibi yurt dışından denenmez. `[[cliproxy]]` sayfasındaki kural bölge geneli göründüğü için kapsadığı varsayılır (çıkarım).

### Taşıma sırası

1. **seenboard reposu:** kod adı değişir, testler geçer, main'e birleşir. `gh repo rename seenboard`, yerel `origin` güncellenir, `v0.5.0` etiketi itilir. Plugin'ler git etiketiyle bağlandığı için bu adım önce gelir.
2. **seenboard-plugins reposu:** paket adları, bağımlılıklar (`tag = "v0.5.0"`), release iş akışı ve README değişir. `gh repo rename seenboard-plugins`. Beş plugin `<ad>-v0.3.0` etiketiyle yayımlanır, release iş akışı `index.toml`'u günceller.
3. **Sunucu (CT 147) ve Caddy:** önce `pct snapshot 147 pre-seenboard`. Servis durur, `v0.5.0` `rust:1-trixie` imajında derlenip `/usr/local/bin/seenboard` olarak itilir. Kullanıcı ve grup `seenboard` olur, ev dizini `/home/seenboard`'a taşınır, XDG dizinleri yeniden adlandırılır, profil ve `config.toml`'daki `~/.config/focusdeck/` yolları `~/.config/seenboard/` olur. Plugin'ler `seenboard plugin install` ile yeniden kurulur. `seenboard.service` eski birimin aynısıdır, yalnız yollar değişir. Eski birim kaldırılır, nft tablosu ve hostname değişir. Servis başlar. Caddy reposuna `seenboard.conf` push edilir, CI yeşil olunca yeni adreste token'sız 401, token'lı 200, eski adreste 308 beklenir. Doğrulama geçince snapshot ve eski `/usr/local/bin/focusdeck` silinir.
4. **Masaüstü (archlinux-home):** `focusdeck-remind.timer` durur, `v0.5.0` `~/.local/bin/seenboard` olarak kurulur. Dizinler taşınır, yollar ve `[server] url` düzeltilir, plugin'ler yeniden kurulur, kullanıcı birimleri, Hyprland satırları ve Noctalia plugin'i değişir. Doğrulama geçince eski binary ve birimler silinir.
5. **Android:** paket ve sınıf adları, uygulama adı, imza anahtarı, README'ler. Emülatörde smoke testi yeniden koşulur. Ardından Task 12 kaldığı yerden devam eder: `seenboard-android` repo açılışı (public push öncesi Ömer'e haber verilir), secret'lar, `v0.1.0`, telefon.
6. **Wiki:** `focusdeck-sunucu.md` → `seenboard-sunucu.md` (`bm_move`, içerik güncellenir). Eski adı ve `[[focusdeck-sunucu]]` bağlantısını taşıyan sayfalar: `kisisel-repolar.md`, `kisisel-sunucular.md`, `gowa.md` (GOWA'nın kendi `focusdeck` kullanıcısı ve cihaz kimliği kalır), `musteri-iletisim.md` (laptop paragrafı), ofis laptopu runbook'u ve `NOW.md`'deki laptop maddesi. Tarihli olay ve karar kayıtlarına dokunulmaz. `check.py` temiz olmalı.
7. **Ofis laptopu (açılınca):** Plan B 8. adım yeni adla yapılır. Laptoptaki eski `~/.local/share/focusdeck/local.json` sunucudakiyle `jq` ile birleştirilir (kopya ID çıkarsa durulur), sonra `seenboard` 0.5.0, `[server]` ve token kurulur.

Her makinede taşıma öncesi dizinler tarihli bir `tar` arşivine alınır (`~/seenboard-migrate-20261114.tar`, 0600). Doğrulama geçince arşiv `trash-put` ile silinir.

### Doğrulama

- Repolarda tam test takımları yeşil, `rg -i focusdeck` yalnız tarihli belgelerde ve README'deki taşıma notunda eşleşir.
- Sunucu: `systemctl is-active seenboard nftables`, token'sız `401`, token'la `{"profiles":["pano"],...}`, dokuz kutu 200 döner, yerel kutuların öğe sayısı taşıma öncesiyle aynı.
- Masaüstü: TUI aynı panoyu gösterir, yakalama kısayolu not ekler, `seenboard-remind.timer` aktif.
- Android: emülatörde kurulum, pano, not, bitirme, sürükleme, hatırlatıcı.

## Yapanın kararına kalanlar

- Kod içi yeniden adlandırmanın nasıl yapılacağı (`git mv`, LSP rename, toplu düzenleme), commit'lere bölünmesi.
- README'lerde eski adın anılması. Önerilen: README'ye kısa bir "focusdeck'ten geçiş" bölümü (eski ad, `mv` komutları, plugin'lerin yeniden kurulması), eski URL'lerden gelen biri yolunu bulsun. Repoda CHANGELOG yok, yenisi açılmaz.
