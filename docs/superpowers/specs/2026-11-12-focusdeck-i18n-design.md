# focusdeck çok dillilik tasarımı

Tarih: 2026-11-12 · Durum: onaylandı

## Amaç

focusdeck herkese açık yayınlandı ama arayüzü yalnız Türkçe konuşuyor. Türkçe bilmeyen biri alt satırdaki ipuçlarını, `?` yardımını, hata metinlerini okuyamıyor; `capture` sözdizimi de `yarın`, `pzt`, `30dk` gibi Türkçe kelimelerle çalışıyor. Ana README İngilizce olacak, ekran görüntüleri de İngilizce arayüzü göstermeli.

İstenen: program kullanıcının dilini konuşsun, dil sistemden otomatik gelsin ama ayarlanabilsin. Çeviriler topluluğun Rust bilmeden ekleyebileceği dosyalarda dursun. Kullanıcı `capture` için kendi kelimelerini ekleyebilsin.

## Hedefler

- Kullanıcıya görünen her metin (TUI, CLI çıktısı ve hataları, `--help`, bildirimler, dört plugin'in ürettiği alt satırlar ve hatalar) dil dosyasından gelir. İlk sürümde İngilizce (`en`) ve Türkçe (`tr`) gelir.
- Dil sırasıyla şuradan seçilir: `config.toml`'daki `language`, sonra `LC_ALL`, `LC_MESSAGES`, `LANG`. `tr_TR.UTF-8` gibi değerden dil kodu (`tr`) alınır. `C`, `POSIX`, boş değer ya da dosyası olmayan dil İngilizceye düşer.
- Temel dil İngilizce. Bir dil dosyasında mesaj eksikse o mesaj İngilizce görünür, kullanıcı hiçbir zaman mesaj kimliğini görmez.
- Yeni dil eklemek için kod değişmez: dil dosyalarını ekleyip PR açmak yeter. Her iki README'de bunu anlatan kısa bir "Translating" / "Çeviri" bölümü olur.
- `capture` zaman kelimeleri (yarın, gün kısaltmaları, dakika ve saat birimi) etkin dilin dosyasından gelir. Çevirmen kendi dilinin kelimelerini dosyaya yazar. Kullanıcı `config.toml`'da kendi kelimelerini ekler.
- README.md İngilizce, README.tr.md Türkçe. İngilizce README İngilizce arayüzün, Türkçe README Türkçe arayüzün ekran görüntülerini gösterir. Görüntüler tam ekran terminalden alınır ve uydurma veriyle üretilir.

## Hedef olmayanlar

- Weblate ya da başka bir çeviri platformunun kurulması. Format onlarla uyumlu olur, platform ayrı iş.
- Sağdan sola yazılan diller, çalışırken dil dosyası indirme, tarih biçimini dile göre değiştirme (`SS:DD` her dilde aynı kalır).
- Kullanıcının yazdığı metinler: kutu başlıkları, profil adları, plugin adları çevrilmez.
- Plugin'lerin sakladığı veri biçimi değişmez: Vikunja'daki `kimden:` etiket öneki ve `focusdeck-source:` satırı veridir, dil ne olursa olsun aynı kalır. Yoksa eski görevler okunamaz.

## Olmaması gerekenler

- Ekranda `capture-tomorrow` gibi bir mesaj kimliğinin görünmesi.
- Türkçe kullanan birinin bugünkü alışkanlığının bozulması. `tr` dilinde `capture` bugün kabul ettiği her kelimeyi aynı anlamla kabul etmeye devam eder (`yarin`, `pts`, `prs` gibi aksansız ya da eski biçimler dahil).
- İngilizce kullanan birinin Türkçe kelime öğrenmek zorunda kalması.
- Kodda yazım hatalı bir mesaj kimliğinin sessizce İngilizceye ya da boşluğa düşmesi. Böyle bir hata derlemede ya da test takımında yakalanır.

## Kısıtlar

- Plugin protokolü değişmez (sürüm 1 kalır). Dil bilgisi plugin'e protokolle değil ortam değişkeniyle gider.
- Bugünkü kullanıcı (`LANG=tr_TR.UTF-8`, `language` yok) terminalden açtığı focusdeck'te güncellemeden sonra her şeyi Türkçe görür.
- cron, systemd timer ya da pencere yöneticisi kısayolu `LANG` taşımayabilir; o zaman dil İngilizceye düşer. README bu durumda `config.toml`'a `language` yazılmasını söyler. `config.toml` her komutta okunduğu için `remind --send` ve `capture` da bu ayara uyar. (Benim kararım: zincire `LANGUAGE` ya da `/etc/locale.conf` eklenmez.)
- Dil dosyaları ikiliye derlemede gömülür; `focusdeck plugin install` yine tek dosya indirir.

## Yaklaşımlar

1. **Elle yazılmış mesaj listesi.** Her mesaj Rust'ta bir enum satırı, her dil bir `match` kolu. Eksik çeviri derlenmez, bağımlılık gelmez. Çevirmen Rust okumak zorunda kaldığı için elendi.
2. **Fluent dosyaları (seçilen).** Çeviriler `.ftl` dosyalarında. Weblate gibi platformlar formatı doğrudan okur, çoğul kuralları dilin kendi kuralıyla çözülür (`1 overdue` / `3 overdue`, Türkçede tek biçim). Bedeli yeni bağımlılık ve eksik çevirinin çalışırken ortaya çıkması; bu ikincisi aşağıdaki denetimlerle kapatılır.
3. **İki sabit metin tablosu.** Derlemede denetlenir ama çevirmen yine Rust dosyası düzenler, parametreli mesajlar dağınık kalır. Elendi.

Kullanıcı topluluk çevirisini istediği için 2. yol seçildi. Format önerisi (Fluent) benim kararım.

## Kararlaştırılan nasıl

### Dil seçimi

- `config.toml`'a isteğe bağlı `language = "<kod>"` gelir. Değer `tr`, `en` ya da `pt-BR` gibi bir dil etiketi olabilir.
- Seçim sırası ve düşme kuralı Hedefler'deki gibidir. Etiketin tamamı için dosya yoksa yalnız dil kısmı denenir (`pt-BR` yoksa `pt`), o da yoksa `en`.
- Çekirdek her plugin sürecini başlatırken ona `FOCUSDECK_LANG` ortam değişkenini verir. Değer, çekirdeğin düşme kuralı uygulanmadan önceki istenen etikettir (`language` ya da ortamdan gelen, ör. `pt-BR`); düşme kuralını her bileşen kendi dosyalarına göre uygular. Böylece çekirdekte olmayan bir dili taşıyan plugin o dili gösterebilir. (Benim kararım.) Plugin dili sırasıyla `FOCUSDECK_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG` değerinden seçer; böylece `gmail login` gibi doğrudan çalıştırılan plugin komutları da dili bilir. Plugin'ler `config.toml` okumaz; doğrudan çalıştırılan plugin komutu kabuğun dilini kullanır. (Benim kararım.)

### Dosyalar ve paylaşılan kod

- Çekirdeğin dil dosyaları `crates/focusdeck/locales/<kod>.ftl`, her plugin'inki `plugins/<ad>/locales/<kod>.ftl`. Plugin'in kendi mesajları kendi dosyasında, başka plugin'e bağımlılık yok.
- Dil seçimini, Fluent dosyalarını yükleyip İngilizceye düşen mesaj aramasını ve "her dil dosyası `en.ftl` ile aynı kimlikleri taşır" testinin yardımcısını taşıyan yeni bir crate gelir: çekirdek reposunda `crates/focusdeck-i18n`. Çekirdek ve plugin'ler bunu `focusdeck-protocol` gibi git etiketiyle kullanır. Protokol crate'i dil işine karışmaz.
- `focusdeck-protocol`'deki tek Türkçe varsayılan metin (`drop_item` varsayılan hatası) İngilizceye çevrilir; protokol crate'i dil bilmez, bu metin geliştiriciye yöneliktir.

### Denetimler

- CI'da her repo için bir test: her `locales/*.ftl` dosyası `en.ftl` ile aynı mesaj kimliklerini ve aynı değişken adlarını taşır, fazlası ya da eksiği hata verir. Hata metni eksik kimliği ve dosyayı söyler.
- Kodda kullanılan mesaj kimliği `en.ftl`'de yoksa derleme ya da test takımı düşer. Mekanizma yapan kişinin seçimi.
- Bugün Türkçe metin bekleyen testler dili açıkça sabitler (ortamdan okumaz). Testler hangi dilde yazılırsa yazılsın, CI makinesinin `LANG` değeri sonucu değiştirmez.

### `capture` kelimeleri

- Her dil dosyası şu kelime listelerini taşır: yarın, haftanın yedi günü, dakika birimi, saat birimi. Liste Fluent mesajı olarak tutulur, kelimeler boşlukla ayrılır; dosyadaki yorum çevirmene biçimi anlatır.
- Etkin dilin kelimeleri ile kullanıcının eklediği kelimeler kabul edilir. Başka dilin kelimeleri kendiliğinden eklenmez; Türkçede `2s` 2 saat demek, İngilizcede `s` birim olarak tanınmaz, karışıklık çıkmaz.
- Yakalanamayan not kuyruğa yazılırken o anki dil etiketi de yazılır (yeni, isteğe bağlı `lang` alanı). Gönderimde not bu dille ayrıştırılır; alanı olmayan eski kayıtlar o anki dille ayrıştırılır. Kuyruk ile gönderim arasında `capture_words` değişirse yeni kelimeler geçerli olur. (Benim kararım.)
- Kullanıcı `config.toml`'da ekler:

  ```toml
  [capture_words]
  tomorrow = ["tmrw"]
  hour = ["sa"]
  mon = ["pazartesi"]
  ```

  Anahtarlar: `tomorrow`, `minute`, `hour`, `mon`, `tue`, `wed`, `thu`, `fri`, `sat`, `sun`. Eklenen kelimeler dil dosyasındakilerin üstüne gelir, onları silmez. Büyük/küçük harf farkı bugünkü gibi yok sayılır.
- `en` için kelimeler: `tomorrow`, `mon`…`sun` (tam adlar da: `monday`…), birimler `m`/`min` ve `h`. `tr` için bugünkü kelimelerin hepsi.

### Kapsamdaki metinler

- Çekirdek: TUI başlıkları ve sayaçları, alt satır ipuçları, `?` yardımı, açılır pencereler, kutudaki kırmızı hata satırları, CLI çıktıları ve hataları, kurulum mesajları, `--help` metinleri, `remind` çıktısı ve bildirim metinleri.
- Plugin açıklamaları (`Cargo.toml` `description`, oradan `index.toml`'a ve `focusdeck plugin list` çıktısına giden metin) yalnız İngilizce olur; dizin biçimi değişmez, eski çekirdekler dizini okumaya devam eder. (Benim kararım.)
- Plugin'ler: alt satırlar (`2g gecikmiş`, `4g`, `40dk` gibi), hata metinleri, `gmail login` çıktısı. `focusdeck-index` aracı geliştirici aracıdır, metinleri İngilizceye çevrilir ve çok dilli olmaz.
- Log dosyasına yazılan satırlar İngilizce olur ve çevrilmez; hata raporunda herkesin okuyabilmesi için. (Benim kararım.)

### Sürümler

- Çekirdek `v0.2.0` etiketiyle çıkar; `focusdeck-i18n` ve `focusdeck-protocol` bu etiketten kullanılır.
- Dört plugin `0.2.0` olur, yeni çekirdek etiketine bağlanır. Etiketler tek push'ta en fazla üç olacak şekilde gönderilir.

### README ve ekran görüntüleri

- Çekirdek ve plugins reposunda README.md İngilizce, README.tr.md Türkçe, en üstte dil bağlantısı. Plugins reposunda bu zaten yapıldı (`c960d84`).
- Ekran görüntüleri her dil için ayrı klasörde: `docs/screenshots/en/`, `docs/screenshots/tr/`. Dört görüntü: ana ekran, büyütülmüş kutu, taşıma penceresi, `?` yardımı.
- Görüntüler tam ekran terminalden (pencere bütün ekranı kaplar) alınır.
- Görüntüleri uygulama bittikten sonra kontrolcü kendisi çeker (ghostty, Hyprland, `grim`, tam ekran pencere); planda yapana verilen bir adım değildir. Eski `docs/screenshots/*.png` dosyaları silinir, README'ler yeni klasörleri gösterir.
- Görüntüleri yeniden üretmek için uydurma veri veren demo plugin'i ve iki dilin demo profilleri repoya girer (`docs/screenshots/demo/` altında), README'de değil bu klasördeki kısa bir notta nasıl çalıştırılacağı yazar. Kişisel bilgi, gerçek adres, makine adı görüntüye girmez. Demo iş temalı olmaz: profil adı "home" / "ev", kutular ve öğeler günlük hayattan (randevu, fatura, okunacak kitap, hediye gibi); iş yeri, müşteri, sunucu ya da iş takip sistemi çağrıştıran içerik yok. Görüntüler anonimdir: hiçbir kişi, şirket ya da kurum özel adı geçmez; gönderen ve kimden alanları "Eczane", "Kütüphane", "Ev sahibi" gibi genel rollerdir.

### Anonimleştirme

- İki reponun çalışma ağacında iş bilgisi kalmaz: test ve örneklerdeki iş alan adları, proje adı, müşteri adı, iş numarası, iş e-postası ve `docs/` altındaki Vikunja proje/görünüm/sütun kimlikleri nötr örneklerle (`example.com`, `Home`, `12345`, `1`, `2`, `3`) değiştirilir. Yeni yazılan test ve çevirilerde de gerçek kişi, şirket ya da iş verisi kullanılmaz.
- Git geçmişi yeniden yazılmaz; yalnız dosyalar düzeltilir. (Benim kararım: geçmişte sır yok, yalnız alan adları ve proje adları var; repo silinip yeniden açılmaz.)

## Yapana bırakılanlar

- Fluent'i hangi crate'lerle kullanacağı (`fluent-bundle` ile kendi yükleyicisi ya da `fluent-templates` gibi bir sarmalayıcı) ve dil etiketini nasıl ayrıştıracağı.
- Mesaj kimliklerinin adlandırması ve dosya içi düzeni.
- "Kodda olmayan kimlik" denetiminin mekanizması (build script, makro, test).
- `--help` metinlerinin clap'e nasıl verileceği.
- Demo plugin'inin dili ve biçimi.
