# focusdeck tasarımı

Tarih: 2026-11-12 · Durum: kullanıcı onayı bekliyor

## Amaç

ADHD'li biri için "bende bekleyen her şey" ekranı. Sözlü verilen sözler, planlanan Redmine işleri,
yıldızlı mailler, yıldızlı Zulip mesajları, kendine iletilen WhatsApp mesajları ve fikirler tek
terminal ekranında, kaynak başına bir kutuda durur. Uzun listelerde kaybolmadan gezilir, fareyle
taşınır, her an hangi tuşun ne yaptığı en alt satırda yazar.

Bugünkü `pano` (Python betiği + wtfutil) yalnızca gösterim yapıyor, listede aşağı inilemiyor.
focusdeck onun yerini alır.

## Kapsam

İlk sürümde olanlar:

- Rust ile yazılmış TUI çekirdeği, iki profil (iş, ev), profil başına kutu yerleşimi.
- Ayrı program olarak çalışan plugin'ler ve monokit'teki gibi `plugin install/list/remove`.
- Ayrı plugin reposu ve fork'lanabilir plugin dizini.
- Hazır plugin'ler: `vikunja`, `redmine`, `gmail`, `zulip`, `whatsapp` (GOWA).
- `focusdeck capture` (dışarıdan not alma, F9) ve `focusdeck remind` (bildirim zamanlayıcısı).
- GOWA kurulumu (pve11 üzerinde LXC).

Kapsam dışı:

- GOWA'ya yama. WhatsApp yıldızları okunmaz, yalnız "kendime mesaj" sohbeti okunur.
- WhatsApp'a ya da Gmail'e yazma. Gmail `gmail.readonly` kapsamında kalır.
- Mobil arayüz. Telefonda görmek gereken her şey zaten Vikunja'da durur.
- Ev profilinin kutuları. Profil desteği ilk sürümde var, ev kutuları ayrıca konuşulur.
- Windows ve macOS paketleri. Linux x86_64 ve aarch64.

## Repolar

| Repo | İçerik |
|---|---|
| `focusdeck` | Çekirdek: TUI, profil okuma, plugin süreç yönetimi, `capture`, `remind`, `plugin` alt komutları |
| `focusdeck-plugins` | Hazır plugin'lerin Rust kaynağı, `index.toml` dizin dosyası, sürüm iş akışları |

İkisi de GitHub'da herkese açık. Kişiye özel bilgi (adresler, hesaplar) kodda yer almaz, yalnız
kullanıcının profil dosyasında durur. Ortak tipler ve Rust plugin yazarları için `serve` yardımcısı
çekirdek reposundaki `focusdeck-protocol` crate'inde durur. Plugin reposu onu sürüm etiketiyle
bağımlılık olarak alır; repolar GitHub'a gönderilene kadar yerel yol bağımlılığı kullanılır.

Kullanılan crate'ler: `ratatui` + `crossterm` (ekran, fare olayları), `tokio` (plugin süreçleri,
zamanlayıcılar), `serde` + `serde_json` + `toml`, `reqwest` (rustls), `sha2`.

## Plugin protokolü (sürüm 1)

Çekirdek her kutu için plugin programını bir kez başlatır ve açık tutar. Satır başına bir JSON
mesaj, stdin'den istek, stdout'tan cevap. stderr çekirdeğin log dosyasına yazılır.

```json
{"id": 7, "method": "list", "params": {"view": "full"}}
{"id": 7, "result": {"items": []}}
{"id": 8, "error": {"message": "401 Unauthorized"}}
```

| Yöntem | Parametre | Cevap |
|---|---|---|
| `describe` | yok | `name`, `version`, `protocol: 1`, `actions` (desteklenen eylemler), `accepts_drop`, `reminders` |
| `configure` | profildeki `config` tablosu, JSON nesnesi olarak | boş; hatalı ayar `error` döner |
| `list` | `view`: `summary` ya da `full` | `items` |
| `action` | `item` (öğe kimliği), `action` (`done`, `delete`, `move`, `reorder`, `unstar`), `args` | güncellenmiş öğe ya da boş |
| `drop` | `item` (bırakılan öğe), `from` (öğenin geldiği plugin ve kimlik, dışarıdan notta yok) | oluşturulan ya da taşınan öğe |
| `reminders` | `since` (RFC 3339) | `at` ve `item` çiftleri |

Öğe şeması:

```json
{
  "id": "12345",
  "title": "DEPO için S3",
  "subtitle": "Ö1 · 4g gecikmiş",
  "url": "https://redmine.example.com/issues/12345",
  "due": "2026-11-08T17:00:00+03:00",
  "overdue": true,
  "source": {"plugin": "redmine", "id": "12345"},
  "description": null,
  "labels": [],
  "from": null,
  "links": []
}
```

