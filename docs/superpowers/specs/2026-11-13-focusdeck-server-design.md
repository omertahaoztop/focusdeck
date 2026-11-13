# focusdeck sunucu ve Android istemcisi tasarımı

Tarih: 2026-11-13 · Durum: kullanıcı onayı bekliyor

## Amaç

Ofisteki ve evdeki focusdeck aynı panoyu görmüyor. Notlar her makinenin kendi `local.json` dosyasında, plugin token'ları da her makinede ayrı ayrı duruyor. Telefondan panoya bakmanın yolu yok. İlk tasarım "telefonda görmek gereken her şey Vikunja'da" diyerek mobili kapsam dışı bırakmıştı, yerel kayıt tasarımı da makineler arası senkronu. Bu iş ikisini birlikte açıyor.

İstenen:

- focusdeck'in kendi sunucusu olsun. Pano (kutular, notlar, Redmine/Zulip/Gmail/WhatsApp/Vikunja kutuları) sunucuda yaşasın, ofis, ev ve telefon aynı panoyu görsün.
- Ayarla seçilsin: `config.toml`'da sunucu yazmıyorsa focusdeck bugünkü gibi yerelde çalışır, yazıyorsa sunucuya bağlanır.
- Vikunja isteğe bağlı kalsın. Vikunja kutusu profilde varsa sunucuda Vikunja plugin'iyle çalışır, yoksa Vikunja hiç devreye girmez.
- Ardından Android uygulaması: panoyu görme, not ekleme, bitirme, kutular arası sürükle-bırak.

## Hedefler

- `focusdeck server` alt komutu. Aynı program. Sunucudaki profilleri okur, her kutunun plugin'ini sunucuda çalıştırır, panoyu HTTPS arkasında bir REST API olarak sunar.
- İstemci kipi: `config.toml`'da `[server]` bölümü varsa TUI, `capture` ve `remind` plugin başlatmaz, her şeyi sunucudan alır. Profil düzeni de sunucudan gelir.
- Bugün TUI'de çalışan her eylem istemci kipinde de çalışır: listeleme, `x` bitti, plandan çıkarma, `m` ve sürükle-bırak ile taşıma, sıralama, `a` ile not, bağlı görevin Redmine işi kapanınca bitmesi, hatırlatıcı.
- Android uygulaması (Kotlin + Jetpack Compose) aynı API ile: pano, öğe ayrıntısı, not ekleme, bitirme, kutular arası sürükle-bırak, kutu içinde sıralama, hatırlatıcı bildirimi.

## Hedef olmayanlar

- Çevrimdışı çalışma. Sunucuya ulaşılamazsa kutu kırmızı hata satırı gösterir. Yalnız `capture` ile alınan not bugünkü kuyrukta bekleyip sonra gider.
- Birden çok kullanıcı, hesap, rol. Tek kişi, tek token.
- Profili arayüzden ya da telefondan düzenleme. Profiller sunucuda TOML dosyası olarak elle düzenlenir.
- Aynı istemcide yerel ve uzak kutuları karıştırmak. Kip makine başına tektir.
- Vikunja'dan focusdeck kaydına aktarma. Vikunja kutuları sunucuda Vikunja plugin'iyle çalışmaya devam eder.
- iOS, masaüstü GUI, web arayüzü.
- Anlık itme (SSE, WebSocket, FCM). İstemciler yoklar.

## Olmaması gerekenler

- `[server]` yazmayan birinin focusdeck'inde herhangi bir davranışın değişmesi.
- Plugin ayarlarının (`config` tablosu: adresler, e-posta, `*_file` yolları) API'den dışarı çıkması.
- Token olmadan ya da yanlış token'la tek bir öğenin okunabilmesi.
- `capture` ile alınan notun kaybolması ya da kuyruk yeniden gönderdiğinde iki kez yaratılması.
- Sunucu taşınırken mevcut notların kaybolması.

## Yaklaşımlar

