# Seenboard

[English](README.md) · **Türkçe**

Terminalde çalışan "bende bekleyen her şey" panosu. Sözlü verilen sözler, Redmine işleri, yıldızlı mailler ve mesajlar kaynak başına bir kutuda, tek ekranda durur. Kutuları ayrı programlar olan plugin'ler doldurur; fareyle taşınır, her an hangi tuşun ne yaptığı en alt satırda yazar.

Linux x86_64 ve aarch64 hedeflenir. Arayüz İngilizce ve Türkçe gelir, yeni bir dil tek dosyadır (bkz. [Çeviri](#çeviri)).

![Ana ekran: altı kutu, gecikmiş öğeler kırmızı](docs/screenshots/tr/board.png)

| Büyütülmüş kutu | Taşıma | Tuş listesi |
|---|---|---|
| ![Büyütülmüş Ayak işleri kutusu, planlananlar yıldızlı](docs/screenshots/tr/zoom.png) | ![Hedef kutu seçimi](docs/screenshots/tr/move.png) | ![? ile açılan tuş listesi](docs/screenshots/tr/help.png) |

Görüntülerdeki veriler uydurmadır.

## Kurulum

```sh
cargo install --git https://github.com/omertahaoztop/seenboard --tag v0.5.0 --locked seenboard
```

Hazır plugin'ler [seenboard-plugins](https://github.com/omertahaoztop/seenboard-plugins) reposunda. Dizinden kurulur:

```sh
seenboard plugin install vikunja
seenboard plugin list
seenboard plugin remove vikunja
```

`seenboard plugin list` çıktısı:

```
vikunja  0.2.0  (dizinde: 0.2.0)
local  (yerleşik)
```

`local` Seenboard'un içinde gelir ve listenin sonunda hep yer alır. `plugin install local` ile `plugin remove local` reddedilir.

Plugin'ler `~/.local/share/seenboard/plugins/<ad>` altına yazılır. `install` dizinden indirdiği dosyanın sha256'sını doğrular, tutmazsa dosyayı siler. Yarıda kalan bir yeniden kurulum önceki sürümü yerinde bırakır. `remove` plugin'i hâlâ kullanan bir profil varsa uyarır.

> Plugin'ler kullanıcının yetkisiyle çalışır, kum havuzu yoktur. Bir plugin'i kurmadan önce kaynağına güvendiğinden emin ol.

## Hızlı başlangıç

`cargo install` sonrası `seenboard` çalıştır. Profil seçilmemişse (`--profile`, `SEENBOARD_PROFILE`, `default_profile`) ve `~/.config/seenboard/profiles/` boşsa Seenboard `profiles/notes.toml` dosyasını tek kutuyla ("Notlar" başlıklı, arayüz dilinde) oluşturup açar. `a` tuşuna basıp bir satır yazmak not almaya yeter. `config.toml` yazılmaz.

`profiles/` içinde tek profil varsa Seenboard sormadan onu açar. Birden çok profil var ve seçim yapılmamışsa hata verip çıkar. [Profil](#profil) bölümündeki gibi birini seç.

## Ayarlar

`~/.config/seenboard/config.toml`, hepsi isteğe bağlı:

```toml
language = "tr"
default_profile = "ev"
registries = ["https://raw.githubusercontent.com/omertahaoztop/seenboard-plugins/main/index.toml"]
capture_command = ["rofi", "-dmenu", "-p", "Not"]

[capture_words]
tomorrow = ["yrn"]
minute = ["dak"]
```

- `language`: arayüz dili, `en` ya da `tr` gibi bir kod (bkz. [Çeviri](#çeviri)). Yoksa çekirdek sırayla `LC_ALL`, `LC_MESSAGES` ve `LANG` değişkenlerine bakar, hiçbiri yoksa İngilizceye düşer. Cron, systemd timer ya da pencere yöneticisi kısayolu `LANG` taşımayabilir; `capture` ya da `remind` bunlardan çalışıyorsa `language` yaz.
- `capture_words`: `capture` komutunun anladığı kelimelere ek yazımlar; etkin dilin kendi kelimelerine eklenir. Her anahtar bir liste alır; anahtarlar `tomorrow`, `minute`, `hour`, `mon`, `tue`, `wed`, `thu`, `fri`, `sat`, `sun`, bilinmeyen anahtar hatadır.
- `config.toml` içindeki bilinmeyen her anahtar hatadır, yazım yanlışı sessizce geçmez.
- `default_profile`: `--profile` ve `SEENBOARD_PROFILE` verilmediğinde kullanılan profil.
- `registries`: plugin dizini adresleri, sırayla. Aynı ad iki dizinde varsa öndeki kazanır. Kendi dizinini ya da fork'unu buraya ekleyebilirsin.
- `capture_command`: `capture` metinsiz çağrılınca tek satır sormak için çalıştırılan komut.
- `[serve]`: `seenboard server` için `listen` ve `token_file` (bkz. [Sunucu](#sunucu)).
- `[server]`: yerel profiller yerine sunucuyu kullanmak için `url` ve `token_file` (bkz. [Sunucu](#sunucu)).

## Profil

`~/.config/seenboard/profiles/<ad>.toml`. Profil seçimi sırasıyla `--profile`, `SEENBOARD_PROFILE`, `default_profile`.

```toml
name = "ev"
capture_into = "sozlu"
capture_refs = { R = "https://redmine.example.com/issues/{}" }
capture_label_aliases = { tel = "telefon", yuz = "yuz-yuze", "yüz" = "yuz-yuze" }

[[box]]
id = "sozlu"
title = "Sözlü işler"
plugin = "vikunja"
row = 0
col = 0
refresh = "1m"
config = { url = "https://vikunja.example.com", token_file = "~/.config/seenboard/secrets/vikunja", project = "Ev" }

[[box]]
id = "redmine"
title = "Redmine planım"
plugin = "redmine"
row = 0
col = 1
plan_into = "sozlu"
config = { url = "https://redmine.example.com", key_file = "~/.config/seenboard/secrets/redmine" }
```

- `row` ve `col` ızgara konumudur. İsteğe bağlı `width` ve `height` kutuyu birden çok hücreye yayar.
- `config` tablosu olduğu gibi plugin'e gider. Plugin'lerin ayar anahtarları kendi README'lerinde yazar.
- Şifreler profilde durmaz, yalnız dosya yolu (`token_file`, `key_file`) yazılır. Adı `_file` ile biten her ayarın başındaki `~` açılır. Dosya grup ya da başkaları tarafından okunabiliyorsa kutu açılmaz (`chmod 600`).
- `plan_into`: kutudaki öğeler boşluk tuşuyla hedef kutuya "planıma al" olarak taşınır. Kaynak plugin öğeyi artık listelemiyorsa (iş kapandı) bağlı hedef öğe de bitmiş sayılır. Kaynak `list` hata verdiğinde hiçbir görev kapanmaz.
- `capture_into`, `capture_refs` ve `capture_label_aliases` aşağıdaki `capture` komutunun ayarlarıdır.

### Yerel kutu

`plugin = "local"` olan kutu yerleşiktir: kurulacak bir şey yok, `config` de yazılmayabilir.

```toml
[[box]]
id = "fikirler"
title = "Fikirler"
plugin = "local"
row = 1
col = 0
config = { list = "fikirler" }
```

- `list` not listesinin adıdır; yazılmazsa kutunun `id` değeri kullanılır. Aynı `list` adını kullanan kutular, hangi profilde olurlarsa olsunlar, aynı notları görür.
- `x` notu bitti sayar. Notlar taşınabilir (`m` ya da sürükleme) ve sıralanabilir, kutu başka kutulardan bırakılanları da kabul eder: Zulip mesajı, mail ya da Redmine işi not olur.
- Son tarihi olan not, saati geldiğinde `seenboard remind` ile hatırlatma olarak gösterilir.
- Yerel kutu bir Redmine kutusunun `plan_into` hedefi olabilir.

Notlar `~/.local/share/seenboard/local.json` dosyasında durur, `SEENBOARD_DATA_DIR` tanımlıysa `$SEENBOARD_DATA_DIR/local.json` dosyasında. Yedek için bu dosyayı kopyalamak yeter. Bitmiş notlar dosyada kalır ama kutuda görünmez.

## Ekran ve tuşlar

Ana ekranda profildeki kutular ızgarada durur. Kutu başlığı öğe sayısını ve gecikmiş sayısını yazar, gecikmiş öğe `!` ile başlar ve kırmızıdır. En alt satır o anki ekranın tuşlarını gösterir, `?` tam listeyi açar. Çift tık ya da `Enter` kutuyu büyütür, `Esc` geri döner.

| Tuş | Ana ekran | Büyütülmüş kutu |
|---|---|---|
| `a` | not al (`capture_into` kutusuna) | aynı |
| `x` | seçili öğe bitti | aynı |
| `u` | seçili öğenin yıldızını kaldır (Zulip) | aynı |
| `m` | taşı: hedef kutu listesi açılır | aynı |
| `boşluk` | yok | `plan_into` olan kutuda planıma al; zaten planda olan öğede ilk basış sorar, ikincisi öğeyi plandan siler |
| `o` | linki tarayıcıda aç (yalnız `https`, `http` ve `mailto`) | aynı |
| `/` | yok | ara |
| `Tab`, `Shift+Tab` | sonraki / önceki kutu | yok |
| `←` `→` | soldaki / sağdaki kutu | yok |
| `↑` `↓` | listede gezin, en üstte / en altta üstteki / alttaki kutuya geç | listede gezin |
| `PageUp` `PageDown` `Home` `End` | yok | sayfa sayfa gezin, ilk / son öğeye git |
| `Shift+↑` `Shift+↓` | yok | seçili öğeyi yukarı / aşağı taşı (plugin sıralamayı destekliyorsa) |
| `?`, `q`, `Esc` | yardım, çık, geri | yardım, çık, geri |

Not ya da arama alanına yapıştırılan metin tek satır olarak gelir, satır sonları boşluğa döner. Başka bir ekranda yapıştırılan metin yok sayılır, yani tuş olarak çalışmaz. Bunun için terminalin bracketed paste desteklemesi gerekir, güncel terminaller destekler.

`NO_COLOR` boş olmayan bir değerle tanımlıysa Seenboard renk kullanmaz: seçim ters video ile gösterilir, gecikmiş öğeler `!` işaretini korur.

Fare:

- Tekerlek kutunun içinde kaydırır.
- Tıklama öğeyi seçer, çift tıklama kutuyu büyütür.
- Öğeyi başka kutuya sürükleyip bırakmak hedef plugin'e `drop` gönderir. Örneğin Zulip ya da mail öğesini Vikunja kutusuna bırakınca görev olur.
- Aynı kutu içinde sürüklemek sırayı değiştirir. En üstteki en önceliklidir.

## Dışarıdan not alma

```sh
seenboard capture "R12345 arandı #tel yarın 14:00 // ayrıntı"
seenboard capture
```

Metin verilmezse `capture_command` tek satır sorar. Not profildeki `capture_into` kutusuna gider; başlık boşsa ya da plugin'e ulaşılamıyorsa komut 1 ile çıkar. Ulaşılamayan not `~/.local/state/seenboard/capture-queue.jsonl` dosyasında bekler ve sonraki başarılı yakalamada gönderilir. Not gönderim başlamadan önce bu dosyaya yazıldığından öldürülen ya da kesilen bir `capture` notu kaybetmez. Teslim en az bir kezdir: öldürülen bir çalıştırmadan sonra not plugin'e iki kez ulaşabilir. Bir pencere yöneticisi kısayoluna (örneğin Hyprland'de `F9`) bağlamak için `seenboard capture` yeterlidir.

Yazım kuralları:

- `//` sonrası açıklamadır.
- Başlığın son kelimesi `30dk` (dakika) ya da `2s` (saat) gibi birimli bir sayıysa şimdiden o kadar sonra, `SS:DD` ise bugün o saat (geçmişse yarın) son tarih ve hatırlatma olur. Önüne "yarın" kelimesi ya da bir gün kısaltması konabilir (kelimeler aşağıda).
- İlk `@ad` kimden bilgisidir. `#kelime` etikettir ve `capture_label_aliases` ile çevrilir. Yalnız rakamdan oluşan `#123` düz metin kalır.
- `capture_refs` önekiyle başlayıp en az iki rakamla süren kelime (`R12345`) linke çevrilir.
- Geri kalan kelimeler başlıktır.

Kelimeler arayüz diline bağlıdır, büyük küçük harf ayırmaz ve `capture_words` üstüne ekler:

| Dil | yarın | dakika | saat | Pazartesi'den Pazar'a |
|---|---|---|---|---|
| `en` | `tomorrow` | `min`, `m` | `h` | `mon`/`monday`, `tue`/`tuesday`, `wed`/`wednesday`, `thu`/`thursday`, `fri`/`friday`, `sat`/`saturday`, `sun`/`sunday` |
| `tr` | `yarın`, `yarin` | `dk` | `s` | `pzt`/`pts`, `sal`, `çar`/`car`, `per`/`prş`/`prs`, `cum`, `cmt`, `paz` |

Örneğin Türkçe `seenboard capture "R12345 arandı #tel yarın 14:00"`, İngilizce `seenboard capture "R12345 called #tel tomorrow 14:00"`.

## Hatırlatma

```sh
seenboard remind
seenboard remind --send
```

Hatırlatma destekleyen plugin'lerden son 12 saatin hatırlatmalarını ister. `remind` saati gelenleri yazdırır; `--send` bunları `notify-send -u critical` ile bildirim olarak gösterir. Gösterilenler `~/.local/state/seenboard/reminded.json` dosyasına yazılır, aynı hatırlatma bir daha gösterilmez. Dakikada bir çalışan bir systemd timer ya da cron girdisi `seenboard remind --send` çalıştırabilir.

## Sunucu

`seenboard server` kutuları tek makinede çalıştırır, öteki makineler ve uygulamalar bunlara HTTPS ile erişir. Profiller, plugin'ler, şifreleri ve `local.json` sunucuda durur. İstemci bunların hiçbirine sahip olmadan aynı kutuları gösterir.

Sunucu tarafında `config.toml`:

```toml
[serve]
listen = "127.0.0.1:8740"
token_file = "~/.config/seenboard/secrets/server-token"
```

- `listen`: dinlenecek adres. Varsayılan `127.0.0.1:8740` olduğundan önüne bir vekil sunucu koymadıkça ya da başka bir adres seçmedikçe sunucuya dışarıdan erişilmez.
- `token_file`: taşıyıcı token'ı tutan dosya. En az 32 karakter olmalı, yalnız görünür ASCII karakterler (boşluksuz) içermeli ve yalnız sahibi okuyabilmeli (0600). Profildeki `*_file` anahtarlarıyla aynı kural geçerlidir.

Token'ı bir kez üret, aynı dosyayı her istemciye kopyala:

```sh
install -D -m 600 /dev/null ~/.config/seenboard/secrets/server-token
openssl rand -hex 32 > ~/.config/seenboard/secrets/server-token
```

Sonra `seenboard server` çalıştır. Sunucu TLS konuşmaz, bunu Caddy gibi bir ters vekil üstlenir:

```
seenboard.example.com {
    reverse_proxy 127.0.0.1:8740
}
```

İstemci tarafında `config.toml`:

```toml
[server]
url = "https://seenboard.example.com"
token_file = "~/.config/seenboard/secrets/server-token"
```

- `url` `https://` olmalı. Düz `http://` yalnız deneme için `127.0.0.1`, `[::1]` ve `localhost` adreslerinde kabul edilir.
- `token_file` sunucudaki kurallara uyar.
- Her istek 20 saniyede zaman aşımına uğrar.

`[server]` bölümü varsa TUI, `capture` ve `remind` sunucuyla konuşur. Profil her zamanki gibi seçilir (`--profile`, `SEENBOARD_PROFILE`, `default_profile`), hiçbiri verilmemişse istemci sunucunun varsayılan profilini ya da tek profilini alır. Sunucudan profil (kutular, yerleşim, `capture_into`, `capture_refs`, `capture_label_aliases`, `capture_words`), her kutunun öğeleri ve her eylemin, bırakmanın ve yakalamanın sonucu gelir. Yerel `[capture_words]` istemci kipinde yok sayılır. `plan_into` bağlantısı yalnız sunucuda çalışır ve bağlı görevleri orada kapatır, böylece iki istemci aynı görevi iki kez bitirmez. Hatırlatmalar sunucudan alınır, hangilerinin gösterildiği ise makine başına kendi `reminded.json` dosyasında tutulur. `[server]` varken yerel `profiles/`, kurulu plugin'ler ve `local.json` kullanılmaz.

`capture` notu önce kendi kuyruğuna yazar. Sunucuya ulaşılamazsa not orada bekleyip sonraki başarılı yakalamada gider. Not kuyruktan yalnız sunucu `INVALID` cevabı verirse kalıcı olarak çıkar. Seçili profilde `capture_into` yoksa not kuyrukta kalır, nedeni de ekrana bildirilir.

`[server]` bölümü yoksa Seenboard eskisi gibi yerelde çalışır.

API bir kutunun `config` tablosunu ve gizli dosyalarının yollarını hiç döndürmez. Plugin hata metni değiştirilmeden iletilir. Token da log'a ya da hata metnine yazılmaz.

### API

Her istek `Authorization: Bearer <token>` başlığını taşır. Gövdeler ve cevaplar snake_case alanlı JSON'dır, istek gövdesi en çok 1 MiB olabilir.

| Yöntem | Yol | Ne yapar |
|---|---|---|
| `GET` | `/api/v1/profiles` | profil adları ve varsayılan profil |
| `GET` | `/api/v1/profiles/{p}` | profil görünümü: yakalama ayarları ve kutular, her birinin `describe` sonucuyla |
| `GET` | `/api/v1/profiles/{p}/boxes/{b}/items` | kutunun öğeleri ve listelendikleri an |
| `POST` | `/api/v1/profiles/{p}/boxes/{b}/actions` | `item`, `action`, `args`: `done`, `delete`, `move`, `reorder`, `unstar` |
| `POST` | `/api/v1/profiles/{p}/boxes/{b}/drops` | `item`, `from`: başka kutudan öğe bırakır |
| `POST` | `/api/v1/profiles/{p}/captures` | `text`, `at`, `lang`: sunucu notu çözümler ve `capture_into` kutusuna koyar |
| `GET` | `/api/v1/profiles/{p}/reminders?since=<RFC 3339>` | o andan beri bütün kutuların hatırlatmaları |

`drops` ve `captures` `Idempotency-Key` başlığını alır. Aynı gövdeyle aynı hedefe tekrarlanan anahtar işi ikinci kez yapmaz, ilk cevabı döndürür. Aynı anahtar başka bir kutuya ya da profile giderse `KEY_REUSED` alır. Anahtarını almış bir istek, istemci bağlantıyı kapatsa da sonuna kadar çalışır. Yeniden deneyen istemciye kaydedilen cevap döner. 90 günden eski anahtarlar sunucu başlarken ve her yeni anahtar kaydedilirken silinir.

Hata aşağıdaki durum koduyla ve `{"error": {"code": "...", "message": "..."}}` gövdesiyle gelir:

| Kod | Durum | Anlamı |
|---|---|---|
| `UNAUTHORIZED` | 401 | token yok ya da yanlış |
| `NOT_FOUND` | 404 | bilinmeyen profil ya da kutu, yanlış yol ya da yöntem de bu kodu alır |
| `INVALID` | 422 | gövde, sorgu ya da `Idempotency-Key` bozuk ya da not yerleştirilemiyor |
| `UNSUPPORTED` | 422 | kutunun plugin'i bu eylemi desteklemiyor |
| `PLUGIN` | 502 | plugin hata verdi ya da cevap vermedi |
| `IN_PROGRESS` | 409 | aynı `Idempotency-Key` ile gelen istek hâlâ çalışıyor |
| `KEY_REUSED` | 422 | `Idempotency-Key` başka bir gövdeyle ya da başka bir hedefe kullanılmış |
| `INTERNAL` | 500 | sunucu kendi durumunu kaydedemedi |

## Plugin protokolü

Plugin, çekirdeğin her kutu için bir kez başlatıp açık tuttuğu bir programdır. Satır başına bir JSON mesaj: stdin'den istek, stdout'tan cevap. stderr çekirdeğin log dosyasına yazılır. Protokol sürümü 1.

```json
{"id": 7, "method": "list", "params": {"view": "full"}}
{"id": 7, "result": {"items": []}}
{"id": 8, "error": {"message": "401 Unauthorized"}}
```

| Yöntem | Parametre | Cevap |
|---|---|---|
| `describe` | yok | `name`, `version`, `protocol`, `actions`, `accepts_drop`, `reminders` |
| `configure` | profildeki `config` tablosu | boş, hatalı ayarda `error` |
| `list` | `view`: `summary` ya da `full` | `items` |
| `action` | `item`, `action` (`done`, `delete`, `move`, `reorder`, `unstar`), `args` | güncellenmiş öğe ya da boş |
| `drop` | `item`, `from` | oluşturulan ya da taşınan öğe |
| `reminders` | `since` (RFC 3339) | `at` ve `item` çiftleri |

- İstek başına zaman aşımı 15 saniyedir. Zaman aşımı, JSON olmayan satır, 16 MiB'tan uzun satır ya da süreç çökmesi kutuda kırmızı bir satır olarak görünür; çekirdek plugin'i bir sonraki yenilemede yeniden başlatır, öteki kutular etkilenmez.
- `describe` cevabındaki `protocol` çekirdeğin bildiği sürümden farklıysa kutu açılmaz.
- Çekirdek yalnız `*_file` yolunu ve izinlerini denetler; şifreyi plugin dosyadan kendisi okur, değer çekirdekten geçmez.
- Çekirdek her plugin'i istenen arayüz dilini `SEENBOARD_LANG` ortam değişkeninde (örneğin `tr`) vererek başlatır, dil yoksa değişkeni kaldırır. Plugin'in o dilde çevirisi yoksa İngilizce kullanılır.
- Plugin herhangi bir dille yazılabilir, tek şart protokole uymak.

Rust ile plugin yazanlar için tipler ve `Plugin` trait'iyle satır döngüsünü yürüten `serve` yardımcısı `seenboard-protocol` crate'inde durur:

```toml
[dependencies]
seenboard-protocol = { git = "https://github.com/omertahaoztop/seenboard", tag = "v0.5.0" }
```

`Plugin` trait'ini uygulayıp `seenboard_protocol::serve(plugin, stdin.lock(), stdout)` çağırmak yeter.

## Güvenlik ve gizlilik

- Plugin dizinleri ve plugin dosyaları HTTPS üzerinden indirilir. Düz `http://` reddedilir, yönlendirme hedefi olarak da. İstisna, dizini yerelde denemek için `127.0.0.1`, `[::1]` ve `localhost` adresleridir. Ağdan indirilen bir dizin plugin'i yerel bir dosyaya ya da bu adreslere yönlendiremez.
- `plugin install` binary'nin sha256'sını yanındaki manifest dosyasına yazar. Bir kutu o plugin'i başlatmadan önce Seenboard binary'yi manifest ile karşılaştırır, değişmiş bir binary'yi başlatmayıp hata gösterir. Elle kopyalanan plugin'in manifest'i olmadığından denetlenmez.
- Log, yakalama kuyruğu, hatırlatma kaydı ve `local.json` yalnız sahibinin okuyup yazabileceği şekilde (0600) oluşturulur.
- `~/.local/state/seenboard/seenboard.log` 1 MiB'ı geçince `seenboard.log.1` adına taşınır, yani en çok 2 MiB kadar yer tutar.

## Çeviri

Bir dil tek dosyadır: [Fluent](https://projectfluent.org/) söz diziminde `crates/seenboard/locales/<kod>.ftl` ekle (örneğin `de.ftl`) ve yeniden derle; dosya adı, `language` ve `LANG` değerinin eşleştiği koddur.

- Temel `en.ftl` dosyasıdır. Kopyala, değerleri çevir; mesaj kimlikleri ve `{ $değişken }` adları olduğu gibi kalır.
- `cargo test`, eksik ya da fazla kimlikleri ve `en.ftl`'den farklı değişkenleri söyleyerek düşer; yarım bir çeviri içeri giremez.
- `capture-words-*` girdileri cümle değil, boşlukla ayrılmış yazım listesidir: `capture-words-tomorrow`, `capture-words-minute`, `capture-words-hour` ve `capture-words-mon` ile `capture-words-sun` arası. İnsanların gerçekten yazdığı biçimleri koy; klavye düzeni yüzünden aksansız yazım olası ise onu da ekle.
- Bir plugin'in yazdığı metinler burada değil, [seenboard-plugins](https://github.com/omertahaoztop/seenboard-plugins) reposunda çevrilir.

## Geliştirme

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Lisans

MIT, bkz. [LICENSE](LICENSE).

## focusdeck'ten geçiş

Seenboard, 0.5.0 öncesinde focusdeck adını taşıyordu. Eski ad altında hiçbir şey okunmaz: `focusdeck` dizini de `FOCUSDECK_*` değişkenleri de yok sayılır. Protokol 2, protokol 1'in yerini aldığından plugin'ler yeniden kurulmalıdır.

```sh
mv ~/.config/focusdeck ~/.config/seenboard
mv ~/.local/share/focusdeck ~/.local/share/seenboard
mv ~/.local/state/focusdeck ~/.local/state/seenboard
```

Sonra profillerdeki ve `config.toml` içindeki yolları düzelt: `~/.config/focusdeck/secrets/` altını gösteren `token_file` ve `key_file` girdileri artık `~/.config/seenboard/secrets/` altını gösterir. Kabuğundaki, timer'daki ve pencere yöneticisi ayarındaki `FOCUSDECK_*` değişkenlerini `SEENBOARD_*` yap. Her plugin'i `seenboard plugin install <ad>` ile yeniden kur.
