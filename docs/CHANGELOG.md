# CHANGELOG

## [Unreleased]

### Added
- `kr1878-protocol`: нижний протокольный слой:
  - `transport::Transport` (`write_control` / `write_data` / `read_status` / `delay`), cинхр. ACK/BUSY.
  - `bits`: одноклоковый битбанг `write_bits` / `read_bits`, `wait_ack`, `Timing`.
  - `command`: командный/стирающий импульс (`0x38`→toggle×20→`0x3F`), `idle_off`.
  - `crc`: CRC-16/CCITT + `endc_checksum3`.
  - `device`: `Device` (select/erase/write/read/verify → `SessionResult`), `with_timing`.
  - 14 юнит-тестов.
- `kr1878-programmer`: Hardware-слой:
  - `LptTransport` (база `0x378`, инверсия BUSY/ACK по протоколу; I/O на Linux/FreeBSD x86-64, иначе `backend unavailable`).
  - `WireBackend` (адаптер `protocol::Device` → `core::backend::Backend`).
  - `MockBackend` (в памяти, стирание → `0xFFFF`).
  - 3 юнит + 2 интеграционных (`core::ops` end-to-end).

### Fixed (protocol)
- `write_bits`/`read_bits` переведены на одноклоковое тактирование: запись — только C1, чтение — только C2; устранена коллизия часов и переполнение регистра чтения в симуляторе.
- `read_words`/`read_bytes` повторно выбирают память (`select(CMD_PROGRAM/CMD_DATA)`); симулятор сбрасывает счётчик чтения при `write_data` (привязка к памяти).
- Симулятор тактирует запись только при выбранном адресе записи — импульс стирания/команды не даёт битов.
- `read_bits` семполирует BUSY после фронта RCLK и обнаруживает front по фронту (не по уровню).
- `write_bits`/`read_bits` больше не пишут лидирующий `CONTROL_IDLE`: если шина была в all-off (после `idle_off`), этот фазовый фронт давал лишний бит (смещение канала на 1). Теперь первый низкий такт убирает и запись, и чтение.

### Added (Block 2 — OSC / config канал)
- `kr1878-protocol`: `CMD_CONFIG = 0x01`, `Device::write_config/read_config`, `run`/`verify_image_equals` учитывают `expected_config`, `SessionResult::config_back`.
- `kr1878-protocol`: `SimulatorTransport` хранит `config_bits`; запись/чтение канала OSC.
- `kr1878-core`: `ops::program_with/verify_with` + `WriteOptions` (phase-gates; источник — `.arp`), `Backend::write_config/read_config` (default no-op), `read()` возвращает `config`.
- `kr1878-programmer`: `MockBackend`/`WireBackend` реализуют config-канал.
- `kr1878` CLI: `write/verify --osc N --arp file`; `--no-erase`; `read` печатает OSC-байт.
- Тесты: 16 (protocol), 14 (core), 7 (programmer), bulk read 1K слов.

### Added (Block 3 — WireBackend integration coverage)
- `kr1878-protocol`: `testutil` вынесен под feature `testutil`
  (`#[cfg(any(test, feature = "testutil"))]`), `SimulatorTransport`/`RecordingTransport`
  доступны downstream-крейтам.
- `kr1878-protocol`: `InvertedStatusTransport` — адаптер полярности BUSY/ACK
  (симуляция инвертированных уровней LPT-шины; `raw()`/`normalized()`).
- `kr1878-programmer`: интеграционные тесты `tests/wire_integration.rs` гоняют
  реальный путь `WireBackend → protocol::Device → Transport` (не `MockBackend`):
  - `erase → write → read → verify` round-trip через `ops::program`/`ops::read`/`ops::verify`;
  - config-канал, phase-gate `WriteOptions { data: false }`, `identify`;
  - полный сеанс поверх `InvertedStatusTransport::inverted(SimulatorTransport)`.
- Тесты workspace: 67 зелёных (было 62; +5 интеграционных в `programmer`).
- TODO: валидация LPT-полярности/таймингов на железе (ADR-004 §2).

## Earlier
- `core`: ISA (isa/operand), memory, ops, `Backend` trait — 12 тестов.
- `file-format`: `.sav`/`.dat`/`.arp` — 12 тестов.
- `assembler`: двухпроходный TESSA (lexer/expr/symbols/preprocess/encoder) — 16 тестов.