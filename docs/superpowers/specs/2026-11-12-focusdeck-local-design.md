# focusdeck yerel kayıt tasarımı

Tarih: 2026-11-12 · Durum: onaylandı

## Amaç

focusdeck'te not tutabilen tek kutu türü Vikunja. Vikunja kullanmayan biri not alamıyor, Redmine işini planına ekleyemiyor, ilk açılışta da "profil seçilmedi" hatasıyla karşılaşıyor. focusdeck herkese açık olduğu için bu insanların çoğu ilk dakikada programı bırakır.

İstenen: focusdeck'i ilk kez kuran biri hiçbir ayar yapmadan not alabilsin. Notlar kullanıcının kendi bilgisayarında saklansın, programı kapatıp açınca, yeniden kurunca kaybolmasın.

Bu iş, sonra gelecek "panoyu arayüzden bölme" işinin temeli: arayüzden açılan yeni bir kutunun ayar istemeden çalışabilmesi yerel bir kayıt yerine bağlı.

## Hedefler

- `local` adında, focusdeck programının içinde gelen bir kutu türü. Kurulum gerekmez, ağ gerekmez, hesap gerekmez.
- Vikunja kutusunun yaptığı her şey `local` kutuda da çalışır: listeleme, `x` bitti, plandan çıkarma, kutular arası taşıma (`m` ve sürükle-bırak), kutu içinde sıralama, başka kutudan bırakılanı kabul etme (Zulip mesajı, mail, Redmine işi), hatırlatıcı.
- `focusdeck capture` ve `a` ile alınan not `local` kutuya gider. `capture` sözdizimindeki tarih, etiket, `kimden` ve link alanları korunur.
- Redmine kutusunun `plan_into` hedefi bir `local` kutu olabilir. Redmine'da iş kapanınca bağlı yerel görev bitti sayılır (bugünkü bağ mantığı aynen).
- İlk açılış: hiç profil yoksa focusdeck tek bir "Notlar" kutusu olan başlangıç profilini kendisi oluşturur ve açar.

## Hedef olmayanlar

- Makineler arası senkron, yedekleme, şifreleme.
- Bitmiş notları gösteren ekran. Bitmiş not dosyada kalır ama arayüzde görünmez.
- Vikunja'dan yerele ya da yerelden Vikunja'ya aktarma.
- Panoyu arayüzden bölme, kutu ekleme/silme. Ayrı tasarım, bu işten sonra.

## Olmaması gerekenler

- Not kaybı. TUI açıkken kısayoldan `focusdeck capture` çalışması, iki kutunun aynı anda yazması ya da yazma sırasında elektrik kesilmesi bir notu silmemeli.
- `x` ile bitirilen notun geri dönülmez biçimde silinmesi.
- Bugün Vikunja kullanan birinin profilinde bir şeyin değişmesi.

## Yaklaşımlar

1. **focusdeck kendi plugin'i olur (seçilen).** Yerel kayıt kodu `focusdeck` programının içine girer. Profilde `plugin = "local"` yazan kutu için core, kurulu plugin dosyası yerine kendi programını gizli bir alt komutla başlatır. Kutu öteki plugin'lerle aynı JSON protokolüyle konuşur, böylece arayüz, not alma, plan bağı ve hatırlatıcı değişmeden çalışır. Core'da yalnız plugin adını programa çözme değişir.
2. **Kutu süreç içinde çalışır.** Ayrı süreç yok. Kutuları çalıştıran katmanın (`PluginHost`, worker) bir arayüz arkasına alınması gerekir. Daha büyük değişiklik, bozma riski yüksek, kazancı yalnız biraz daha hızlı açılış.
3. **İlk açılışta plugin indirilir.** Core'a neredeyse dokunulmaz ama ilk açılış ağa bağlı olur ve indirme başarısızsa not alınamaz. "Hiç ayar yok" hedefiyle çelişir.

Ömer 1'i seçti: istenen davranışı veriyor, mevcut protokolü olduğu gibi kullanıyor, tek program dosyası kalıyor.

## Tasarım

### Programa çözme

- Profilde `plugin = "local"` olan kutu için `build_host` plugin dizinine bakmaz. `std::env::current_exe()` yolunu ve gizli `serve-local` alt komutunu kullanır. `PluginHost` program yoluna ek olarak argüman listesi alır.
- `serve-local` `--help` çıktısında görünmez. stdin/stdout üzerinden `focusdeck_protocol::serve` döngüsünü çalıştırır.
- `local` adı ayrılmıştır: `focusdeck plugin install local` "local focusdeck ile birlikte gelir" diyerek reddeder, `plugin remove local` aynı şekilde. `plugin list` kurulu plugin'lerin altında `local (yerleşik)` satırı gösterir.
- `remind` komutu bugün plugin dosyası yoksa kutuyu atlıyor. `local` kutu bu kontrole takılmaz.

### Kayıt dosyası

