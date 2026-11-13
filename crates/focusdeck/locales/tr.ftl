# Quick-capture words: space-separated spellings the parser accepts, case-insensitive.
capture-words-tomorrow = yarın yarin
capture-words-minute = dk
capture-words-hour = s
capture-words-mon = pzt pts
capture-words-tue = sal
capture-words-wed = çar car
capture-words-thu = per prş prs
capture-words-fri = cum
capture-words-sat = cmt
capture-words-sun = paz

# Bottom hint line on the main screen: key and action, joined with " · ".
hint-board-note = a not al
hint-board-select = tıkla seç
hint-board-zoom = çift tıkla büyüt
hint-board-drag = sürükle taşı
hint-board-box = Tab kutu
hint-board-help = ? yardım
hint-board-quit = q çık
# Hints shared by the main screen and the zoomed box.
hint-open = o aç
hint-move = m taşı
hint-action-done = x bitti
hint-action-unstar = u yıldızı kaldır
hint-zoom-plan = boşluk planıma al
hint-zoom-search = / ara
hint-zoom-reorder = Shift+↑↓ sırala
hint-zoom-back = Esc geri
hint-capture = Enter kaydet · Esc vazgeç · @kişi #etiket R123 · sonda 14:30, yarın 09:00, 30dk
hint-picker = 1-9 hedef kutu · Esc vazgeç
hint-search = yaz ara · Enter tamam · Esc temizle
hint-help = Esc kapat

# Hint while an item is being dragged: what releasing the mouse does.
drag-into = bırak: { $title }
drag-reorder = bırak: { $row }. sıra
drag-rejected = { $title } kabul etmiyor
drag-nothing = bırakırsan bir şey olmaz
drag-cancel = { $what } · Esc vazgeç

# Key list opened with "?", one key per line, keys padded with two spaces.
help-title = Tuşlar
help-select = ↑↓, PageUp/PageDown, Home/End  öğe seç (sayfa tuşları: büyütülmüş kutu)
help-reorder = Shift+↑↓  öğeyi yukarı veya aşağı taşı (büyütülmüş kutu)
help-mouse = fare  tıkla seç · çift tıkla büyüt · sürükle taşı veya sırala
help-note = a  not al (capture_into kutusuna)
help-done = x  seçili öğe bitti
help-unstar = u  yıldızı kaldır
help-move = m  başka kutuya taşı
help-plan = boşluk  planıma al · ikinci boşluk siler (büyütülmüş kutu)
help-open = o  linki tarayıcıda aç
help-search = /  ara (büyütülmüş kutu)
help-next = Tab, Shift+Tab, oklar  kutu değiştir (↑↓ liste ucunda)
help-zoom = Enter  kutuyu büyüt · Esc geri
help-quit = q  çık

# Box title suffixes: item count, planned items over total, overdue items.
box-count = { $total }
box-planned = { $planned } / { $total }
box-overdue = { $count } gecikmiş
box-error = hata: { $message }
box-no-matches = "{ $query }" için eşleşme yok

picker-title = Nereye taşınsın
capture-title = Not al

status-refreshed = yenilendi { $time }
status-not-refreshed = yenilenmedi

# Messages flashed on the top line.
flash-action-failed = { $box }: { $what } başarısız: { $message }
flash-no-done = bu kutuda bitirme yok
flash-no-unstar = bu kutuda yıldız kaldırma yok
flash-no-link = bu öğenin linki yok
flash-drop-rejected = { $title } kutusu bırakılan öğe kabul etmiyor
flash-box-closed = { $title } kutusu açık değil
flash-box-busy = { $title } kutusu meşgul
flash-box-stopped = { $title } kutusunun plugin görevi durdu
flash-no-capture-into = profilde capture_into yok
flash-capture-failed = not eklenemedi: { $message }
flash-open-failed = xdg-open başlatılamadı: { $error }
flash-confirm-unplan = tekrar boşluk, "{ $title }" öğesini { $box } kutusundan siler
flash-no-move-target = hiçbir kutu taşınan öğeyi kabul etmiyor
flash-link-unsupported = bu tür link açılamaz: { $url }

terminal-raw-mode = terminal ham kipe alınamadı
terminal-alt-screen = alternatif ekrana geçilemedi
terminal-event = terminal olayı okunamadı

