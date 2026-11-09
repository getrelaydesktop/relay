# Relay Desktop

Прототип интерфейса Telegram Desktop на Rust и Slint.

## Запуск

Для авторизации задайте `TG_API_HASH` перед сборкой. API ID задан в `TG_API_ID`
в `src/telegram.rs`; при использовании своего приложения замените и его.
Без API hash программа собирается, но вход в Telegram недоступен.

Workflow `Build debug and release` собирает Windows x64 при push в `main`,
push тега `v*`, pull request и ручном запуске через Actions.
Готовые `.exe` и лицензии доступны в артефактах
`relay-desktop-windows-x64-debug` и `relay-desktop-windows-x64-release`.
Для авторизации в этих сборках добавьте repository secret `TG_API_HASH`.
Секреты недоступны в pull request из сторонних fork.

```sh
cargo run --locked
```

Для Windows нужна среда Rust MSVC с C++ Build Tools и Windows SDK.
На Linux используются X11 или Wayland. Например, для сборки на Debian/Ubuntu
нужны пакеты `build-essential pkg-config libfontconfig1-dev`.

Оформление открывается кнопкой меню слева от поиска. Доступны темы Telegram,
Material You и WinUI, светлая и тёмная схемы. Для Material доступны три акцента.
Это оформления, нарисованные Slint. Тема WinUI не использует нативный WinUI 3,
а Material использует подготовленные палитры вместо системной генерации цветов.

Настройки оформления сохраняются в
`%LOCALAPPDATA%/relay-desktop/preferences.json` на Windows и
`$XDG_CONFIG_HOME/relay-desktop/preferences.json` на Linux.
Если XDG_CONFIG_HOME отсутствует, используется `~/.config`.
Переменная `RELAY_CONFIG_DIR` задаёт другую папку настроек.

Работают поиск чатов, переключение переписки, черновики в пределах сеанса,
локальная отправка текста и вставка эмодзи. Ctrl+K переводит фокус в поиск,
Enter открывает первый результат или отправляет текст.

Сейчас используются демонстрационные данные в памяти.
Подключение Telegram, авторизация, вложения и звонки ещё не реализованы.
Интерфейс воспроизводит основные размеры Telegram Desktop, но точное
пиксельное совпадение со всеми его экранами пока не достигнуто.

Проверка Rust-кода

```sh
cargo fmt --check
cargo clippy --locked -- -D warnings
```
