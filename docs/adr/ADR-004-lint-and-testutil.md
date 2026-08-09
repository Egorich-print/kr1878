# ADR-004 — Политика lint-гигиены и API `protocol::testutil`

## Статус
Принят (2026-08-06); раздел про `testutil` дополняется в Block 3.

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
*(Раздел будет дополнен в Block 3 — вынос `SimulatorTransport` в публичный `protocol::testutil` под feature, интеграционное покрытие `WireBackend` по `erase → write → read → verify`.)*

## Альтернативы
- Завершить lint-гигиену только в новых crates: отвергнуто — `-D warnings` должен проходить по всему workspace, чтобы CI не регрессировать на legacy.
- Запретить любые правки legacy: отвергнуто — оставило бы workspace не-`-D`-чистым навсегда.