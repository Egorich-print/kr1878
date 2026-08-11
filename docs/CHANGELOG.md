# CHANGELOG

## [Unreleased]

### Added (CPU simulator)
- **`kr1878-core::cpu`** — исполняющий CPU-симулятор ядра ТЕСЕЙ:
  - `Instruction::decode` — декодер, точный инверс `encode` (round-trip всех 55
    инструкций, тест `decode_roundtrips_all_instructions`);
  - `Cpu`: PC/RS-флаги, сегменты SR0-7, стеки команд/данных, IR0/IR1 косвенная
    адресация, память данных 256 байт, `step()`/`run(n)`, прерывания;
  - исполнение всех 52 команд с корректными флагами Z/S/C/OF/DC;
  - CLI `kr1878 run <image> [--steps N] [--dump] [--eeprom]` — исполнение
    `.sav`/`.mic` и дамп состояния; **blink-скетчи реально моргают портом B**.
- `examples/blink_ram.mic` — короткая версия blink (счётчики в ОЗУ) для
  симулятора.
- **Исправлена кодировка ветвлений**: опкод 4 бита в битах 15..12, адрес
  12 бит (ранее ошибочно 6/10). Устранена коллизия ADDL↔JNS. Обновлены
  `docs/ISA_REFERENCE.md` §2.7/§3.5 и `docs/REVERSE_ENGINEERING.md` §5.5.

### Added (assembler gaps from audit)
- `rtsc [c]` — возврат с битом C (0x000E | c).
- `.endc` — 3-байтовая контрольная сумма (сумма байтов кода с переносами),
  записывается в тело программы (`endc_checksum3` перенесён в `kr1878-core::crc`).
- Псевдооперации загрузки адресов: `mial/miah`, `mdal/mdah` (в регистр памяти),
  `ldal/ldah`, `lia/liah` (в SR6/SR7). `mdah`/`ldah` очищают биты 6-7 старшей
  части (по даташиту).
- CLI `assemble --lst FILE --sym FILE` — вывод листинга и таблицы символов.
- CLI `--include-dir DIR` — рабочий `#include` loader (файлы по каталогу).
- Исправлен баг переименования макро-меток `@nn` в конце строки.
- `write_sav_uprg` — запись официального `.sav` с 256-словным заголовком.
- `mea` задокументирован как псевдооперация отладчика MC_WIN (не аппаратная
  команда), см. `docs/ISA_REFERENCE.md`.

### Added (examples)
- `examples/blink_led.mic` — тестовый скетч «моргание светодиодом на PB0»:
  настройка порта B через подрегистры регистра конфигурации (0x1A),
  программная задержка вложенным счётчиком. Собран, прошит через
  `--backend sim` и верифицирован (round-trip `.sav` совпадает).
  Карта периферии восстановлена из даташита (табл. 3: 0x01/0x02 — раб. регистры
  портов A/B, 0x19/0x1A — конфигурация портов, 0x04/0x05 — таймер, 0x38/0x39/
  0x3F — блок ЭСППЗУ, ОЗУ 0x40–0xDF). `examples/README.md` с инструкцией.

### Added (docs)
- `docs/ISA_REFERENCE.md` — полный справочник по системе команд КР1878ВЕ1:
  архитектура, форматы команд, все 52 команды с кодировками и семантикой,
  флаги, псевдооперации, директивы/препроцессор ассемблера, векторы
  прерываний, каркас программы. Составлен своими словами по собственным
  исследованиям (факты, не охраняемые авторским правом); кодировки сверены
  с `crates/core/src/isa.rs`. Кросс-ссылки добавлены в README,
  ARCHITECTURE.md, PROJECT_STATE.md.

### Added (Uprg1878 analysis)
- Расшифрован официальный USB-программатор `Uprg1878.exe` (результат анализа
  официального бинаря, полученного от Angstrom): `docs/HARDWARE_PROTOCOL.md` §7.
  - COM-UART 38400-8N1 через USB-мост (НЕ LPT).
  - Блочная передача 16 байт с подтверждением `'R'` (0x52) на каждый блок.
  - Фазы: sync → data EEPROM ×2 (64 байта) → program `.sav` с пропуском 256-словного
    заголовка → конфиг-слово → program повтор.
  - Форма `TForm1` (unit1.pas), порт-обёртка `com_port.pas`; config-слово = RDE/OT1/OT2,
    биты утроены (см. §7.5).
- **`kr1878-programmer` USB bridge backend** (`UsbBridgeBackend`, `usb.rs`):
  - `serialport` 38400-8N1, блоки 16 байт, `'R'`-подтверждение (drain-до-тишины);
  - полная последовательность Uprg1878: sync → data×2 → program → config → program;
  - shadow-image для read (протокол write-only, read-back отсутствует);
  - `GeneratorMode` (RC / Internal / QuartzHigh / QuartzLow) + `config_word(delay, gen)`;
  - `UsbBridge::available_ports` — список портов (аналог `GetSerialPortsList`).
- **`kr1878-file-format`**: `read_sav_uprg` — чтение официальных `.sav`
  (пропуск 256-словного заголовка), `sav::UPRG_HEADER_WORDS`.
- **CLI**: `--backend usb --port <port>`, `--gen`, `--no-delay`; `kr1878 ports`.
  read/verify над USB-мостом — явная ошибка (нет read-back).