1. **Sunucu plugin'leri çalıştırır, istemciler ince (seçilen).** Sunucu bugünkü `PluginHost`'u ve `serve-local`'ı olduğu gibi kullanır. İstemci tarafında her kutunun plugin süreci yerine HTTP konuşan bir uzak kutu durur. Plugin protokolü, `local` kaydı ve plugin'ler değişmez. Token'lar tek yerde (sunucuda) durur. Telefon da Redmine işlerini, mailleri görür.
2. **Yalnız notlar sunucuda, plugin'ler masaüstünde.** Sunucu bir not deposu olur, `local` kutusu dosya yerine HTTP'ye yazar. Daha küçük, sunucuda token durmaz. Telefon yalnız notları görür. Ömer bütün panoyu istediği için elendi.
3. **Çevrimdışı öncelikli eşitleme.** Her makinede yerel kopya, değişiklik günlüğü, birleştirme. Çakışma çözümü gerektirir, plugin kutularında anlamı yok (Redmine'ın kopyası tutulmaz). Ömer "sunucu yoksa hata satırı yeter" dedi.

Ömer 1'i seçti.

## Tasarım

### Kip seçimi

`config.toml`, istemci tarafı:

```toml
[server]
url = "https://deck.example.com"
token_file = "~/.config/focusdeck/secrets/server.token"
```

Sunucu tarafı (aynı dosya, sunucu makinesinde):

```toml
[serve]
listen = "127.0.0.1:8740"
token_file = "~/.config/focusdeck/secrets/server.token"
```

- `[server]` varsa: TUI, `capture`, `remind` istemci kipinde çalışır. Yerel `profiles/` okunmaz, plugin başlatılmaz, `local.json`'a dokunulmaz.
- `[server]` yoksa: bugünkü davranış, bir satırı değişmeden.
- `url` `https://` olmalı. `http://` yalnız `127.0.0.1`, `[::1]`, `localhost` için kabul edilir (yerel deneme). Kural `install.rs`'deki indirme kuralıyla aynı.
- `token_file` öteki `*_file` alanları gibi `secrets.rs` denetiminden geçer: dosya yoksa ya da grup/başkaları okuyabiliyorsa hata.
- Token biçimi serbest, en az 32 karakter. README `openssl rand -hex 32` ile üretmeyi gösterir. *(Benim kararım: token üreten alt komut eklenmez.)*
- `focusdeck server` `[serve]` yoksa hata verip çıkar. `listen` varsayılanı `127.0.0.1:8740`. TLS sunucu programında yok, önündeki Caddy yapar.

### Sunucu

- Açılışta `profiles/*.toml` dosyalarının hepsini okur ve doğrular (bugünkü `profile::load`). Biri bozuksa açılmaz.
- Her profil kutusu için bir kutu nesnesi: `build_host` ile kurulan `PluginHost`, onu koruyan `tokio::sync::Mutex`, son tam liste, son liste zamanı, son hata, `closures_after_list`'in "üst üste boş liste" sayacı. Plugin süreci ilk istekte başlar. Çöken plugin bugünkü gibi sonraki istekte yeniden başlar (`PluginHost::call` bunu zaten yapıyor).
- Plugin'ler `summary` ve `full` görünümü ayırmıyor (plugin reposunda `View` kullanan kod yok). Sunucu plugin'e hep `full` sorar, iki görünüme de aynı listeyi verir.
- Önbellek: `GET items` kutunun son listesi `refresh` süresinden yeniyse onu döner, değilse plugin'e sorar. Bir kutuda eylem ya da bırakma başarılı olunca, bütün profillerde hedef kutunun plugin'ini ya da bırakmadaki `from.plugin`'i kullanan her kutunun önbelleği düşer. Bu TUI'deki `siblings_after_finish` mantığının genişletilmiş hâli: `local` listeleri profiller arasında paylaşılıyor, bir Vikunja görevi birden çok kutuda görünebiliyor.
- Zamanlayıcı yok. Plugin'e yalnız bir istemci sorduğunda gidilir. *(Benim kararım: kimse bakmazken Redmine'ı yoklamak gereksiz.)*
- Plan bağı sunucuda işler: `plan_into`'su olan bir kutu plugin'e sorularak listelendiğinde sunucu `closures_after_list` ile kapanacak bağlı görevleri bulur ve hedef kutuya `done` gönderir. Önbellekten dönen liste sayılmaz, "üst üste boş liste" sayacı yalnız gerçek listelerde ilerler. Kimse bakmıyorsa bağ da işlemez, bugün TUI kapalıyken olduğu gibi. İstemci kipindeki TUI bu işi yapmaz, yoksa iki istemci aynı görevi iki kez bitirmeye çalışır.
- Plugin dili sunucunun `language` ayarı. Plugin'den gelen metinler (hata satırları dahil) bu dilde olur. Arayüz metinleri istemcinin dilinde kalır.
- Sunucu tek süreç. Aynı kutuya gelen istekler kutunun kilidinde sıraya girer, farklı kutular paralel çalışır. Taşımada iki kutunun kilidi aynı anda tutulmaz: kaynak plugin'e yalnız bırakma çağrılır (taşıma `local`'da tek `drop` çağrısıdır), kaynak kutunun önbelleği sonra düşürülür.
- Sunucudaki `local.json`'a yazan tek süreç sunucunun `serve-local` çocuklarıdır. Yine de kayıt dosyası bugünkü süreçler arası kilitle korunur (`local/store.rs`), LXC'de elle çalıştırılan bir `focusdeck` da bozamaz.
- Plugin başlatılamazsa (`describe` hata verirse) profil cevabında kutunun `describe`'ı `null`, `error`'ı dolu olur. O kutuya gelen her istek önce `describe`'ı yeniden dener, başarısızsa `PLUGIN` döner. İstemciler kutuyu kırmızı hata satırıyla gösterir, eylem ve bırakma sunmaz.

