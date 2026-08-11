# ARCHITECTURE.md — Целевая архитектура нового проекта

> Статус: accepted baseline (этап 1). Rust edition 2024, Tauri 2 + Svelte.
> Принципы: разделение UI и логики, переносимость, минимальная зависимость от ОС, тестируемость, расширяемость, без техдолга.

---

## 1. Архитектурные слои

```
┌─────────────────────────────────────────────┐
│                   UI Layer                  │  Svelte (desktop) — отображение, формы, состояние
├─────────────────────────────────────────────┤
│              Application Layer              │  CLI (clap), Tauri commands — оркестрация
├─────────────────────────────────────────────┤
│                Business Logic               │  core: проект, операции read/write/verify/assemble
├─────────────────────────────────────────────┤
│                Protocol Layer               │  protocol: framing микрокода программатора
├─────────────────────────────────────────────┤
│               Hardware Layer                 │  programmer: ProgrammerBackend (LPT / USB / Mock)
└─────────────────────────────────────────────┘
```

Зависимости строго вниз. UI не знает про железо; Business Logic зависит от абстрактного `ProgrammerBackend`.

---

## 2. Схема крате (workspace)

```
kr1878/
├── crates/
│   ├── core/            # бизнес-логика: память, операции, сборка конвейеров
│   ├── assembler/       # TESSA-ассемблер .mic → MemoryImage + листинг/симв.
│   ├── programmer/      # ProgrammerBackend + LPT/USB/Mock реализации
│   ├── protocol/        # низкоуровневый битбанг (WriteBits/ReadBits/рамки)
│   ├── file-format/     # .sav/.dat/.arp/.lst/.sym читатель/письмователь
│   └── cli/             # бинарник `kr1878` (clap)
├── desktop/
│   ├── src-tauri/       # Tauri 2 app, команды → core
│   └── frontend/        # Svelte
├── docs/
│   ├── PROJECT_STATE.md
│   ├── ARCHITECTURE.md
│   ├── ISA_REFERENCE.md
│   ├── REVERSE_ENGINEERING.md
│   ├── FILE_FORMATS.md
│   ├── HARDWARE_PROTOCOL.md
│   └── adr/
├── tests/               # интеграционные тесты на уровне core/CLI
└── tools/               # вспомогательные утилиты/скрипты
```

---

## 3. Ключевые абстракции

### 3.1 Бизнес-справочник

```rust
pub struct MemoryImage {
    pub program: Vec<u16>,        // память команд
    pub data: [u8; 64],           // ЭСПЗУ данных
    pub config: Option<[u8; 8]>,  // конфигурация/оса (опц.)
}
```

### 3.2 ProgrammerBackend (раскрытие в HARDWARE_PROTOCOL.md)

```rust
pub trait ProgrammerBackend {
    fn identify(&mut self) -> Result<DeviceId, Error>;
    fn erase(&mut self) -> Result<(), Error>;
    fn write_program(&mut self, words: &[u16]) -> Result<(), Error>;
    fn write_data(&mut self, bytes: &[u8; 64]) -> Result<(), Error>;
    fn read_program(&mut self) -> Result<Vec<u16>, Error>;
    fn read_data(&mut self) -> Result<[u8; 64], Error>;
    fn verify_program(&mut self, words: &[u16]) -> Result<bool, Error>;
    fn verify_data(&mut self, bytes: &[u8; 64]) -> Result<bool, Error>;
}
```

### 3.3 Операции Business Logic (один источник правды)

```rust
pub enum Operation { Read, Write, Verify }
pub fn program_image(backend: &mut dyn ProgrammerBackend, img: &MemoryImage) -> Result<VerifyReport, Error>
```
Правила оригинала сохраняются: стиль erase→CMDWrite→записи→verify.

---

## 4. Безопасность и качество
- Rust edition 2024.
- Никакого `unwrap()`/`expect()` в production — обработка через `Result`.
- Public API задокументирован (`///`).
- Ошибки — типизированные `Error` (thiserror).
- Логирование через `tracing`.
- Юнит-тесты в каждом крейте + интеграционные в `tests/`.

## 5. CLI (этап 3)

```
kr1878 assemble input.mic [-o out.sav]
kr1878 read    [--backend ...] [-o dump.sav]
kr1878 write   program.sav [--data eeprom.dat] [--erase-config ...]
kr1878 verify  program.sav [--data eeprom.dat]
kr1878 dev info
```

CLI стабилен раньше GUI. `--backend` сек=on: `lpt`, `usb`, `mock` (debug/CI).

## 6. Решения по формам/config
- `.sav` — канонический little-endian u16 образ (см. FILE_FORMATS.md, ADR-000).
- Параметры аппарата (`lpt base`, `com port`, baud) — конфигурация CLI/env, НЕ зашиты (в отличие от оригинала).

## 7. Переносимость
- LPT backend: `#[cfg]`-фамилья: Windows (`inpout32`) / Linux (`ppdev`) / прочее → недоступно (возврат ошибки «backend unavailable»).
- USB-mosh: кроссплатформенный (hidapi/serialport), позволяющий macOS/Linux/Win.
- `simplest` ядро — platform-agnostic.

## 8. Примечание (этот документ поддерживается в актуальном состоянии при ADR-изменениях)