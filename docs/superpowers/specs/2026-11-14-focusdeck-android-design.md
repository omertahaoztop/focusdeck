# focusdeck Android uygulaması tasarımı

Tarih: 2026-11-14 · Durum: kullanıcı onayı bekliyor · Önceki tasarım: `2026-11-13-focusdeck-server-design.md` ("Android uygulaması" bölümü bu belgeyle ayrıntılanır, çelişkide bu belge geçerlidir)

## Neden

Pano artık sunucuda yaşıyor (`https://deck.omertahaoztop.tr`, focusdeck 0.4.0). Ofiste ve evde masaüstünden görülüyor, telefondan görülemiyor. Ömer telefondan da panoya bakmak, not eklemek, iş bitirmek ve öğeleri kutular arasında taşımak istiyor. Telefon Pixel 10 Pro Fold: kapalıyken önde dar bir ekran, açıkken neredeyse kare büyük bir ekran. Sunucu bu yüzden `ipfilter` yerine `geofilter-tr` ile açıldı, telefon Türkiye'deki mobil veriden bağlanabilsin diye.

Başarı: telefonda yakalanan not birkaç saniye içinde masaüstü panosunda görünür, telefonda bitirilen iş masaüstünden kaybolur, hatırlatıcı telefonda zamanında çalar.

## Hedefler

- Sunucunun REST API'si (sürüm 1) ile pano: profil seçimi, kutular, öğeler, öğe ayrıntısı.
- Not ekleme (`captures`), bitirme (`done`), kutular arası taşıma (`drops`), kutu içinde sıralama (`reorder`).
- Katlanır telefona göre iki düzen: kapalıyken sekmeli liste, açıkken masaüstündeki ızgara.
- Hatırlatıcı bildirimi, FCM ve Google servisleri olmadan.
- Türkçe ve İngilizce, cihaz diline göre.
- GitHub Actions ile imzalı APK, telefona elle kurulur.

## Hedef olmayanlar

- Sunucuda değişiklik. API Plan A'da dondu, uygulama ona uyar. Uygulamanın ihtiyacı yüzünden API değişmesi gerekirse bu tasarım durur ve konuşulur.
- Çevrimdışı pano. Sunucuya ulaşılamazsa kutularda hata satırı görünür. Yalnız yakalanan notlar kuyrukta bekler (aşağıda).
- Profili telefondan düzenleme, kutu ekleme/silme.
- Plan bağı ("plana al / plandan çıkar"), öğe silme, `move` eylemi. Masaüstünde kalır.
- Ana ekran widget'ı, hızlı ayarlar kutucuğu, paylaş menüsünden not. İleride ayrı iş olabilir.
- Google Play, F-Droid, otomatik güncelleme. iOS, tablet için ayrı tasarım.

## Kaçınılacaklar

- Yanlışlıkla kaydırınca geri alınamayan iş: WhatsApp kutusunda `done` mesajı WhatsApp'tan siler, geri gelmez.
- Token'ın loglara, hata metinlerine, yedeklere ya da bildirimlere sızması.
- Pil tüketen sürekli yoklama: uygulama arka plandayken kutular yoklanmaz.

## Kısıtlar

- Repo `github.com/omertahaoztop/focusdeck-android`, yerelde `~/Desktop/personal/focusdeck-android`. Public, MIT (focusdeck gibi). Commit mesajları İngilizce, tek satır.
- Kotlin, Jetpack Compose, Material 3. `minSdk 26`, `compileSdk` ve `targetSdk` 36. OkHttp, kotlinx.serialization, WorkManager, DataStore, Jetpack WindowManager ve Material 3 adaptive. Başka ağır bağımlılık yok (DI çerçevesi, Retrofit, Room yok).
- `applicationId` `tr.omertahaoztop.focusdeck`. Sürüm 0.1.0, etiket `v0.1.0`.
- Yerel araçlar: JDK 21, Gradle 9.8, Android SDK `/opt/android-sdk` (platform 36, build-tools 37). Emülatör paketi ve sistem imajı yok, kurulacak. `/dev/kvm` var.