- `source` öğenin asıl kaynağıdır ve yalnız başka kutudan gelmiş öğelerde bulunur. Vikunja plugin'i
  bunu görev açıklamasına `focusdeck-source: redmine:12345` satırı olarak yazar ve geri okur.
- `labels`, `from`, `description`, `links` dışarıdan not alırken dolar. Plugin bunları kendi
  karşılığına çevirir (Vikunja'da etiket, `kimden:<ad>` etiketi, açıklama, link paragrafı).
- `drop` içindeki `from` hedef plugin ile aynıysa (Vikunja görevi başka Vikunja kutusuna) plugin
  yeni görev açmaz, mevcut görevi taşır.
- `move` ve `reorder` eylemlerinde `args`: `{"position": 0}` listede yeni sıra, 0 en üst.
- `open` eylemi yoktur. Linki çekirdek `xdg-open` ile açar.
- Profilde adı `_file` ile biten her ayarın `~`'i çekirdekte açılır. Çekirdek `configure`'dan önce
  dosyanın izinlerine bakar, grup ya da başkaları okuyabiliyorsa kutuyu açmaz. Şifreyi plugin okur,
  değer çekirdekten geçmez.
- Her istek için zaman aşımı 15 saniye. Zaman aşımı, JSON olmayan satır ya da süreç çökmesi kutuda
  kırmızı bir satır olarak görünür, çekirdek plugin'i bir sonraki yenilemede yeniden başlatır.
  Öteki kutular etkilenmez.
- `describe` cevabındaki `protocol` çekirdeğin bildiği sürümden farklıysa kutu açılmaz ve hata
  satırında iki sürüm yazar.

## Plugin kurulumu ve dizin

`index.toml` örneği:

```toml
[[plugin]]
name = "redmine"
description = "Bana atanmış Redmine işleri"
version = "0.1.0"
protocol = 1

[plugin.targets.x86_64-unknown-linux-gnu]
url = "https://github.com/omertahaoztop/focusdeck-plugins/releases/download/redmine-v0.1.0/redmine-x86_64-unknown-linux-gnu"
sha256 = "64 haneli onaltılık sağlama"
```

- Çekirdeğin ayarı (`~/.config/focusdeck/config.toml`) dizin adreslerini sırayla listeler:
  `registries = ["https://raw.githubusercontent.com/omertahaoztop/focusdeck-plugins/main/index.toml"]`.
  Fork'layan kişi kendi adresini ekler. Aynı ad iki dizinde varsa listede önce gelen kazanır.
- `plugin install <ad>`: dizini indirir, makinenin hedefini seçer, dosyayı indirir, sha256'yı
  doğrular, `~/.local/share/focusdeck/plugins/<ad>` olarak çalıştırılabilir yazar. Sağlama tutmazsa
  dosya silinir ve komut 1 ile çıkar.
- `plugin list`: kurulu plugin'ler, sürümleri, dizindeki son sürüm.
- `plugin remove <ad>`: dosyayı siler. Profilde hâlâ kullanılıyorsa uyarır.
- Plugin istenen dilde yazılabilir. Dizine Rust dışı plugin de girer, tek şart protokole uyması.
- Plugin'ler kullanıcının yetkisiyle çalışır, kum havuzu yok. README bunu açıkça yazar.

## Profiller

- Yer: `~/.config/focusdeck/profiles/<ad>.toml`. Seçim sırası: `--profile`, `FOCUSDECK_PROFILE`,
  `config.toml` içindeki `default_profile`.
- Şifreler profilde yazmaz, yalnız dosya yolu yazar (`token_file`, `key_file`). Varsayılan klasör
  `~/.config/focusdeck/secrets/`, dosyalar 0600. Grup ya da başkaları okuyabiliyorsa kutu açılmaz.
- Dışarıdan not ayarları profilde: `capture_into` (hedef kutu), `capture_refs` (önek → link
  kalıbı), `capture_label_aliases` (kısa etiket → asıl etiket).

```toml
name = "iş"
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
config = { url = "https://vikunja.example.com", token_file = "~/.config/focusdeck/secrets/vikunja", project = "Home" }

[[box]]
id = "redmine"
title = "Redmine planım"
plugin = "redmine"
row = 0
col = 1
plan_into = "sozlu"
config = { url = "https://redmine.example.com", key_file = "~/.config/focusdeck/secrets/redmine" }
```

`row`/`col` ızgara konumudur. İsteğe bağlı `width`/`height` kutuyu birden çok hücreye yayar.

## Ekran ve etkileşim

- Ana ekran: profildeki kutular ızgarada. Kutu başlığı öğe sayısını ve gecikmiş sayısını yazar.
  Gecikmiş öğe kırmızıdır. Üst satır profil adı, saat ve son yenilemeyi gösterir.
- En alt satır ipucu satırıdır, o an odaktaki ekranın tuşlarını gösterir. `?` tam listeyi açar.
- Kutunun içinde fare tekerleği kaydırır, liste kesilmez.
- Çift tık ya da `Enter` kutuyu büyütür (`full` görünüm). `Esc` geri döner.

Tuşlar, hepsi tek tuş:

| Tuş | Ana ekran | Büyütülmüş kutu |
|---|---|---|
| `a` | not al (`capture_into` kutusuna) | aynı |
| `x` | seçili öğe bitti | aynı |
| `u` | seçili öğenin yıldızını kaldır (Zulip) | aynı |
| `m` | taşı: hedef kutu listesi açılır | aynı |
| `boşluk` | yok | `plan_into` olan kutuda planıma al / çıkar |
| `o` | linki tarayıcıda aç | aynı |
| `/` | yok | ara |
| `Tab` | sonraki kutu | yok |
| `?`, `q`, `Esc` | yardım, çık, geri | yardım, çık, geri |

Fare:

- Tıklama öğeyi seçer, çift tıklama kutuyu büyütür.
- Öğeyi başka kutuya sürükleyip bırakmak hedef plugin'e `drop` gönderir. Zulip, mail ya da WhatsApp
  öğesi Sözlü işler'e bırakılınca Vikunja görevi olur. Fikirler'e bırakılınca fikir olur.
- Aynı kutunun içinde sürüklemek sırayı değiştirir (`action: reorder`). En üstteki en önceliklidir.
- Sürükleme sırasında öğe kesik çizgiyle çerçevelenir, bırakılabilecek kutular vurgulanır.

## Kutular arası bağ

- `plan_into` olan kutunun `summary` görünümü yalnız hedef kutuda `source` olarak bağlı öğeleri
  gösterir. Büyütülmüş görünüm hepsini gösterir, planlananlar ★ ile işaretlidir.
- Boşluk tuşu öğeyi hedef kutuya `drop` eder, ikinci basış bağlı görevi `action: delete` ile siler.
- Kaynak plugin bir öğeyi artık listelemiyorsa (Redmine işi kapandı) çekirdek ona bağlı hedef
  öğeye `action: done` gönderir. Bu kontrol yalnız kaynak `list` başarıyla döndüğünde yapılır,
  hata durumunda hiçbir görev kapanmaz.

## Hazır plugin'ler

| Plugin | Listeler | Eylemler | Drop | Hatırlatma |
|---|---|---|---|---|
| `vikunja` | Projedeki açık görevler (isteğe bağlı sütun süzgeci) | `done`, `delete`, `move`, `reorder` | Evet: görev açar ya da Vikunja görevini taşır | Evet: hatırlatma saati gelenler |
| `redmine` | Bana atanmış açık işler, öncelik ve gecikme | yok | Hayır | Hayır |
| `gmail` | `is:starred` mailler, 3+ gün kırmızı | yok | Hayır | Hayır |
| `zulip` | Yıldızlı mesajlar | `unstar` | Hayır | Hayır |
| `whatsapp` | GOWA'da kendime sohbetinin mesajları | `done` (yalnız yerel gizleme) | Hayır | Hayır |

- Fikirler kutusu `vikunja` plugin'inin başka bir projeyle çalışan ikinci örneğidir ("Home Ideas").
- `gmail` ilk girişi `focusdeck-gmail login` alt komutuyla yapar. Bugünkü OAuth istemcisi ve token
  dosyası taşınır, yeniden giriş gerekmez.
- `whatsapp` GOWA'ya yalnız `GET /chat/<jid>/messages` ile bağlanır. Gizlenen mesaj kimlikleri
  `~/.local/state/focusdeck/whatsapp-hidden.json` içinde durur. WhatsApp'a hiçbir istek yazmaz.

## Dışarıdan not alma: `focusdeck capture`

- `focusdeck capture "metin"` doğrudan, metinsiz çağrı `capture_command` ile (varsayılan rofi)
  tek satır sorar.
- Yazım bugünkü `pano yakala` ile aynı:
  - `//` sonrası açıklamadır.
  - Başlığın son kelimesi `30dk` ya da `2s` ise şimdiden o kadar sonra; `SS:DD` ise bugün o saat,
    geçmişse yarın. Önünde `yarın` ya da gün kısaltması (`pzt`, `sal`, `çar`, `per`, `cum`, `cmt`,
    `paz` ve ASCII yazımları) varsa o gün, geçmişse bir hafta sonra. Zaman son tarih ve hatırlatma olur.
  - İlk `@ad` kimden bilgisidir. `#kelime` etikettir, `capture_label_aliases` ile çevrilir,
    yalnız rakamdan oluşan `#123` düz metin kalır.
  - `capture_refs` önekiyle başlayıp en az iki rakamla süren kelime (`R12345`) linke çevrilir.
  - Geri kalan kelimeler başlıktır. Başlık boşsa not eklenmez, komut 1 ile çıkar.
- Hedef profilin `capture_into` kutusudur. Plugin ulaşılamazsa not
  `~/.local/state/focusdeck/capture-queue.jsonl`'a yazılır, komut 1 ile çıkar, bir sonraki
  başarılı yakalamada kuyruk boşaltılır.
- Kısayol: Hyprland'de tek `F9` tuşu. NuPhy üst sırası varsayılan olarak medya tuşu gönderiyorsa
  klavyenin Fn kilidi açılır, kurulumda fiziksel tuşla denenir.

## Bildirim: `focusdeck remind`

- `focusdeck remind --send` profildeki `reminders` destekli kutulara son 12 saati sorar, saati
  gelmiş ve daha önce gösterilmemiş olanları `notify-send -u critical` ile gösterir (başlık
  `⏰ <görev>`, gövde `SS:DD`). Gösterilenler `~/.local/state/focusdeck/reminded.json`'a
  `<kutu>:<öğe>@<zaman>` anahtarıyla yazılır, 7 günden eski kayıtlar silinir.
- `focusdeck-remind.service` + `.timer`, her dakika, bugünkü `pano-hatirlat` ile aynı biçim.

## GOWA kurulumu

- pve11 üzerinde yeni Debian 13 LXC (1 vCPU, 512 MB, 4 GB), GOWA sürüm ikilisi, systemd birimi,
  SQLite depo. Basic Auth zorunlu, webhook kapalı.
- Dış erişim Caddy üzerinden `gowa.example.com`: IP filtresi (`import ipfilter`) ve Basic Auth.
  Vhost Caddy reposuna eklenir.
- Eşleştirme QR ile, kullanıcının telefonundan. GOWA'dan mesaj gönderilmez.
- Wiki `kisisel-sunucular` sayfasına yeni LXC satırı eklenir.

## Geçiş

1. focusdeck ve plugin'ler kurulur, iş profili yazılır.
2. Şifreler `~/.config/focusdeck/secrets/` altına taşınır. Vikunja token'ı, Redmine anahtarı ve
   Zulip anahtarı bu sırada yenilenir.
3. `focusdeck-remind.timer` açılır, `pano-hatirlat.timer` kapatılır.
4. Hyprland: `F9` → `focusdeck capture`. Super+I, Super+K ve Super+M bağları kaldırılır.
5. Bir hafta yan yana çalışma. Sorun yoksa Python `pano`, wtfutil ayarı ve `~/.config/pano`
   emekliye ayrılır (`trash-put`).

## Hata ve log

- Log: `~/.local/state/focusdeck/focusdeck.log`, plugin stderr'i kutu adıyla öneklenir.
- Ağ hatası kutuda kırmızı satır, son başarılı liste soluk renkte gösterilmeye devam eder.
- Şifre değerleri log'a yazılmaz, istek gövdeleri log'lanmaz.

## Test

- Birim: capture ayrıştırıcı, protokol kodlama/çözme, `index.toml` seçimi ve sha256 doğrulama,
  ızgara yerleşimi, bağ kapatma kuralı (kaynak hata verdiğinde görev kapanmaz).
- Entegrasyon: test içinde yazılmış sahte bir plugin ile çekirdeğin tüm yöntemleri, çökme ve zaman aşımı.
- Plugin'ler: kayıtlı HTTP cevaplarıyla ayrıştırma testleri.
- Elle: sabit boyutlu tmux bölmesinde açılış, kaydırma, sürükle bırak ve ekran görüntüsü.

## Riskler

- GOWA resmi olmayan istemci. Yalnız okuma riski düşürür, sıfırlamaz.
- NuPhy F9'u Fn olmadan göndermeyebilir. Kurulumda denenir, gerekirse başka tek tuş seçilir.
- Terminalin fare desteği: ghostty ve foot destekliyor. tmux içinde `mouse on` gerekir.