### REST API (sürüm 1)

Taban yol `/api/v1`. Her istek `Authorization: Bearer <token>` taşır. Token sabit zamanlı karşılaştırılır. Alan adları protokoldeki gibi `snake_case`, çünkü cevaplar protokolün `Item`, `Describe`, `Source`, `Reminder` tiplerini olduğu gibi taşır. *(Benim kararım: tek sözlük, iki ayrı alan adı düzeni yok.)*

| Yöntem ve yol | Gövde | Cevap |
|---|---|---|
| `GET /profiles` | | `{"profiles": ["pano"], "default": "pano"}`; `default` sunucunun `default_profile`'ı ya da tek profil, yoksa `null` |
| `GET /profiles/{p}` | | `name`, `capture_into`, `capture_refs`, `capture_label_aliases`, `capture_words`, `boxes[]` |
| `GET /profiles/{p}/boxes/{b}/items` | | `{"items": [Item], "listed_at": RFC3339}` |
| `POST /profiles/{p}/boxes/{b}/actions` | `{"item": "id", "action": "done", "args": {}}` | `{"item": Item\|null}` |
| `POST /profiles/{p}/boxes/{b}/drops` | `{"item": Item, "from": Source\|null}` | `{"item": Item}` |
| `POST /profiles/{p}/captures` | `{"text": "...", "at": RFC3339, "lang": "tr"}` | `{"box": "notes", "item": Item}` |
| `GET /profiles/{p}/reminders?since=RFC3339` | | `{"reminders": [{"box": "notes", "at": ..., "item": Item}]}` |