## API sözleşmesi (özet)

Ayrıntı ve hata tablosu önceki tasarımın "REST API" bölümünde, tiplerin kaynağı `crates/focusdeck/src/api.rs` ve `crates/focusdeck-protocol/src/types.rs`. Kotlin tipleri bunlara birebir uyar, alan adları `snake_case`, bilinmeyen alanlar yok sayılır.

| Uç | Uygulamada |
|---|---|
| `GET /api/v1/profiles` | kurulumda ve ayarlarda profil listesi |
| `GET /api/v1/profiles/{p}` | kutular: `id`, `title`, `plugin`, `row`, `col`, `width`, `height`, `refresh` (sn), `describe` (`actions`, `accepts_drop`, `reminders`) ya da `null` + `error` |
| `GET …/boxes/{b}/items` | `{"items": [Item], "listed_at"}` |
| `POST …/boxes/{b}/actions` | `{"item": id, "action": "done" \| "reorder", "args": {}}`; `reorder` için `args = {"position": n}`, `n` kutunun tam listesindeki hedef sıra (0 tabanlı) |
| `POST …/boxes/{b}/drops` | `{"item": Item, "from": {"plugin": kaynak kutunun plugin'i, "id": öğe kimliği}}`, `Idempotency-Key` ile |
| `POST /api/v1/profiles/{p}/captures` | `{"text", "at": cihaz saat dilimli RFC 3339, "lang": "tr" \| "en"}`, `Idempotency-Key` ile; cevap `{"box", "item"}` |
| `GET /api/v1/profiles/{p}/reminders?since=` | `{"reminders": [{"box", "at", "item"}]}` |

`Item`: `id`, `title`, `subtitle?`, `url?`, `due?`, `overdue`, `source?`, `description?`, `labels[]`, `from?`, `links[{label, url}]`.

Hata gövdesi `{"error": {"code", "message"}}`. Kodlar: `UNAUTHORIZED` 401, `NOT_FOUND` 404, `INVALID` 422, `UNSUPPORTED` 422, `KEY_REUSED` 422, `IN_PROGRESS` 409, `PLUGIN` 502, `INTERNAL` 500.

Zaman aşımı: bağlantı 10 sn, istek 60 sn. Sunucuda plugin başına her alışveriş 15 sn ile sınırlı (`host.rs:19`), soğuk kutu ilk istekten önce `describe` ve `configure` çalıştırdığı için bir liste en kötü 45 sn sürebilir. Masaüstü istemcisinin 20 sn'si bu yüzden telefonda kullanılmaz. `GET /profiles/{p}` kısmi sonuç döndürmez, bekleme boyunca kutu iskeletleri gösterilir.

`actions` sunucuda ayrık çalışmaz ve `Idempotency-Key` almaz (`server/http.rs:181-190`). İstemci zaman aşımına düşüp bağlantıyı kapatırsa plugin işi yarıda kesilebilir ya da bitirmiş olabilir, sonuç bilinmez. `drops` ve `captures` anahtarlı ve ayrık, aynı anahtarla tekrar güvenli.

## Yaklaşımlar

Teknoloji önceden seçildiği için yapım şeklinde tek mantıklı yol var: tek modüllü uygulama, ekran durumu `ViewModel` + `StateFlow`, bağımlılıklar elle kurulur. Düzen için üç seçenek iç ekranda, iki seçenek dış ekranda tarayıcıda maket olarak gösterildi.

- Kapalı ekran: A (katlanır bölümlerle tek liste) ve B (Mono / Kişisel sekmeleri). **Ömer B'yi seçti.**
- Açık ekran: 1 (masaüstündeki ızgara), 2 (aynı listenin geniş hâli), 3 (solda liste, sağda ayrıntı). **Ömer 1'i seçti.** 2 büyük ekranı boşa harcar, 3'te kutular yan yana görünmediği için kutudan kutuya sürükleme olmaz.

## Tasarım

### Kurulum ve ayarlar