- **Desktop**: backend `usb` в списке, поле ввода порта, `program_device` для USB.

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

### Added (Block 2 — WireBackend integration coverage)
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

### Added (Block 3 — CLI productization)
- `kr1878-file-format`: `.sim`-снапшот состояния устройства (`KRSI` magic, 1024 words
  + data EEPROM + config; `read_sim`/`write_sim`/`write_sim_file`, 4 теста).
- `kr1878-programmer`: `SimBackend` под feature `sim` — полный протокольный путь
  (`WireBackend` → `protocol::Device` → `SimulatorTransport`) без железа;
  персистентность через `.sim`-файл (`load_or_new`, авто-сохранение при записи).
- `kr1878` CLI:
  - `--backend sim` (симулированный чип; реальный протокол, в отличие от Mock);
  - `--sim-state FILE` в `read`/`write`/`verify` — состояние переживает запуски;
  - `write`/`verify` принимают `.mic` напрямую (авто-ассемблирование).
- Тесты CLI: `tests/cli.rs` (3) — assemble→.sav, cross-process write→verify
  round-trip с `.sim`-state (вкл. mismatch), read dump config + program.
- Тесты workspace: **77** зелёных (было 67; +4 `.sim`, +3 SimBackend, +3 CLI).

### Added (Block 4 — Desktop UI, Tauri 2 + Svelte)
- Новый каталог `desktop/` — настольное приложение, **вне** cargo-workspace
  (отдельный граф сборки с `tauri-build`; в root `Cargo.toml` — `exclude`).
- `desktop/src-tauri/src/ops.rs` — тестируемый слой команд над ядром:
  `assemble` (.mic → words/listing/symbols), `connect` (backend + sim-state),
  `program`, `verify`, `read`. Живой бэкенд живёт в Tauri-managed state
  (`Mutex<Box<dyn Backend>>`), поэтому `write → verify → read` работают с
  одним устройством (как физический программатор на одном чипе).
- Типажи `Backend` (`core::backend`) и `Transport` (`protocol::transport`)
  помечены `Send` — требование Tauri `State` (все реализации держат plain data).
- `desktop/src-tauri/src/lib.rs` — команды Tauri: `list_backends`, `assemble`,
  `connect`, `program`, `verify`, `read_device` (ошибки → `{message}`).
- SvelteKit UI (`src/routes/+page.svelte`): выбор бэкенда mock/sim/lpt,
  `.mic`-редактор с примером, OSC-флаг (hex), чекбокс erase,
  кнопки Program+Verify / Verify / Read, дамп слов устройства, listing и symbols.
- Сборка: `npm run check` (svelte-check 0 ошибок) и
  `npm run tauri build -- --no-bundle` → `desktop/src-tauri/target/release/desktop`.
- Тесты: 7 юнит-тестов в `ops.rs` (assemble, mock/sim round-trip, mismatch,
  identity, connect); workspace по-прежнему **77** зелёных.

### Fixed (Block 4 — по результатам аудита)
- **Ассемблер — паника на `.`**: `Expr::Here` (`jmp .+1`, `.byte .`) уходил в
  `unreachable!` в `expr.rs` → panic вместо диагностики. Добавлен `Expr::with_here(pc)`,
  `.` подставляется до eval (`eval_expr`). Новые тесты: `.`-выражения, `.byte .`.
- **Ассемблер — листинг показывал `0000`** вместо реальных слов: `emit_words`
  возвращал нули-заглушки. Теперь в листинг попадают фактически закодированные
  слова (`process` возвращает реальные `u16`). Тест `listing_shows_real_encoded_words`.
- **Ассемблер — `.=` не работал**: лексер выдаёт `Token::Equal` (не символ), поэтому
  директива `.= N` падала с «expected directive name». Разбор `.=` вынесен на уровень
  `Token::Dot + Token::Equal`. Тест `setpc_directive_moves_cursor`.
- **Desktop — `connect` не подключал**: команда создавала backend локально и
  выбрасывала; все program/verify/read работали с `Mock`. Теперь `connect` заменяет
  backend в Tauri-managed state (`AppBackend`), выбор Sim/LPT и `.sim`-файл реально
  используются. Lock-poisoning обрабатывается как ошибка, а не `expect()`-паника.
- **LPT — конфиг не программировался**: `LptBackend` наследовал no-op
  `write_config`/`read_config`, поэтому `--osc` молча игнорировался на железе.
  Добавлено делегирование в `WireBackend`.
- **Desktop UI**: обработка ошибок `invoke` (try/catch, красный error-блок),
  busy-индикация и блокировка кнопок, «исходник изменён — пересоберите»,
  требование «сначала подключите устройство», валидация OSC (hex 00–FF),
  дамп с адресами + секция Data EEPROM и config, кнопка «Загрузить демо»,
  сброс состояния при смене бэкенда, русский интерфейс и заголовок окна.
- Тесты: workspace **81** зелёный (было 77; +4 в assembler).

## Earlier
- `core`: ISA (isa/operand), memory, ops, `Backend` trait — 12 тестов.
- `file-format`: `.sav`/`.dat`/`.arp` — 12 тестов.
- `assembler`: двухпроходный TESSA (lexer/expr/symbols/preprocess/encoder) — 16 тестов.