- Tek dosya: `~/.local/share/focusdeck/local.json` (`FOCUSDECK_DATA_DIR` ile değişir). Bütün profiller ve kutular aynı dosyayı paylaşır.
- Her `local` kutunun ayarında isteğe bağlı `list` anahtarı olur. Verilmezse kutunun `id`'si kullanılır. Not, `list` alanı kutununkiyle eşleşen kutuda görünür. İki profil aynı `list` adını kullanırsa aynı notları görür. Bu bilinçli: "İş" ve "Ev" profilleri ortak bir "Fikirler" listesi tutabilir.
- Biçim: `{"version": 1, "items": [...]}`. Her öğe yalnız `local`'a ait bir kayıt türüdür, protokoldeki `Item` değişmez (öteki plugin'lerin konuştuğu biçime dokunulmaz). Kayıt alanları: `id`, `list`, `title`, `description`, `due`, `labels`, `from`, `links`, `source`, `position`, `created`, `done_at`. Listelerken bunlardan `Item` üretilir; `subtitle` ve `url` boş kalır, `overdue` `due`'dan hesaplanır. Bilinmeyen alanlar `#[serde(flatten)]` bir ek alan haritasında tutulup yazarken korunur, ileri sürümle açılan dosya bozulmasın diye. *(Benim kararım.)*
- `id`: zaman damgası, süreç kimliği ve sayaçtan üretilen, dosya içinde benzersiz bir metin. Yeni bağımlılık eklenmez. Kilit std `File::lock` ile yapıldığı için workspace `Cargo.toml`'a `rust-version = "1.89"` yazılır. *(Benim kararım.)*

### Eşzamanlılık ve dayanıklılık

- Her okuma ve yazma `local.json.lock` dosyasında `std::fs::File::lock` (yazma) ya da `lock_shared` (okuma) ile kilit alır. TUI'deki birden çok `local` kutu, `capture` ve `remind` aynı anda çalışabilir.
- Yazma: kilit altında dosyayı yeniden oku, değişikliği uygula, `local.json.tmp`'ye yaz, `fsync`, `rename`. Her değişiklik dosyanın son hâli üzerinde yapılır. Bellekteki eski kopya üstüne yazılmaz.
- Dosya bozuksa (JSON okunamıyorsa) kutu hata satırı gösterir ve hiçbir şey yazmaz. Bozuk dosyanın üstüne yazmak not kaybı demek.

### Eylemler

- `list`: kutunun `list`'indeki, `done_at` boş öğeler, `position` sırasıyla. `due` geçmişse `overdue`.
- `done`: `done_at` doldurulur, öğe dosyada kalır.
- `delete`: öğe dosyadan çıkar. Arayüzde yalnız plandan çıkarma bunu çağırır.
- `reorder`: arayüz `{"position": n}` gönderir; `n` listenin görünen (bitmemiş) öğeleri arasındaki sıra numarasıdır. Saklanan `position` bir `f64` sıralama anahtarıdır: yeni değer komşu iki öğenin ortası olur (Vikunja'daki `place_between` gibi), ötekiler yeniden numaralanmaz. `done` ve `delete` başka öğenin `position`'ını değiştirmez. *(Benim kararım.)*
- `drop_item`: `from.plugin == "local"` ve öğe dosyada varsa öğe silinip yeniden yaratılmaz, `list` alanı hedef kutununkine çevrilir (taşıma). Yoksa yeni öğe yaratılır, `source` bağı korunur. İki durumda da öğe hedef listenin sonuna girer, çünkü `drop` satır numarası taşımaz. *(Benim kararım.)*
- `reminders`: `due` alanı olan ve bitmemiş öğeler, `due` anında hatırlatılır (Vikunja'da `capture` tarihinin hatırlatıcı olması gibi).
- `describe`: `actions = ["done", "delete", "move", "reorder"]`, `accepts_drop = true`, `reminders = true`.

### İlk açılış

- Bugün: profil seçilmediyse hata. Yeni kural, sırasıyla:
  1. `--profile`, `FOCUSDECK_PROFILE` ya da `default_profile` verilmişse o profil (bugünkü gibi).
  2. Hiçbiri yoksa ve `profiles/` altında tek profil varsa o açılır.
  3. Hiç profil yoksa `profiles/notes.toml` oluşturulur ve açılır.
  4. Birden çok profil varsa ve seçim yoksa bugünkü hata.
- Başlangıç profili: tek kutu, `id = "notes"`, başlık etkin dilde ("Notlar" / "Notes"), `plugin = "local"`, `capture_into = "notes"`, tam ekran. `config.toml` yazılmaz.
- `capture` ve `remind` aynı seçim kuralını kullanır. Kısayola bağlanan `focusdeck capture` ilk kez çalıştığında da profil oluşur.

### Dil

Yeni metinler (`local (yerleşik)`, install reddi, başlangıç kutu başlığı, kayıt hataları) `en.ftl` ve `tr.ftl`'ye girer, parite testi kapsar.

### Belgeler

README ve README.tr.md: "Hızlı başlangıç" kurulumdan sonra doğrudan `focusdeck` ile başlar; `local` kutu türü ve `list` anahtarı profil bölümüne, kayıt dosyasının yeri ve yedeği için tek paragraf.

## Kurucuya kalanlar

- Hata metinlerinin tam ifadesi, modül ve fonksiyon adları.
- `serve-local`'ın gizlenme biçimi (clap `hide`).
- Araya yerleştirmenin kenar durumları (listenin başı/sonu, boş liste).
- Test düzeni: kayıt mantığı birim testleri, kilitli eşzamanlı yazma testi, `serve-local`'ı gerçek süreç olarak başlatan bir uçtan uca test.