- İlk açılışta: sunucu adresi, token, "Bağlan" düğmesi. Bağlan `GET /profiles` dener. Başarılıysa ve tek profil varsa ya da sunucu bir `default` döndürdüyse o profil seçilir, birden çok profil varsa liste gösterilir (varsayılan işaretli).
- Adres `https://` olmalı. `http://` yalnız `localhost`, `127.0.0.1` ve `10.0.2.2` (emülatörden geliştirme makinesi) için kabul edilir; ağ güvenlik ayarı da düz HTTP'ye yalnız bu üç adreste izin verir. Kural masaüstündeki `[server] url` kuralıyla aynı.
- Hata metinleri ayırt eder: adres çözülemedi, bağlantı kurulamadı, zaman aşımı, TLS hatası, 401 (token yanlış), 403 (sunucuya bu ağdan erişim yok: `geofilter-tr` ya da Cloudflare), diğer.
- Token Android Keystore'da üretilen AES-GCM anahtarıyla şifrelenir, şifreli hâli DataStore'da durur. Adres, profil ve arayüz durumu (seçili sekme, katlı bölümler) düz DataStore'da.
- Adres `https://` olmalı. Release sürümü düz HTTP'ye hiç izin vermez. Debug sürümü emülatörden geliştirme için `http://` adresini `localhost`, `127.0.0.1`, `[::1]` ve `10.0.2.2` (emülatörde geliştirme makinesinin adresi) için kabul eder, ağ güvenlik ayarı (`src/debug/res/xml/network_security_config.xml`) da düz HTTP'ye yalnız bunlarda izin verir. Masaüstündeki `[server] url` kuralı `127.0.0.1`, `[::1]`, `localhost` ile sınırlı (`install.rs:135-137`), `10.0.2.2` telefona özgü ektir.
- Ayarlar ekranı: adres ve token değiştirme (kurulum ekranının aynısı), profil değiştirme, bildirim izni durumu ve sistem ayarına giden bağ, sürüm.
- 401 her ekranda kurulum ekranına döndürür, adres dolu gelir, token boş.

### Pano: hangi düzen ne zaman

- Pencere genişlik sınıfı en az Medium (600 dp) **ve** yükseklik sınıfı en az Medium (480 dp) ise ızgara, değilse liste. Pixel 10 Pro Fold açıkken ızgara, kapalıyken liste, kapalı ekran yatayda da liste.
- Telefon açılıp kapanınca uygulama yeniden başlamaz ya da durumu `ViewModel` ve `rememberSaveable` ile korur: seçili sekme, açık ayrıntı, her kutunun kaydırma konumu (kutu kimliğine bağlı).
- İki düzen de aynı veriyi gösterir, aynı eylemleri sunar.

### Pano: kapalı ekran (liste, sekmeli)

- Sekmeler kutu başlıklarındaki önekten çıkar: başlık `"<grup> · <ad>"` biçimindeyse (ayraç boşluk, orta nokta U+00B7, boşluk) grup `<grup>`, bölüm başlığı `<ad>` olur. Sekme sırası grubun ilk kutusunun profildeki sırası. Öneksiz kutular "Diğer" sekmesine düşer, sona. Yalnız tek grup çıkıyorsa sekme çubuğu gösterilmez.
- Sekme etiketinde grup adı ve gruptaki toplam öğe sayısı; grupta gecikmiş öğe varsa kırmızı nokta.
- Sekme içinde kutular profil sırasıyla bölüm olarak alt alta. Profil sırası dosyadaki sıradır, `row/col` listede kullanılmaz.
- Bölüm başlığı: ad, öğe sayısı, gecikmiş varsa kırmızı `N!`. Başlığa dokununca bölüm katlanır/açılır, durum cihazda saklanır. Boş kutular varsayılan katlı, art arda gelen boş kutular tek satırda birleşir ("Inbox · Bu akşam · Fikirler 0"). Dolu kutular varsayılan açık.
- Sekmeler arasında yana kaydırarak da geçilir.
- Öğeye dokununca ayrıntı tam sayfa açılır.

