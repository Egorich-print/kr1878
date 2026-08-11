# ADR-004 — Политика lint-гигиены и API `protocol::testutil`

## Статус
Принят (2026-08-06); разделы про `testutil` и симуляцию дополнены в Block 2/3/4.

## 1. Политика lint-гигиены

### Контекст
Workspace не проходил `cargo clippy --workspace --all-targets -- -D warnings`: накоплены механические нарушения (`ops.get(0)` → `first()`, unused imports, `doc-lazy-continuation`, collapsible `if`, пересложные типы). Исторические crates (`core`, `assembler`, `file-format`) формально «завершены», и их правка без явного указания ограничена.

### Решение
- Механические lint-only изменения в **завершённых** crates **допустимы** без пересмотра функционального статуса:
  - `get(0)` → `first()`, удаление неиспользуемых импортов/мёртвого кода,
  - авто-`cargo fmt`,
  - пересчёт типа через `type` alias, collapsible `if`, `RangeInclusive::contains`.
- Критерий «без поведенческого изменения»: `cargo test --workspace` — те же (или больше) зелёные тесты; `git diff` — только механические правки.
- Требование качества: после каждой смены — `fmt --check` + `clippy -D warnings` + `test --workspace`.

## 2. `protocol::testutil` / simulator API

### Решение (Block 2)
- `SimulatorTransport` живёт как *публичный, feature-gated* API в `protocol::testutil`:
  - `kr1878-protocol` объявляет `[features] testutil = []`; модуль компилируется при
    `#[cfg(any(test, feature = "testutil"))]` — в release-сборках его нет.
  - Downstream-тесты подключают его через
    `kr1878-protocol = { path = "../protocol", features = ["testutil"] }` в `[dev-dependencies]`.
- `testutil` экспортирует:
  - `RecordingTransport` — волновой тротт, записывает control/data события.
  - `SimulatorTransport` — исполнимая модель чипа (биты по регистрам, replay на READ-клоке).
  - `InvertedStatusTransport` — адаптер полярности: `inverted(inner)` подаёт на протокол
    нормализованные (active-high) уровни, но позволяет наблюдать «сырые» инвертированные
    уровни LPT-шины через `raw()`/`normalized()`.

### Решение (Block 3 — интеграционное покрытие `WireBackend`)
- Тесты `crates/programmer/tests/wire_integration.rs` гоняют **настоящий** путь
  `WireBackend → protocol::Device → Transport` (без `MockBackend`):
  - `erase → write_program → read_program` и `write_data → read_data` round-trip;
  - `ops::program` (с config) + независимый `ops::read` + `ops::verify` (match/mismatch);
  - phase-gate `WriteOptions { data: false }` — data EEPROM не трогается;
  - `identify` — имя устройства.
- Полярность BUSY/ACK проверяется без железа: тест прогоняет весь сеанс поверх
  `InvertedStatusTransport::inverted(SimulatorTransport)` и отдельно проверяет
  `raw()` (инвертированные биты) против `normalized()` (active-high).

### TODO (LPT hardware validation)
- Реальная проверка полярности BUSY/ACK и таймингов на физическом LPT (нужен x86-64 +
  Linux/FreeBSD либо эквивалент). До тех пор договорённость о полярности покрывается
  симуляцией, описанной выше; на железе поведение не подтверждено.

## 3. `SimBackend` — симулированный backend (Block 3)

### Контекст
Для CLI нужен способ гонять *честный* протокольный путь (`WireBackend` ->
`protocol::Device` -> транспорт) без железа: Mock-бэкенд хранит `MemoryImage` и
обходит протокол, поэтому не годится для проверки прошивочного сценария.

### Решение
- `kr1878-programmer` объявляет feature `sim = ["kr1878-protocol/testutil"]` и
  `SimBackend` поверх `SimulatorTransport` (все операции делегированы в `WireBackend`).
- Персистентность: `.sim`-снапшот (`kr1878-file-format::sim`); `SimBackend::load_or_new`
  загружает состояние и авто-сохраняет при каждом мутирующем вызове — состояние
  переживает раздельные CLI-запуски (`write` затем `verify`).
- CLI: `--backend sim` + `--sim-state FILE`; `write`/`verify` принимают `.mic`
  (авто-ассемблирование).
- Альтернатива «добавить полярную инверсию в SimBackend» отклонена: полярность уже
  покрыта отдельным адаптером `InvertedStatusTransport` на уровне транспорта.

## 4. Desktop UI — Tauri 2 + Svelte (Block 4)

### Контекст
Тулчейн доведён до CLI (assemble/read/write/verify, `--backend mock|sim|lpt`).
Для широкой аудитории нужен тонкий графический фронт, который не дублирует
прошивочную логику, а переиспользует собранные крейты ядра.

### Решение
- Новый каталог `desktop/` **вне** cargo-workspace: Tauri требует собственный граф
  сборки (`tauri-build`, `beforeDevCommand`, мобильные таргеты), а workspace
  остаётся библиотечным. В корневом `Cargo.toml` — `exclude = ["desktop/src-tauri"]`;
  крэйт подключает крейты ядра по `path: ../../crates/*`.
- Слой команд `desktop/src-tauri/src/ops.rs` — чистые функции, повторяющие CLI-поток
  (assemble / connect / program / verify / read) над `&mut dyn Backend`, без зависимостей
  от Tauri; покрыт юнит-тестами. Tauri-обёртка в `lib.rs` — тонкий адаптер.
- **Живой бэкенд в managed-state**: `AppBackend(Mutex<Box<dyn Backend>>)` — в отличие от CLI
  (по одному бэкенду на вызов + `.sim`-файл для персистентности), приложение держит одно
  устройство на всю сессию, чтобы `write → verify → read` были согласованы (как реальный
  программатор, подключённый к одному чипу). Для sim-бэкенда опционально указывается
  `.sim`-файл, сохраняющий состояние между сессиями приложения.
- Типажи `Backend` (`core::backend`) и `Transport` (`protocol::transport`) получили
  bound `Send` — требование `tauri::State<Send+Sync>`. Все реализации держат только
  plain data/Vec, поэтому разрядка безопасна (силами линта подтверждена clippy `-D`).
- Frontend: SvelteKit (адаптер-static, `ssr=false`), единый экран
  (бэкенд → .mic → assemble → program/verify/read + дамп). Команды вызываются
  через `@tauri-apps/api/core::invoke`.
- Вариант «spawn CLI-бинарь из UI по argv» отклонён: переусложнение, хрупкий парсинг,
  неудобная потоковая передача образов; ядро доступно как библиотеки.

## Альтернативы
- Завершить lint-гигиену только в новых crates: отвергнуто — `-D warnings` должен проходить по всему workspace, чтобы CI не регрессировать на legacy.
- Запретить любые правки legacy: отвергнуто — оставило бы workspace не-`-D`-чистым навсегда.