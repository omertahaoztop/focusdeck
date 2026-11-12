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
help-note = a  not al (capture_into kutusuna)
help-done = x  seçili öğe bitti
help-unstar = u  yıldızı kaldır
help-move = m  başka kutuya taşı
help-plan = boşluk  planıma al / çıkar (büyütülmüş kutu)
help-open = o  linki tarayıcıda aç
help-search = /  ara (büyütülmüş kutu)
help-next = Tab  sonraki kutu
help-zoom = Enter  kutuyu büyüt · Esc geri
help-quit = q  çık

# Box title suffixes: item count, planned items over total, overdue items.
box-count = { $total }
box-planned = { $planned } / { $total }
box-overdue = { $count } gecikmiş
box-error = hata: { $message }

picker-title = Nereye taşınsın
capture-title = Not al

status-refreshed = yenilendi { $time }
status-not-refreshed = yenilenmedi

# Messages flashed on the top line.
flash-action-failed = { $what } başarısız: { $message }
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

terminal-raw-mode = terminal ham kipe alınamadı
terminal-alt-screen = alternatif ekrana geçilemedi
terminal-event = terminal olayı okunamadı