### Pano: açık ekran (ızgara)

- Kutular profildeki `row`, `col`, `width`, `height` ile yerleşir. Toplam sütun sayısı `max(col + width)`, toplam satır `max(row + height)`. Sütun genişliği ekran genişliğinin eşit payı: masaüstündeki boş ayraç sütunu dar bir boşluk olarak kalır.
- Satır yüksekliği ekran yüksekliğinin eşit payıdır, en az 112 dp. Toplam sığmazsa ızgaranın tamamı dikey kayar.
- Her kutu kendi içinde kayar. Başlıkta ad, sayı, gecikmiş sayısı. Izgarada katlama ve sekme yok.
- Öğeye dokununca ayrıntı alttan açılan panelde (modal bottom sheet) açılır.

### Öğe satırı ve ayrıntı

- Satır: başlık (en çok iki satır), varsa `subtitle` ya da son tarih kısa biçimde ("bugün 17:00", "yarın", "12 Kas"). Gecikmiş öğe kırmızı ve başında `!`.
- Ayrıntı: başlık, kutu adı, `subtitle`, açıklama, etiketler, kimden, son tarih (tam), linkler ve `url` (dokununca tarayıcıda açılır, Custom Tabs gerekmez).
- Ayrıntıdaki düğmeler: "Bitti" (kutu `done` destekliyorsa), "Taşı…" (bırakma kabul eden başka kutu varsa).

### Bitirme

- Kutunun `describe.actions` listesinde `done` varsa öğe sola kaydırılınca ya da ayrıntıdaki "Bitti" ile bitirilir.
- Öğe hemen listeden kalkar, altta 5 sn'lik "Bitti: <başlık> · Geri al" çubuğu çıkar. İstek çubuk kapanınca gönderilir. "Geri al" isteği hiç göndermez, öğe yerine döner. Uygulama bu arada arka plana giderse bekleyen istek hemen gönderilir.
- `done` hiçbir zaman kendiliğinden tekrar gönderilmez. Sunucu açık bir hata döndürürse (`PLUGIN`, `NOT_FOUND`, `UNSUPPORTED`, `INTERNAL`) öğe geri gelir, çubuk hatayı söyler.
- Zaman aşımı ya da bağlantı kopması sonucu belirsiz bırakır. Uygulama önce kutuyu yeniden listeler: öğe artık yoksa bitmiş sayılır ("Bitti" çubuğu), hâlâ varsa geri gelir ve çubuk "Sonuç belirsiz, öğe hâlâ listede" der. Yeniden listeleme de başarısızsa öğe soluk gösterilir ve sonraki başarılı yoklama kararı verir. Kullanıcı tekrar bitirmeyi elle seçer.
- `reorder` aynı kurala uyar: tekrar gönderilmez, hata ya da belirsizlikte kutu yeniden listelenir.
- Başarıda kutu yeniden listelenir. Sunucu aynı plugin'i kullanan kutuların önbelleğini düşürdüğü için uygulama aynı plugin'li öteki kutuları da yeniden listeler.

### Not ekleme

