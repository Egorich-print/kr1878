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

## Earlier
- `core`: ISA (isa/operand), memory, ops, `Backend` trait — 12 тестов.
- `file-format`: `.sav`/`.dat`/`.arp` — 12 тестов.
- `assembler`: двухпроходный TESSA (lexer/expr/symbols/preprocess/encoder) — 16 тестов.