- `boxes[]` öğesi: `id`, `title`, `plugin`, `row`, `col`, `width`, `height`, `refresh` (saniye), `plan_into`, `describe` (plugin başlatılamadıysa `null` ve `error` metni). `config` tablosu **hiçbir zaman** gönderilmez.
- `refresh` istemcinin yoklama aralığıdır. Sunucu profilinde `local` kutular için kısa bir değer (örneğin `30s`) yazmak, öteki makinedeki değişikliğin çabuk görünmesini sağlar.
- `captures`: sunucu metni bugünkü `capture::parse` ile, `at` anına göre, profilin `capture_refs` ve `capture_label_aliases`'ıyla, `lang` dilinin kelimeleri ve sunucunun `capture_words`'üyle ayrıştırır, profilin `capture_into` kutusuna bırakır. Android ve `focusdeck capture` bunu kullanır.
- TUI'nin `a` tuşu bugünkü gibi metni kendisi ayrıştırıp `drops`'a gönderir. Kelimeleri ve kısaltmaları profil cevabındaki `capture_*` alanlarından alır, böylece TUI ile sunucu aynı sonucu verir.
- Hata gövdesi her yerde aynı: `{"error": {"code": "...", "message": "..."}}`.

| Kod | Durum | Ne zaman |
|---|---|---|
| `UNAUTHORIZED` | 401 | Token yok ya da yanlış |
| `NOT_FOUND` | 404 | Profil ya da kutu yok |
| `INVALID` | 422 | Gövde okunamadı, eksik alan, `capture` ayrıştırılamadı, kutunun `capture_into`'su yok |
| `UNSUPPORTED` | 422 | Kutu bu eylemi `describe`'da bildirmiyor ya da bırakma kabul etmiyor |
| `PLUGIN` | 502 | Plugin hata döndü, zaman aşımı, plugin çöktü. `message` plugin'in metni |
| `IN_PROGRESS` | 409 | Aynı `Idempotency-Key` ile ilk istek hâlâ sürüyor |
| `KEY_REUSED` | 422 | Aynı `Idempotency-Key` başka bir gövdeyle geldi |

### Tekrar gönderme

- `drops` ve `captures` isteğe bağlı `Idempotency-Key` başlığı kabul eder. Anahtar istemcide niyetten bir kez üretilir, yeniden denemede aynısı gönderilir.
- Sunucu anahtarı `state_dir/idempotency.json` dosyasında tutar: anahtar, gövdenin SHA-256'sı, durum (`in_progress`, `done`), cevap, zaman. Anahtarı sahiplenme tek bir kilit altında yapılır (oku-kontrol et-yaz ayrı adım değil). Aynı anahtar ve aynı gövde: kayıtlı cevap tekrar döner, plugin'e gidilmez. Farklı gövde: `KEY_REUSED`. İlk istek sürüyorsa: `IN_PROGRESS`.
- Plugin hata dönerse anahtar silinir, istemci aynı anahtarla yeniden deneyebilir. Sunucu cevap yazmadan çökerse anahtar `in_progress` kalır. Açılışta `in_progress` kalan anahtarlar silinir ve log'a yazılır: plugin'in işi yapıp yapmadığı bilinmez, kayıt silinince tekrar deneme ikinci bir not yaratabilir. *(Benim kararım: kaybetmek yerine olası bir çift kayıt.)*
- Saklama süresi 90 gün. `capture` kuyruğu bir notu sınırsız tutabilir ama 90 günden uzun bağlantısız bir makine gerçekçi değil.
- `actions` anahtar almaz. `done` ikinci kez gelirse plugin "öğe yok" der, istemci bunu hata satırı olarak gösterir (bugünkü davranış).

### İstemci kipi: TUI