- Yüzen "+" düğmesi tek satırlık bir giriş açar (alt panel, klavye açık). Sözdizimi masaüstüyle aynı, ayrıştırmayı sunucu yapar. Alanın altında kısa ipucu: `// açıklama  #etiket  @kimden  yarın 9:00`.
- Gönderilen not önce cihazdaki kuyruğa yazılır (id: UUID, metin, `at`, `lang`), sonra `POST captures` ile `Idempotency-Key: <id>` gönderilir. Başarıda kuyruktan çıkar, çubukta "Eklendi: <sunucunun döndürdüğü başlık> → <kutu>" görünür, hedef kutu yeniden listelenir.
- Ağ hatası, zaman aşımı, 5xx, `PLUGIN` ya da `IN_PROGRESS`: not kuyrukta kalır, çubuk "kuyruğa alındı" der. WorkManager tek seferlik işi (ağ bağlı kısıtıyla, üstel geri çekilmeyle) kuyruğu aynı anahtarlarla gönderir. Uygulama her açıldığında da kuyruk denenir.
- 401: not kuyrukta kalır, kurulum ekranına gidilir.
- Kalıcı hatalar `INVALID`, `KEY_REUSED`, `UNSUPPORTED` (profilin `capture_into` kutusu bırakma kabul etmiyor) ve `NOT_FOUND` (profil sunucuda yok): tekrar denenmez. Not ön planda gönderiliyorsa kuyruktan çıkar, metni giriş alanına geri konur ve sunucunun hata metni gösterilir. Arka plandaki gönderimde not kuyrukta "gönderilemedi" durumuna geçer, hata metniyle birlikte saklanır.
- Panoda üstte ince bir satır bekleyenleri ("2 not gönderilmeyi bekliyor") ve gönderilemeyenleri ("1 not gönderilemedi") sayar. Gönderilemeyenler satırına dokununca liste açılır: metin, hata, "Düzenle" (metni giriş alanına koyar, gönderim yeni anahtar alır, eski kayıt silinir) ve "Sil".

### Taşıma ve sıralama

