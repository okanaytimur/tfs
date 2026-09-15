<img src="assets/tfs.png" width="88" align="right" alt="tfs ikonu">

# tfs (terminal-file-send) — SSH dosya tarayıcısı + modern SSH terminali

<!-- Demo GIF'i buraya: assets/demo.gif olarak koyup alttaki satırın yorumunu kaldır.
![tfs demo](assets/demo.gif)
-->

*🇬🇧 [English summary](#english) · `cargo install tfs-ssh`*

ratatui + russh + russh-sftp ile:
- **F2** — iki panelli (YEREL ↔ UZAK) SFTP dosya **ve klasör** transferi:
  `t` ile ya da fareyle sürükle-bırak,
- **F1** — PuTTY / Windows Terminal benzeri, tam ekran interaktif SSH terminali,
- **F4** — seçili dosyayı [`fresh`](https://github.com/sinelaw/fresh) editöründe aç
  (uzak dosya: indir → düzenle → otomatik geri yükle),

hepsi tek SSH bağlantısı üzerinden (yeniden bağlanma yok).

## Kurulum

### Cargo ile (Rust kuruluysa)

```sh
cargo install tfs-ssh
```

Kaynaktan derlemek istemiyorsanız [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall)
hazır binary'yi Releases'ten indirir (saniyeler sürer):

```sh
cargo binstall tfs-ssh
```

> crates.io'da `tfs` adı başkasına ait olduğu için paket adı **`tfs-ssh`**;
> kurulan komut yine **`tfs`**'tir.

### Hazır sürümler (derlemeye gerek yok)

[**Releases**](https://github.com/okanaytimur/tfs/releases) sayfasından hazır
binary indirebilirsiniz:

| Dosya | Platform | Not |
|-------|----------|-----|
| `tfs-vX-linux-x86_64` | Linux x86_64 | glibc 2.34+ · `chmod +x` · **yalnızca v0.4.0'da** (aşağı bak) |
| `tfs-vX-windows-x86_64.exe` | Windows 10+ 64-bit | Önerilen |
| `tfs-vX-windows-i686.exe` | Windows 10+ 32-bit | Eski/32-bit Windows |

İndirdikten sonra yanına bir `config.json` koyup çalıştırın (bkz. Yapılandırma).

> **Linux binary'si** en son v0.4.0 release'inde var; v0.5.0 ve v0.6.0 yalnızca
> Windows binary'siyle çıktı (geliştirme makinasında Linux hedefi kurulu değil).
> Linux'ta güncel sürüm için `cargo install tfs-ssh` kullanın — kaynaktan
> derler, birkaç dakika sürer. `cargo binstall` da Linux'ta kaynağa düşer.

**Platform desteği**: Linux (glibc 2.34+ — Ubuntu 22.04+, Debian 12+, RHEL 9+)
ve Windows 10 ve üzeri. Windows 7/8 desteklenmez — Rust 1.78'den beri standart
Windows hedefi Win7'yi bırakmıştır (binary'ler Win10+ ister). Win7 ancak Tier-3
`*-win7-windows-msvc` hedefi + nightly + `-Z build-std` ile derlenebilir
(deneysel, kripto/async yığınımızla test edilmedi).

Linux'ta pano (kopyala/yapıştır) X11 ve Wayland'da çalışır, ek sistem
kütüphanesi gerekmez; binary yalnızca `libc`/`libm`/`libgcc_s`'e bağlıdır.
Kaynaktan derlemek için bir C linker gerekir (`dnf install gcc` /
`apt install build-essential`).

## Yapılandırma

**İlk çalıştırmada bir şey hazırlamanıza gerek yok** ve dosyayı elle açmanız da
gerekmiyor: `tfs` açılışta **bağlantı yöneticisiyle** gelir, ilk bağlantınızı
oradan kurarsınız (bkz. [Bağlantı yöneticisi](#bağlantı-yöneticisi-açılış-ekranı)).
Yaptığınız her değişiklik anında `config.json`'a yazılır.

Dosya bulunduğunuz dizine oluşturulur; başka bir yol vermek için argüman
kullanın (`tfs sunucular.json` — gerekiyorsa alt dizinler de açılır).

Dosyayı elle düzenlemeyi tercih ederseniz biçimi şu — parola ya da
**SSH anahtarı** ile:

```json
{
  "servers": [
    { "name": "Prod", "host": "1.2.3.4", "port": 22, "user": "okan", "password": "***" },
    { "name": "Anahtarla", "host": "sunucu.example.com", "user": "okan",
      "key": "~/.ssh/id_ed25519" },
    { "name": "Şifreli anahtar", "host": "test.local", "user": "root",
      "key": "~/.ssh/id_rsa", "key_passphrase": "***" }
  ]
}
```

| Alan | Zorunlu | Açıklama |
|------|---------|----------|
| `name` `host` `user` | evet | — |
| `port` | hayır | varsayılan 22 |
| `password` | hayır | anahtarla bağlanıyorsanız hiç yazmayın |
| `key` | hayır | özel anahtar dosyası; `~` açılır |
| `key_passphrase` | hayır | anahtar parolayla şifreliyse |

**Kimlik doğrulama sırası** OpenSSH'inkiyle aynı — önce publickey, sonra parola:

1. `key` doluysa o anahtar denenir.
2. `key` boşsa `~/.ssh/id_ed25519` → `id_ecdsa` → `id_rsa` (var olanlar).
3. Hâlâ olmadıysa ve `password` doluysa parola.

Hiçbiri tutmazsa hata **hangi yöntemin neden düştüğünü** listeler (anahtar
okunamadı / sunucu kabul etmedi / parola reddedildi).

- Şablonu elle de kopyalayabilirsiniz: `config.example.json` → `config.json`.
  (Dosya hiç düzenlenmemişse — yani birebir şablonsa — `tfs` şablondaki uydurma
  sunucuları listelemez; yönetici boş açılır, ilk kaydınızda üzerine yazılır.)
- Yönetici dosyayı **atomik** yazar (önce `config.json.tmp`, sonra taşır), yani
  yarıda kesilen bir yazma elinizdeki config'i bozmaz. Unix'te dosya `0600`'e
  çekilir.
- **Güvenlik**: parolalar düz metin tutulur; `config.json`'ı repoya koymayın
  (`.gitignore`'a ekli). Parola yerine anahtar kullanmak daha güvenlidir —
  `key` verip `password` alanını hiç yazmayabilirsiniz.

## Sunucu anahtarı doğrulaması (known_hosts)

tfs bağlanmadan önce sunucunun anahtarını `~/.ssh/known_hosts` ile doğrular —
OpenSSH ile **aynı dosyayı** kullanır, yani `ssh` ile kaydettiğiniz sunucular
sessizce tanınır.

**İlk bağlantıda** parmak izi gösterilir ve onayınız istenir:

```
┌ Bilinmeyen sunucu anahtarı ──────────────────────────────────┐
│ 'Prod' (1.2.3.4:22) ilk kez bağlanıyorsunuz.                 │
│                                                              │
│   Anahtar türü : ssh-ed25519                                 │
│   Parmak izi   : SHA256:4vQ…8Zk                              │
│                                                              │
│ Bu parmak izini sunucudan bağımsız bir yolla doğrulayın:     │
│   ssh-keyscan -p PORT HOST | ssh-keygen -lf -                │
│                                                              │
│  E / Enter  kabul et ve bağlan     H / Esc  vazgeç           │
└──────────────────────────────────────────────────────────────┘
```

Kabul ederseniz anahtar `known_hosts`a eklenir ve bir daha sorulmaz.

**Anahtar değiştiyse** bağlantı reddedilir ve uyarı gösterilir. Burada bilerek
bir "kabul et" tuşu **yoktur**: anahtar değişikliği ortadaki-adam saldırısının
imzasıdır, tek tuşla geçilebilir olsaydı korumanın anlamı kalmazdı. Sunucuyu
yeniden kurduğunuzdan eminseniz kaydı elle silin:

```sh
ssh-keygen -R '[sunucu.example.com]:2244'
```

## Çalıştırma

```powershell
cargo run                    # ./config.json okur
cargo run -- sunucular.json  # farklı bir config yolu
```

Açılışta sunucu listesi gelir → **fareyle tıkla = bağlan** (ya da ↑/↓ + Enter).

## İki mod: F1 Terminal · F2 Dosya

Bağlandıktan sonra aynı SSH bağlantısı üzerinde iki mod arasında geçiş yapılır
(yeniden bağlanma yok — tek bağlantı, iki kanal):

- **F1 → SSH Terminali**: PuTTY / Windows Terminal benzeri, tam ekran interaktif
  kabuk. ANSI/VT100 renkleri, imleç, `top`/`htop`/`vim` gibi tam ekran programlar
  çalışır. İlk F1'de kabuk açılır ve uygulama boyunca canlı kalır.
- **F2 → Dosya transferi**: iki panelli, fareyle sürükle-bırak SFTP tarayıcısı.

`F1`/`F2` **uygulama-global** kısayoldur; terminal modundayken kabuğa
iletilmezler (diğer tüm tuşlar kabuğa gider). Terminalden çıkmak için kabukta
`exit` yaz (dosya moduna döner) ya da `F2` → `q`.

### Terminal modunda kopyala / yapıştır / kaydır

| İşlem | Nasıl |
|-------|-------|
| **Kopyala** | Fareyle metnin üzerinden sürükle; tuşu bırakınca seçim panoya kopyalanır (üst çubukta `✓ N karakter kopyalandı`). |
| **Yapıştır** | **Sağ tık**, **Ctrl+V**, **Shift+Insert**, **Ctrl+Shift+V** ya da terminalinizin kendi Yapıştır menüsü — hepsi metni **tek seferde** gönderir. |
| **Geçmişe kaydır** | **Fare tekerleği** ya da **Shift+PgUp / Shift+PgDn**. Tampon 1000 satır; bir tuşa basınca canlı ekrana döner. |

Kaydırma sırasında üst çubukta kaç satır geride olduğunuz gösterilir ve imleç
gizlenir. Yapıştırılan metnin satır sonları `\r`'a çevrilir; uzak taraf bracketed
paste modundaysa (`vim`, `bash`) metin `ESC[200~`/`ESC[201~` ile sarılır — böylece
çok satırlı yapıştırma yanlışlıkla komut olarak çalışmaz ve `vim`'de otomatik
girinti metni bozmaz.

<details><summary>Windows'ta yapıştırma neden özel ele alınıyor?</summary>

crossterm'in `Event::Paste` olayı **Windows'ta hiç üretilmez** — eski konsol
API'sinde bracketed paste yok. Windows Terminal / conhost, `Ctrl+V` gibi
kısayolları kendisi yakalayıp panodaki metni uygulamaya **tek tek tuş olayı**
olarak enjekte eder. Bu yüzden yapıştırma harf harf gidiyordu: her karakter ayrı
bir SSH paketi (yavaş) ve `vim` bunu yazım sanıp otomatik girinti uyguluyordu.

Çözüm: bir tuş **hemen gönderilir** (yazmaya gecikme eklenmez), ardından
"arkasından devamı geliyor mu?" diye kısa bir süre (25 ms) dinlenir. Geliyorsa
bu bir yapıştırmadır; kalan karakterler toplanıp tek parça gönderilir. İnsan
yazımında tuşlar arası boşluk ≥60 ms, tuş tekrarında bile ≥32 ms olduğundan
sıradan yazma yığına dönüşmez. Ayrıca yalnızca **çok satırlı** yığın bracketed
paste ile sarılır — tek satırlıkta sarmanın bir faydası yok ve `vim` normal
modunda tuş tekrarı yanlışlıkla yığın sanılırsa zarar verebilirdi.

Sağ tık bu yolu hiç kullanmaz: panoyu doğrudan okur.

> Teşhis: `TFS_KEYLOG=tuslar.txt` ile çalıştırırsanız her girdi olayı, bir
> öncekinden kaç mikrosaniye sonra geldiğiyle birlikte kaydedilir — yapıştırmanın
> nasıl teslim edildiğini ölçmek için.

</details>

**Terminal sorgu yanıtları**: Emülatör, kabuğun/promptun gönderdiği imleç-konumu
(CPR, `ESC[6n`) ve cihaz-kimliği (DA) sorgularına yanıt verir; aksi halde bazı
promptlar ~1 sn bekleyip ekranı sıfırlıyordu.

> Teşhis: `TFS_LOG=yol.txt` ortam değişkeniyle çalıştırırsanız sunucudan gelen ham
> baytlar escape'lenmiş olarak o dosyaya yazılır (terminal sorunlarını incelemek için).

## F4 — dosyayı `fresh` ile düzenle

Dosya modunda (F2) bir dosya seçip **F4** (ya da **`e`**) tuşuna basınca dosya
[`fresh`](https://github.com/sinelaw/fresh) editöründe açılır — VS Code / Sublime
alışkanlıklarını terminale getiren, çoklu imleç ve LSP destekli bir editör.

| Panel | Ne olur |
|-------|---------|
| **YEREL** | Dosya olduğu yerde açılır. |
| **UZAK** | Dosya geçici bir dizine indirilir, editörde açılır; editör kapanınca **içeriği değiştiyse** SFTP ile geri yüklenir. Değişmediyse hiçbir şey yüklenmez. |

Uzak sunucuda `fresh` kurulu olmasına **gerek yoktur** — düzenleme her zaman
yerelde yapılır. Değişiklik tespiti içerik özetiyle (FNV-1a) yapılır, dosya
zaman damgasıyla değil: editör dosyayı açıp kaydetmeden kapansa da, aynı içeriği
tekrar yazsa da gereksiz yükleme olmaz. Uzak dosya sınırı **64 MiB**'tır (bu
sınır transferi içindir; `fresh` çok daha büyük dosyaları açabilir).

Editör açılırken tfs'in kendi arayüzü askıya alınır (ham mod kapatılır,
alternatif ekrandan çıkılır ve klavye olay akışı bırakılır — aksi halde tuşlar
editöre değil tfs'e giderdi); editör kapanınca arayüz geri gelir.

### `fresh` kurulu değilse

tfs onu sizin için kurmayı önerir:

```
┌ Editör kurulumu ─────────────────────────────────┐
│ fresh editörü bulunamadı.                        │
│                                                  │
│ Şimdi kurulsun mu? Çalıştırılacak:               │
│   $ cargo binstall --no-confirm fresh-editor     │
│                                                  │
│  E / Enter  kur     H / Esc  vazgeç              │
└──────────────────────────────────────────────────┘
```

`cargo binstall` hazır derlenmiş binary'yi indirir (saniyeler). Sırayla denenir:

1. `cargo binstall --no-confirm fresh-editor` — `cargo-binstall` kuruluysa.
2. Değilse önce `cargo install cargo-binstall`, sonra (1).
3. O da olmazsa son çare `cargo install --locked fresh-editor` (kaynaktan
   derler, uzun sürer).

Kurulum çıktısı doğrudan terminalde akar. Kurulu bir `fresh`i tfs, `PATH`'e ek
olarak `~/.cargo/bin` ve `~/.local/bin` altında da arar — böylece kurulumdan
hemen sonra, kabuk yeniden başlatılmadan bulunur.

Editörü elle kurmak isterseniz:

```sh
cargo binstall fresh-editor      # hazır binary
cargo install --locked fresh-editor   # kaynaktan
```

## Kullanım

- **F5**: Odaklı paneldeki seçili **dosya ya da klasörü** karşı
  panele aktarır — karşı panelin o anki dizinine, aynı adla. (YEREL odaklıysa
  upload, UZAK odaklıysa download.) Fare kullanmadan transfer.
- **Sürükle-bırak**: Bir dosyayı/klasörü bir panelden diğerine fareyle sürükleyip
  bırak → yükleme/indirme başlar. (YEREL→UZAK = upload, UZAK→YEREL = download.)
- **Tek tık**: dosya seçer; klasöre tıklamak içine girer.
- **Tekerlek**: seçimi kaydırır.
- **Klavye**: `Tab` panel değiştir, `Enter` gir, `Backspace` üst dizin,
  `↑/↓` `PgUp/PgDn` `Home/End` gezin, `F5` transfer, `F4` düzenle,
  `Esc`/`F10`/`Ctrl+Q` çıkış. **Yazmaya başlamak arama yapar** (aşağıya bakın).
- **F1**: SSH terminaline geç · **F2**: dosya moduna dön. Üst çubuktaki
  `F1 Terminal` / `F2 Dosya` sekmelerine **fareyle de tıklanabilir**.
- **F4**: seçili dosyayı `fresh` editöründe aç (bkz. yukarıdaki bölüm).

## Bağlantı yöneticisi (açılış ekranı)

Açılışta `config.json`'daki bağlantılar listelenir ve **aynı ekranda** düzenlenir
— dosyayı elle açmanız gerekmez. Her değişiklik anında kaydedilir.

```
┌ SSH Bağlantıları (3) ─────────────────┬ Ayrıntı ───────────────┐
│ ▶ Prod Sunucu   okan@1.2.3.4:22       │ Ad             Prod... │
│   Yedek         root@10.0.0.7:2222    │ Sunucu         1.2.3.4 │
│   Test          okan@test.local:22    │ Kullanıcı      okan    │
│                                       │                        │
│                                       │ Kimlik doğrulama       │
│                                       │   Anahtar      varsa.. │
│                                       │   Parola       •••••   │
└───────────────────────────────────────┴────────────────────────┘
 [ + Yeni (F5) ] [ Düzenle (F4) ] [ Kopyala (F6) ] [ Sil (F8) ] [ ↑ ] [ ↓ ]
 Enter / tıkla: bağlan · yaz: ara · Esc: çık · config.json
```

| İşlem | Fare | Klavye |
|-------|------|--------|
| Bağlan | satıra **tek tık** | `Enter` |
| Yeni bağlantı | `[+ Yeni]` | `F5` · `Insert` |
| Düzenle | `[Düzenle]` | `F4` |
| Kopyala (çoğalt) | `[Kopyala]` | `F6` |
| Sil | `[Sil]` | `F8` · `Delete` |
| Sırala | `[↑]` `[↓]` | `Alt+↑` · `Alt+↓` |
| Ara | — | doğrudan yazın |
| Çık | — | `Esc` · `Ctrl+Q` · `F10` |

Arama ad, host **ve** kullanıcı adını tarar — `root` ya da `10.0` yazmak da
bulur. `Esc` önce aramayı temizler, arama yokken çıkar (panellerdeki davranışın
aynısı). Sıralama arama açıkken kapalıdır: gördüğünüz komşu ile gerçek komşu
farklı olurdu.

### Düzenleme formu

```
┌ Bağlantıyı düzenle ──────────────────────────────────────────┐
│ Ad                Prod Sunucu                                │
│ Sunucu (host)     1.2.3.4                                    │
│ Port              22                                         │
│ Kullanıcı         okan                                       │
│ Parola            ••••••••                                   │
│ Anahtar dosyası   ~/.ssh/id_ed25519                          │
│ Anahtar parolası                                             │
│                                                              │
│                                                              │
│  [ Kaydet (Enter) ]  [ Vazgeç (Esc) ]  [ Parolayı göster ]   │
└──────────────────────────────────────────────────────────────┘
```

- `Tab` / `↑` `↓` alanlar arası; alana **tıklayarak** da geçebilirsiniz.
- `Enter` (ya da `Ctrl+S` / `F10`) kaydeder, `Esc` vazgeçer.
- **`F9` parolaları gösterir/gizler** — normalde `•` ile maskelidirler.
- `Port` alanı yalnızca rakam alır. Boş bırakılırsa `22`, `Ad` boş bırakılırsa
  host adı kullanılır.
- Parolanın başındaki/sonundaki boşluk **korunur** (diğer alanlar kırpılır).

> **`ssh-list`ten farkı:** [ssh-list](https://github.com/akinoiro/ssh-list) bilinçli
> olarak parola saklamaz, çünkü işi harici `ssh` istemcisine devreder. tfs SSH
> bağlantısını kendi kurduğu için parolayı saklama seçeneği sunar — ama düz metin
> olduğunu unutmayın; mümkünse `Anahtar dosyası` alanını kullanın.

## Arama — yazmaya başlayın

Aktif panelde **yazmaya başladığınız an** liste filtrelenir; eşleşmeyen girdiler
gizlenir. Ayrı bir "arama moduna" girmeniz gerekmez.

```
┌ UZAK: /var/www — ara: index (3/412) ──┐
│ ▶ 📄 index.html                       │
│   📄 index.php                        │
│   📄 INDEX.md                         │
│                                       │
│   (409 girdi gizlendi)                │
└───────────────────────────────────────┘
```

| Tuş | Ne yapar |
|-----|----------|
| yazılabilir karakter | sorguya ekler, liste anında filtrelenir |
| `Backspace` | sorgu varsa son harfi siler · sorgu yoksa **üst dizine** çıkar |
| `Esc` | sorgu varsa temizler · sorgu yoksa **çıkar** |
| `Enter` | seçili klasöre girer (ve sorguyu temizler) |
| `↑/↓`, `PgUp/PgDn`, `Home/End` | filtrelenmiş liste içinde gezinir |

- Arama **büyük/küçük harf duyarsızdır** ve alt dizge eşleşmesi yapar
  (`index` → `index.html`, `INDEX.md`).
- Her panelin **kendi sorgusu** vardır; `Tab` ile geçtiğinizde diğerinin
  filtresi bozulmaz.
- Dizin değiştirince sorgu temizlenir.
- Harf sildikçe **imleç seçili dosyanın üstünde kalır** — liste büyürken seçim
  zıplamaz.
- Filtre açıkken seçim, transfer ve düzenleme daima **görünen** listeye göre
  çalışır; gizli bir dosya yanlışlıkla seçilemez.

> **Not**: Arama yazılabilir tüm harfleri kullandığı için eski tek harfli
> kısayollar (`q`, `t`, `e`) kaldırıldı. Yerlerine `Ctrl+Q`/`F10`/`Esc` (çıkış),
> `F5` (transfer) ve `F4` (düzenle) geçti — Norton/Midnight Commander geleneği.

## Klasör transferi

Bir klasörü seçip `t`'ye basmak (ya da sürükleyip bırakmak) **ağacın tamamını**
aktarır. Önce ağaç taranır — böylece ilerleme çubuğu gerçek bir toplam gösterir
ve kaçıncı dosyada olduğunuz yazar:

```
┌ Transfer — 37/412 dosya — q/Esc: iptal ─────────────────┐
│ ████████████░░░░░░░░  proje  18.4 MiB / 61.2 MiB  (30%) │
│ …/src/components/header/index.tsx                       │
└─────────────────────────────────────────────────────────┘
```

Davranış:

- **Sembolik bağlar izlenmez.** Bir dizin bağı taramayı sonsuz döngüye sokabilir
  ve bağı hedefiyle sessizce değiştirmek sürpriz olurdu. Atlanır ve sonunda
  "N sembolik bağ atlandı" diye bildirilir.
- **Tek bir dosyanın hatası transferi durdurmaz** — sayılır, ilk hatanın metni
  durum çubuğunda gösterilir ("… 412 dosya · 3 hata · ilk hata: …").
  Listelenemeyen dizinler (izin yok) de aynı şekilde atlanıp bildirilir.
- Hedefte var olan dosyaların **üzerine yazılır**, dizinler yoksa açılır.
- `q`/`Esc` transferi iptal eder (o ana kadar aktarılanlar hedefte kalır).

## Bilinen sınırlar (skeleton — sonraki adımlar)

- Transferler ayrı bir tokio task'inde, parça parça (64 KiB) yapılır ve `mpsc`
  ile ortada bir **progress bar** gösterilir — UI bloklanmaz.
- Dosya izinleri/zaman damgaları korunmaz (SFTP `create` varsayılanı).
- SSH agent (`ssh-agent` / Pageant) desteklenmiyor; anahtar dosyadan okunur.
  Şifreli anahtarın parolası config'te düz metin durur.
- OS dosya yöneticisi ↔ terminal DnD **mümkün değil** (terminal sınırı).
- SSH terminali (F1): fare uzak programlara **iletilmez** (SGR mouse forwarding
  yok) — `htop`/`vim` içinde fare çalışmaz, fare seçme/kopyalama içindir.
  Seçim satır bazlıdır (blok/dikdörtgen seçim yok).

## Kripto backend notu

`russh` varsayılanı `aws-lc-rs` Windows'ta NASM ister; bu yüzden `Cargo.toml`'da
`ring` backend'i seçili (NASM gerektirmez).

---

## English

**tfs** (*terminal-file-send*) is a mouse-first terminal UI that puts an **SFTP
file browser** and a **full interactive SSH terminal** on a *single* SSH
connection. Switch between them with `F1` / `F2` — no reconnect, no second
session, no second tool.

### Features

- **`F2` — two-pane file manager** (local ↔ remote). Transfer files *and whole
  folders* by dragging them with the mouse, or with a keystroke. Live progress,
  cancel with `Esc`.
- **`F1` — a real terminal.** VT100 emulation, so `vim`, `htop` and other
  full-screen programs work. Mouse text selection, clipboard copy/paste
  (right-click pastes), and 1000 lines of scrollback.
- **Built-in connection manager.** Add, edit, duplicate, reorder, delete and
  search your servers without leaving the TUI. Unlike most SSH launchers it can
  **store passwords** for you — masked on screen (`F9` reveals), written
  atomically, and the file is `chmod 0600` on Unix. Prefer keys? Leave the
  password empty and point it at your private key instead.
- **`F4` — edit a remote file** in the [`fresh`](https://github.com/sinelaw/fresh)
  editor: tfs downloads it, opens the editor, and uploads it back when you save.
- **Type to filter.** Start typing in any pane or in the connection list and it
  filters as you go — no separate search mode.
- **Host key verification** against the same `~/.ssh/known_hosts` OpenSSH uses,
  plus publickey auth (`id_ed25519` → `id_ecdsa` → `id_rsa`, or a key you name).
  When auth fails it tells you *which* method failed and why.

### Install

```sh
cargo install tfs-ssh     # the installed command is `tfs`
cargo binstall tfs-ssh    # prebuilt binary, no compiling (Windows)
```

The crate is called `tfs-ssh` because `tfs` was already taken on crates.io — the
command it installs is still `tfs`. Prebuilt Windows binaries (64-bit and 32-bit)
are on the [Releases](https://github.com/okanaytimur/tfs/releases) page.

On first run tfs opens the connection manager with an empty list; add your first
server there and it writes `config.json` for you.

### Platforms

Windows 10+ and Linux (glibc 2.34+ — Ubuntu 22.04+, Debian 12+, RHEL 9+).
Windows 7/8 are not supported. Built with
[ratatui](https://github.com/ratatui/ratatui) and
[russh](https://github.com/Eugeny/russh). Dual-licensed MIT OR Apache-2.0.

> The rest of this README is in Turkish; the sections above cover configuration,
> key bindings and troubleshooting in more detail.
