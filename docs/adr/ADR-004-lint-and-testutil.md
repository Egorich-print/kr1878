# ADR-004 — Политика lint-гигиены и API `protocol::testutil`

## Статус
Принят (2026-08-06); разделы про `testutil` и симуляцию дополнены в Block 2/3.

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

## Альтернативы
- Завершить lint-гигиену только в новых crates: отвергнуто — `-D warnings` должен проходить по всему workspace, чтобы CI не регрессировать на legacy.
- Запретить любые правки legacy: отвергнуто — оставило бы workspace не-`-D`-чистым навсегда.