# Command line help: clap prints its own "Usage:", "Options:" and the help flag text.
cli-about = Odak panosu
cli-profile-help = Kullanılacak profil adı
cli-name-value = AD
cli-text-value = METİN
cli-capture-about = Metni profildeki capture_into kutusuna ekler
cli-remind-about = Yaklaşan işleri listeler
cli-remind-send-help = Bildirimi gönder
cli-plugin-about = Plugin kurar, listeler ve kaldırır
cli-plugin-install-about = Dizinden plugin kurar
cli-plugin-list-about = Kurulu plugin'leri listeler
cli-plugin-remove-about = Plugin'i kaldırır
cli-server-about = Panoyu kimlik doğrulamalı bir REST API ile sunar

cli-installed = { $name } { $version } kuruldu
cli-removed = { $name } kaldırıldı
cli-in-use-warning = uyarı: { $profile } profili bu plugin'i kullanıyor
cli-latest = (dizinde: { $version })
cli-not-in-registry = (dizinde: yok)
cli-registry-unreachable = dizine ulaşılamadı
cli-builtin = (yerleşik)

error-prefix = hata:
error-duration-invalid = geçersiz süre: { $value } (örnek: 90s, 2m, 1h)
error-duration-zero = süre sıfır olamaz: { $value }
error-plugin-not-installed = { $plugin } kurulu değil, focusdeck plugin install { $plugin }
error-secret-missing = { $path } bulunamadı
error-secret-exposed = { $path } dosyasını başkaları okuyabiliyor, chmod 600 yap
error-secret-unreadable = { $path } denetlenemedi: { $error }
error-token-too-short = { $path } dosyasındaki token 32 karakterden kısa
error-server-url-insecure = sunucu adresi https olmalı (düz http yalnız localhost için): { $url }
error-no-base-dir = { $var } ayarlı değil ve ev dizini bulunamadı

config-read-failed = { $path } okunamadı
config-invalid = { $path } geçersiz

profile-read-failed = { $path } okunamadı: { $error }
profile-parse-failed = { $path } geçersiz: { $error }
profile-duplicate-box-id = aynı kutu kimliği iki kez: { $id }
profile-unknown-capture-into = capture_into bilinmeyen kutuya bakıyor: { $id }
profile-plan-into-self = { $id } kutusu kendine plan_into veremez
profile-unknown-plan-into = plan_into bilinmeyen kutuya bakıyor: { $id }
profile-mixed-servers = { $plugin } kutuları farklı sunuculara bakıyor: { $first }, { $second }
profile-empty-box = { $id } kutusunun width ve height değeri en az 1 olmalı
profile-overlapping-cells = { $first } ve { $second } kutuları aynı hücrede: satır { $row } sütun { $col }
profile-no-profile = profil seçilmedi: --profile ver ya da config.toml'a default_profile yaz
profile-invalid-name = geçersiz profil adı: { $name }
profile-create-failed = { $path } oluşturulamadı: { $error }

install-invalid-name = geçersiz plugin adı: { $name }
install-checksum-mismatch = sağlama tutmadı
install-checksum-mismatch-named = sağlama tutmadı: { $name }
install-file-url-relative = file:// adresi mutlak yol olmalı: { $url }
install-read-failed = { $path } okunamadı
install-unsupported-url = desteklenmeyen adres (yalnız http, https, file): { $url }
install-file-too-large = dosya çok büyük
install-too-many-redirects = çok fazla yönlendirme
install-insecure-url = düz http reddedildi (https kullan): { $url }
install-http-client = HTTP istemcisi kurulamadı
install-download-failed = { $url } indirilemedi
install-http-status = { $url } indirilemedi: HTTP { $status }
install-url-too-large = { $url } çok büyük
install-index-not-utf8 = dizin UTF-8 değil
install-index-invalid = dizin geçersiz
install-not-found = dizinlerde böyle bir plugin yok: { $name }
install-not-found-unreachable = dizinlerde böyle bir plugin yok: { $name } (ulaşılamayan dizinler: { $failures })
install-protocol-mismatch = { $name } protokol { $wanted } istiyor, çekirdek { $known } biliyor
install-no-target = { $name } için bu makineye uygun sürüm yok ({ $target })
install-dir-create-failed = { $path } oluşturulamadı
install-replace-failed = plugin yerine konamadı
install-write-failed = plugin yazılamadı
install-sync-failed = plugin diske işlenemedi
install-chmod-failed = plugin çalıştırılabilir yapılamadı
install-manifest-failed = manifest yazılamadı
install-delete-failed = { $path } silinemedi
install-not-installed = plugin kurulu değil: { $name }
install-local-builtin = local focusdeck ile birlikte gelir, kurulacak bir şey yok
install-local-not-removable = local focusdeck ile birlikte gelir, kaldırılamaz
install-remote-file-target = { $registry } adresindeki dizin { $name } için yerel bir dosya gösteriyor
install-binary-changed = { $name } artık manifestiyle uyuşmuyor; "focusdeck plugin install { $name }" komutunu yeniden çalıştır