- Öğeye uzun basınca sürüklenir. Parmağın altında öğenin yarı saydam kopyası; bırakma kabul eden (`describe.accepts_drop`) kutular çerçeveli, kabul etmeyenler soluk.
- Başka kutunun üstüne bırakmak `POST drops` (`from` = kaynak kutunun plugin'i + öğe kimliği, `Idempotency-Key` her bırakmada yeni UUID, yeniden denemede aynısı). Başarıda kaynak ve hedef kutular yeniden listelenir.
- Aynı kutu içinde başka bir satırın yerine bırakmak, kutu `reorder` destekliyorsa `actions` + `reorder` + `position`. Desteklemiyorsa bırakma hiçbir şey yapmaz.
- Liste düzeninde katlı bölüm de hedeftir, üstünde 600 ms beklenirse açılır. Sekmeler arası sürükleme yok; "Taşı…" kullanılır.
- Kabul etmeyen kutuya bırakılırsa istek gönderilmez, çubuk "<kutu> bırakma kabul etmiyor" der.
- "Taşı…": uzun basıp sürüklemeden bırakınca açılan menüde ve ayrıntıda. Bırakma kabul eden, kaynak dışındaki kutuları grup başlıklarıyla listeler, seçim `drops` gönderir.

### Yoklama ve hatalar

- Uygulama öndeyken her kutu kendi `refresh` aralığıyla listelenir. Arka planda yoklama yok. Aşağı çekince (liste) ya da üst çubuktaki yenile düğmesiyle (ızgara) hepsi hemen listelenir.
- Profil (`GET /profiles/{p}`) açılışta, aşağı çekince ve uygulama öne her geldiğinde yeniden alınır; kutu düzeni sunucuda değiştiyse ekran ona uyar.
- `describe` `null` olan kutu: kırmızı satırda `error` metni, eylem ve bırakma yok.
- Bir kutunun listesi başarısızsa son başarılı liste soluk kalır, altında kırmızı hata satırı (ağ hatası, zaman aşımı ya da `PLUGIN` metni). Sonraki başarılı yoklamada kalkar.
- Sunucuya hiç ulaşılamıyorsa üstte tek satır "Sunucuya ulaşılamıyor · <neden>".
- Token, `Authorization` başlığı ve istek gövdeleri hiçbir zaman loglanmaz. Hata metinlerinde adres geçebilir, token geçmez.

### Hatırlatıcılar

- Bildirim izni (Android 13 ve üstü) kurulum bittikten sonra bir kez sorulur. Reddedilirse ayarlarda durum ve sistem ayarına bağ görünür, uygulama başka bir şey yapmaz.
- Tam zamanlı alarm `USE_EXACT_ALARM` izniyle kurulur (elle kurulan uygulama, Play kuralı yok). İzin yoksa (eski cihazda ya da kaldırılmışsa) `setAndAllowWhileIdle` ile yaklaşık zamanlı kurulur.
- Eşitleme işi: WorkManager periyodik iş (15 dk, ağ bağlı kısıtıyla), uygulama her açıldığında, cihaz yeniden başladığında (`BOOT_COMPLETED`) ve saat dilimi değiştiğinde çalışır. Seçili profil için `GET reminders?since=<şimdi − 12 saat>`.
- Sunucu üst sınır koymaz, `since`'ten sonraki bütün açık öğeleri döndürür (`local/plugin.rs:265-266`). 24 saat sınırını istemci uygular. Karar her hatırlatıcı için, anahtar `kutu + öğe id + at`:
  - daha önce gösterildiyse atla;
  - `at` geçmişte (son 12 saat içinde, `since` zaten bunu sağlar) ise hemen bildir;
  - `at` önümüzdeki 24 saat içindeyse o ana alarm kur (aynı anahtar için tek alarm, yeniden kurmak eskisinin yerine geçer);
  - daha ilerideyse bir şey yapma, sonraki eşitleme kurar.
- Önceki eşitlemede kurulmuş ama bu cevapta artık olmayan alarmlar iptal edilir (öğe bitti ya da tarihi değişti).
- Alarm çalınca bildirim gösterilir ve anahtar "gösterildi" olarak yazılır. Gösterilen anahtarlar 48 saatten eskiyse silinir.
- Bildirim: başlık öğe başlığı, metin kutu adı ve saat, kanal "Hatırlatıcılar". Dokununca uygulama açılır ve öğenin ayrıntısı gösterilir. Bildirimde token ya da adres yok.
- Masaüstü ve telefon aynı hatırlatıcıyı ayrı ayrı gösterir (önceki tasarımın kararı).

### Dil

- Arayüz metinleri `values/strings.xml` (İngilizce) ve `values-tr/strings.xml`. Cihaz dili Türkçe ise Türkçe, değilse İngilizce.
- `captures` isteğindeki `lang` aynı kurala göre `tr` ya da `en`.
- Tarih biçimleri cihaz yereline göre. Sunucudan ve plugin'den gelen metinler (hata satırları dahil) olduğu gibi gösterilir.
- İki dosyadaki anahtarların aynı olduğunu bir birim testi denetler.

### Derleme, imza, dağıtım

- Gradle wrapper ve sürüm kataloğu (`gradle/libs.versions.toml`) repoda. Kütüphane sürümleri yazıldığı gün en güncel kararlı sürümler.
- GitHub Actions:
  - her push ve PR: `./gradlew lint testDebugUnitTest assembleDebug`;
  - `v*` etiketi: imzalı `assembleRelease`, APK ve `.sha256` dosyası GitHub sürüm sayfasına. Action'lar commit SHA'sına sabitlenir (focusdeck-plugins'teki gibi).
- İmza anahtarı yerelde `keytool` ile üretilir, GitHub Actions secret'larına (`SIGNING_KEYSTORE_BASE64`, `SIGNING_STORE_PASSWORD`, `SIGNING_KEY_ALIAS`, `SIGNING_KEY_PASSWORD`) `gh secret set` ile ekranda göstermeden konur. Anahtar kaybolursa telefondaki uygulama güncellenemez, kaldırıp yeniden kurmak gerekir. Yedek Bitwarden'de tek kayıt: keystore dosyası ek olarak, alias ve iki parola aynı kayıtta. Kaydı Ömer ekler, dosya geçici olarak 0600 ile hazırlanır ve kayıt doğrulandıktan sonra `trash-put` ile silinir.
- README.md (İngilizce) ve README.tr.md: ne olduğu, ekran görüntüleri (kapalı ve açık ekran), kurulum (APK indir, sha256 doğrula, kur), sunucu adresi ve token, derleme.

### Test ve doğrulama

- JVM birim testleri: API istemcisi MockWebServer ile (Bearer başlığı, hata gövdesi → hata türü eşlemesi, zaman aşımının "belirsiz" diye ayrılması, yeniden denemede aynı `Idempotency-Key`), sekme gruplama (önekli, öneksiz, tek grup), ızgara yerleşim hesabı, hatırlatıcı kararı (saf fonksiyon: hatırlatıcılar + gösterilenler + şimdi → bildir / alarm kur / iptal), bitirme sonucu kararı (hata / belirsiz + yeniden liste), not kuyruğu (her hata kodunun kuyruk durumu), dil dosyası paritesi.
- Compose testleri Robolectric ile, JVM'de: kaydırınca bitirme ve geri alma, sekme geçişi, katlanan bölüm, ızgara düzeni, "Taşı…" menüsü.
- Emülatörde elle uçtan uca: katlanır Pixel AVD'si, geliştirme makinesinde yerel `focusdeck server` (deneme profili, `local` kutular, `http://10.0.2.2:8740`). Kapalı ve açık ekranda not ekleme, bitirme, sürükleyip taşıma, sıralama, hatırlatıcı bildirimi; aynı sonucun masaüstü TUI'de görülmesi. Her biri ekran görüntüsüyle.
- Son adım Ömer'in telefonunda: `deck.omertahaoztop.tr`'ye mobil veriden bağlanma, bir not ve bir bitirme.

## Ömer'in kararları

- Android, ayrı repo, Kotlin + Compose (2026-11-13).
- Kapalı ekranda Mono / Kişisel sekmeleri (B), açık ekranda masaüstündeki ızgara (1) (2026-11-14, tarayıcıdaki maketlerden).
- Sunucuya `geofilter-tr` ile erişim, telefon mobil veriden bağlansın diye (2026-11-14).
- İmza anahtarının yedeği Bitwarden'de (2026-11-14).

## Benim kararlarım (düzeltilebilir)

- Sekme grubu başlıktaki `"<grup> · "` önekinden; öneksizler "Diğer"; tek grupta sekme çubuğu yok.
- Sekmede bölümler katlanır, boşlar varsayılan katlı ve birleşik.
- Bitirmede 5 sn geri alma, istek ondan sonra gider (WhatsApp silmesi geri alınamadığı için).
- `USE_EXACT_ALARM`, `allowBackup=false`, `applicationId` `tr.omertahaoztop.focusdeck`, public MIT repo.
- İstek zaman aşımı 60 sn; `done` ve `reorder` kendiliğinden tekrar gönderilmez, belirsiz sonuç yeniden listeyle çözülür. Sunucuya dokunulmaz.
- Düz HTTP yalnız debug sürümünde ve yalnız yerel adreslerde.
- Telefonda da not kuyruğu (masaüstündeki `capture` kuyruğunun karşılığı), çünkü mobil veri kopar. Arka planda kalıcı hata alan not silinmez, "gönderilemedi" olarak bekler.
- Robolectric ile CI'da UI testi, emülatör testleri yerelde (GitHub runner'ında emülatör yavaş ve kararsız).

## Kurucuya kalanlar

- Paket, sınıf ve dosya adları; ekran yönlendirmesi (Navigation Compose ya da elle).
- Sürükle-bırakın Compose'daki uygulanışı (platform sürükle-bırak API'si ya da `pointerInput` ile elle), yeter ki yukarıdaki görsel geri bildirim ve hedef kuralları tutsun.
- Renk, tipografi, boşluk ayrıntıları Material 3 varsayılanlarıyla; dinamik renk (Material You) açık.
- Kütüphanelerin tam sürümleri ve AGP sürümü.

## Riskler

- Token telefonda. Telefon kaybolursa sunucu token'ı yenilenir (`openssl rand -hex 32`, sunucu yeniden başlatılır, masaüstlerindeki `server-token` güncellenir). Bu adım servis wiki sayfasına yazılır.
- `geofilter-tr` ülke denetimini Cloudflare GeoIP kuralına bırakıyor, yurt dışından denenmedi. Yurt dışında uygulama 403 alır ve bunu ayrı bir hata metniyle söyler.
- Pixel'de pil iyileştirmesi WorkManager'ı geciktirebilir. Tam zamanlı alarmlar bundan etkilenmez, yalnız 24 saatten uzak hatırlatıcıların kurulması gecikir.
