## Свой движок окна для Dota 2 Mod Manager

Для тех, у кого Mod Manager не открывается с ошибкой **«Could not find the webview runtime»** (урезанные и «оптимизированные» сборки Windows, удалённый Edge, переустановка WebView2 не помогает) или показывает пустое либо чёрное окно на Linux. Приложение ставит себе свой движок окна, не трогая систему.

### Файлы

| Файл | Что это |
|---|---|
| `webview2-setup.exe` | Установщик для Windows, который запускает Mod Manager: ставит WebView2 в данные приложения, без прав администратора, в окне в стиле Mod Manager. Как подключить — [INTEGRATION.md](../../blob/main/INTEGRATION.md) |
| `Microsoft.WebView2.FixedVersionRuntime.158.0.4370.45.x64.cab` | WebView2 Fixed Version от Microsoft, без изменений; подписи Microsoft проверены при сборке |
| `webkitgtk-linux-x64-2.52.6.tar.gz` | WebKitGTK 2.52.6 со всем стеком (GTK, Mesa, glibc), Ubuntu 24.04 |
| `webview-runtime-linux-x64` | Статический запускатор для Linux: ставит движок и запускает программу на нём |
| `runtime.json` | Размер и SHA-256 каждого архива: качается и запускается только то, что в нём записано |

### Проверено

- Windows 10: установка (скачивание, проверка, распаковка за 7 с), отмена, окно WebView2 на поставленном движке.
- Linux в CI: Ubuntu 22.04/24.04, Mint 22, Debian 12/13, Kali, Arch, Manjaro, Fedora, openSUSE Tumbleweed, Alpine, Void musl, Nix — установка и отрисовка страницы.
- Linux на живых рабочих столах: Mod Manager 4.7.1 на этом движке в Linux Mint 22.3 и EndeavourOS.

### Ещё не сделано

- Подключение в самом Mod Manager (см. INTEGRATION.md).
- Проверка на Windows, где WebView2 нет совсем.

---

## A window engine of Dota 2 Mod Manager's own

For people whose Mod Manager stops with **"Could not find the webview runtime"** (stripped or "optimized" Windows builds, Edge removed, reinstalling WebView2 does not help) or shows a blank or black window on Linux. The app installs a window engine of its own, without touching the system.

| File | What it is |
|---|---|
| `webview2-setup.exe` | The Windows installer Mod Manager starts: WebView2 into the app's data, no administrator rights, in a window of the app's style. How to plug it in: [INTEGRATION.md](../../blob/main/INTEGRATION.md) |
| `Microsoft.WebView2.FixedVersionRuntime.158.0.4370.45.x64.cab` | Microsoft's WebView2 Fixed Version, unchanged; Microsoft's signatures checked at build |
| `webkitgtk-linux-x64-2.52.6.tar.gz` | WebKitGTK 2.52.6 with its whole stack (GTK, Mesa, glibc), Ubuntu 24.04 |
| `webview-runtime-linux-x64` | Static Linux launcher: installs the engine and starts a program on it |
| `runtime.json` | Size and SHA-256 of each archive: only what it names is downloaded and run |