# Notification titles and bodies of "focusdeck capture" and the rofi prompt label.
capture-prompt = Not
capture-added-title = eklendi
capture-failed-title = focusdeck: eklenmedi
capture-queued-title = focusdeck: kuyruğa alındı
capture-lost-title = focusdeck: not kaybolabilir
capture-queue-flushed = kuyruktan { $count } not gönderildi
capture-no-capture-into = bu profilde capture_into yok
capture-empty-title = başlık boş
capture-command-empty = capture_command boş
capture-prompt-spawn-failed = { $program } başlatılamadı: { $error }
capture-no-box = { $target } kutusu yok
capture-interrupted = not gönderilmeden kesildi

remind-write-failed = { $path } yazılamadı

host-spawn-failed = plugin başlatılamadı: { $error }
host-timeout = plugin 15 sn içinde cevap vermedi
host-crashed = plugin kapandı
host-bad-line = plugin geçersiz satır yazdı: { $line }
host-protocol-mismatch = protokol uyuşmuyor: çekirdek { $expected }, plugin { $got }
host-invalid-result = geçersiz sonuç: { $error }
host-invalid-describe = geçersiz describe: { $error }
host-non-utf8 = UTF-8 olmayan çıktı
host-line-too-long = plugin 16 MiB'tan uzun bir satır yazdı

local-file-corrupt = { $path } okunamadı ({ $error }); focusdeck bu dosyaya yazmayacak
local-file-newer = { $path } daha yeni bir focusdeck ile yazılmış (biçim { $version }); focusdeck'i güncelle
local-file-io = { $path }: { $error }
local-invalid-settings = yerel kutu ayarı geçersiz: { $error }
local-item-missing = { $id } notu bu kutuda yok
local-list-required = `list` boş olmayan bir metin olmalı
local-position-required = sıra numarası gerekli
local-unknown-action = bilinmeyen eylem: { $action }

starter-title = Notlar
starter-tmp-left = geçici dosya silinemedi { $path }: { $error }
server-no-profiles = { $dir } içinde profil bulunamadı
board-not-found = bulunamadı
board-unsupported-action = { $box } kutusu { $action } eylemini desteklemiyor
board-no-drops = { $box } kutusu bırakma kabul etmiyor
board-no-capture-into = profilde capture_into kutusu yok
board-secret-missing = bu kutunun bir gizli dosyası yok
board-secret-exposed = bu kutunun bir gizli dosyası başkaları tarafından okunabiliyor
board-secret-unreadable = bu kutunun bir gizli dosyası denetlenemedi
board-secret-token-short = bu kutunun token'ı çok kısa
server-no-section = config.toml içinde [serve] bölümü yok
server-listening = { $addr } adresinde dinleniyor
server-dropped-in-progress = yarım kalan { $count } idempotency anahtarı silindi
server-save-failed = idempotency deposu güncellenemedi: { $error }
server-unauthorized = token eksik ya da yanlış
server-invalid-body = istek gövdesi geçersiz: { $error }
server-body-too-large = istek gövdesi 1 MiB'tan büyük
server-bad-key = Idempotency-Key başlığı geçersiz
server-bad-since = since parametresi RFC 3339 zamanı olmalı
server-key-reused = bu Idempotency-Key başka bir gövdeyle kullanılmıştı
server-in-progress = bu Idempotency-Key ile gelen ilk istek hâlâ sürüyor
server-internal = sunucu idempotency deposunu kullanamadı
