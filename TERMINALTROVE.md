# Terminal Trove submission copy

> Bu dosya kullanıcı dokümanı **değil**: [terminaltrove.com](https://terminaltrove.com)
> ve benzeri dizinlere gönderilirken forma yapıştırılacak İngilizce metin burada
> duruyor ki her seferinde yeniden yazılmasın. Yeni sürümde özellik listesini
> README'nin "English" bölümüyle birlikte güncelle.

## Name

```
tfs
```

## Tagline (one line)

```
An SFTP file browser and a full SSH terminal on a single connection.
```

## Short description

```
tfs is a mouse-first terminal UI that puts a two-pane SFTP file manager and a
full interactive SSH terminal on one SSH connection. Press F2 to drag files and
folders between local and remote, F1 for a real VT100 terminal - no reconnect,
no second session.
```

## Long description

```
tfs (terminal-file-send) gives you two tools over a single SSH connection. F2 is
a two-pane local <-> remote file manager: drag files and whole folders across
with the mouse, watch live progress, cancel with Esc. F1 is a real terminal with
VT100 emulation, so vim and htop work, along with mouse text selection,
clipboard paste and scrollback. Switching between them never reconnects.

Servers are managed inside the TUI - add, edit, duplicate, reorder, delete and
search them without touching a config file. Unlike most SSH launchers tfs can
store passwords for you: masked on screen, written atomically, and chmod 0600 on
Unix. Prefer keys, and it follows OpenSSH's order - publickey first, then
password - verifying host keys against your existing ~/.ssh/known_hosts.

F4 opens the selected remote file in the fresh editor: tfs downloads it, opens
the editor, and uploads it back when you save.
```

## Metadata

| Field | Value |
|-------|-------|
| Repository | `https://github.com/okanaytimur/tfs` |
| Homepage | `https://github.com/okanaytimur/tfs` |
| Install | `cargo install tfs-ssh` (the command is `tfs`) |
| Language | Rust |
| License | MIT OR Apache-2.0 |
| Platforms | Linux (x86_64, static), Windows 10+ (x86_64, i686) |
| Tags | `ssh`, `sftp`, `file-transfer`, `terminal`, `tui`, `file-manager`, `rust` |

## Notes for the demo GIF

Bir çekimde aracın tamamını gösteren sıra:

1. Bağlantı yöneticisinde `[+ Yeni]` → bir sunucu ekle
2. Satıra tıkla → bağlan
3. F2'de yerelden uzağa bir klasör sürükle (ilerleme çubuğu görünsün)
4. F1'e geç, `htop` ya da `vim` aç — aynı bağlantı, yeniden bağlanma yok
5. Panelde yazmaya başla, listenin filtrelendiğini göster

10-15 saniye yeter. GIF'i `assets/demo.gif` olarak koyup README'nin başındaki
yorum satırını aç.