- Profil seçimi bugünkü kurallarla, sunucunun listesi üstünde: `--profile`, `FOCUSDECK_PROFILE`, `default_profile`, sunucunun `default`'u, tek profil. Hiçbiri yoksa bugünkü hata. İlk açılışta başlangıç profili yazılmaz (sunucuda en az bir profil yoksa sunucu zaten açılmaz).
- Her kutu için `PluginHost` yerine uzak kutu. Worker ikisini aynı arayüzle sürer: `describe` profil cevabından gelir, `list`, `action`, `drop` ilgili uca gider. Ağ hatası, 5xx ve `PLUGIN` hatası kutuda kırmızı satır olur. İstemci isteği 20 saniyede zaman aşımına uğrar (plugin'in 15 saniyesinden uzun).
- Plan bağı istemcide çalışmaz (yukarıda).
- Bağlantı açılışta kurulamazsa TUI açılmaz, hata metni sunucu adresini ve nedeni söyler.

### İstemci kipi: `capture`

- Bugünkü akış korunur: not önce kuyruğa yazılır, sonra gönderilir, gönderilemeyen kuyrukta kalır.
- Kuyruk kaydına `id` alanı eklenir, kayıt yazılırken bir kez üretilir (zaman damgası + süreç kimliği + sayaç, `local` kayıt kimliği gibi). Eski kayıtlarda `id` yoksa `at` ve `text`'in SHA-256'sından türetilir, böylece aynı kayıt her denemede aynı anahtarı alır. `at` nanosaniyeli RFC 3339 olduğu için iki farklı eski kaydın çakışması pratikte imkânsız, çakışırsa da iki kayıt birebir aynı nottur.
- İstemci kipinde gönderme `POST captures` ile, `Idempotency-Key: <id>`. Yerel ayrıştırma yapılmaz, bildirimdeki başlık ve saat sunucunun döndürdüğü `item`'dan gelir. Sunucuya ulaşılamazsa bildirim "kuyruğa alındı" der ve metnin kendisini gösterir.
- Sunucu `INVALID` dönerse kayıt kuyruktan çıkar ve bildirim hatayı gösterir. Yeniden denemek aynı sonucu verir.

### İstemci kipi: `remind`

- `GET reminders` ile sunucudan alınır, `since` bugünkü gibi şimdiden 12 saat öncesi. `since` hatırlatıcının zamanına uygulanan alt sınırdır, sunucu onu plugin'lere olduğu gibi geçirir. Gösterilenler bugünkü gibi makinenin kendi `reminded.json`'ında tutulur, böylece her cihaz hatırlatıcıyı kendisi gösterir. Masaüstü ve telefon açıksa aynı hatırlatıcı ikisinde de çıkar. *(Benim kararım: hangi cihazın önünde olduğun bilinmiyor.)*

### Android uygulaması

- Ayrı repo: `focusdeck-android`. Kotlin, Jetpack Compose, Material 3, `minSdk 26`. HTTP için OkHttp + kotlinx.serialization. *(Benim kararım: Rust reposuna Gradle girmesin, CI'lar ayrı kalsın.)*
- İlk açılış: sunucu adresi ve token girilir. Token Android Keystore anahtarıyla şifrelenip DataStore'da saklanır. Profil birden çoksa seçilir.
- Pano: telefon ekranı dar olduğu için kutular ızgara yerine alt alta bölümler halinde. Bölüm başlığında başlık, öğe sayısı, gecikmiş sayısı. Gecikmiş öğe kırmızı ve `!` ile. Aşağı çekince yenilenir, uygulama öndeyken her kutu kendi `refresh` aralığıyla yoklanır.
- Öğe ayrıntısı: başlık, açıklama, etiketler, kimden, son tarih, linkler (tarayıcıda açılır), `url`.
- Bitirme: kutu `done` destekliyorsa sola kaydırma ve ayrıntı ekranında düğme.
- Not ekleme: yüzen düğme, tek satırlık alan, `captures` ucu. Sözdizimi masaüstündekiyle aynı (`//` açıklama, `#etiket`, `@kimden`, sondaki tarih/saat).
- Sürükle-bırak: öğeye uzun basınca sürüklenir, `accepts_drop` olan bir kutunun bölümüne bırakılınca `drops` çağrılır (`from` kaynak kutunun plugin'i ve öğe kimliği). Aynı kutu içinde bırakmak, kutu `reorder` destekliyorsa sıralamadır. Ayrıca öğe menüsünde "Taşı" seçeneği hedef listesiyle (sürüklemenin zor olduğu uzun panolar için).
- Hatırlatıcı: WorkManager ile 15 dakikada bir ve uygulama her açıldığında `since = şimdi - 12 saat` ile hatırlatıcılar alınır. Zamanı geçmiş ve henüz gösterilmemiş olanlar hemen bildirilir (telefon üç gün kapalı kaldıysa yalnız son 12 saattekiler), önümüzdeki 24 saattekiler için AlarmManager ile tam zamanlı alarm kurulur. FCM ve Google servisi yok. Gösterilenler cihazda tutulur.
- Dil: Türkçe ve İngilizce, cihaz diline göre.
- Dağıtım: GitHub Actions imzalı APK üretip sürüm sayfasına koyar, telefona elle kurulur.

### Kurulum

- pve11 üzerinde yeni bir Debian 13 LXC, `focusdeck` kullanıcısı, systemd birimi (`Description=` yok). focusdeck sürüm ikilisi ve plugin'ler `focusdeck plugin install` ile kurulur.
- Profil (`pano.toml`) ve gizli dosyalar (Vikunja token, Redmine anahtarı, Zulip anahtarı, Gmail istemci ve token dosyaları, GOWA parolası, sunucu token'ı) LXC'ye 0600 olarak taşınır.
- Mevcut notlar: ofis ve ev makinesindeki `local.json` dosyaları birleştirilip sunucuya konur. Kayıt kimlikleri zaman damgası + süreç kimliği + sayaçtan üretildiği için iki dosyanın kimlikleri çakışmaz. Birleştirme `jq` ile yapılır, kopya ID kontrolüyle doğrulanır.
- Caddy vhost'u Forgejo `Caddy` reposuna commit'lenir, alan adı `deck.omertahaoztop.tr`. LXC'ye dışarıdan yalnız Caddy erişir.
- Masaüstlerine `[server]` bölümü ve token dosyası yazılır. Yerel `local.json` yedek olarak kalır.
- Wiki'ye yeni servis sayfası (`journal_write kind=wiki`).

### Dil

Yeni arayüz metinleri (sunucu adres/token hataları, bağlantı hatası, sunucu açılış satırları, kuyruk bildirimleri) `en.ftl` ve `tr.ftl`'ye girer, parite testi kapsar. API hata `message`'ları sunucunun dilinde.

### Belgeler

README ve README.tr.md'ye "Sunucu" bölümü: `[serve]` ve `[server]` ayarları, token üretimi, Caddy örneği, istemci kipinde neyin nereden geldiği, API tablosu.

## Riskler

- Token'ların hepsi tek LXC'de. LXC'nin ele geçmesi Redmine, Zulip, Gmail (salt okunur), Vikunja ve GOWA erişimi demek. Azaltma: LXC'ye dışarıdan yalnız Caddy, gizli dosyalar 0600, `focusdeck` kullanıcısı root değil.
- Gmail OAuth token'ının yenilenmesi sunucuda yapılır. İlk yetkilendirme masaüstünde yapılıp dosya taşınır.
- Sunucu çökerse hiçbir makinede pano yok. Kabul edildi (çevrimdışı hedef değil). `[server]` satırı silinince makine yerel kipe döner, eski `local.json` yerinde durur.

## Kurucuya kalanlar

- Modül, fonksiyon ve tip adları. HTTP sunucu crate'i (önerim `axum`, tokio zaten var).
- Uzak kutu ile `PluginHost`'u worker'a aynı arayüzle vermenin biçimi (enum ya da trait).
- Android'de ekran düzeninin ayrıntısı, sürükleme görsel geri bildirimi.
- Test düzeni: API uçları için gerçek sunucu + `fake-plugin` ile uçtan uca testler, token ve `config` sızmaması testleri, tekrar gönderme testleri, istemci kipinin `mockito` ile testleri.
