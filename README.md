<div align="center">

# 🧩 KR1878VE1 Toolkit

### Полный инструментальный комплекс для микроконтроллера **КР1878ВЕ1** (ядро ТЕСЕЙ)

Ассемблер · Исполняющий симулятор · Программаторы (LPT / USB) · Настольное приложение

[![Rust](https://img.shields.io/badge/Rust-edition_2024-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8D8?logo=tauri&logoColor=white)](https://tauri.app)
[![Tests](https://img.shields.io/badge/tests-112%20green-2ea44f)](#-тесты)
[![Clippy](https://img.shields.io/badge/clippy--D%20warnings-clean-2ea44f)](#-качество)

</div>

---

Проект — результат исследования и **реверс-инжиниринга оригинального тулчейна**
Ангстрем (`docs/REVERSE_ENGINEERING.md`): от битовых кодировок ISA до блочного
протокола официального USB-программатора `Uprg1878.exe`. Весь код написан с нуля
и лицензирован MIT; протоколы и факты изложены в документации своими словами.

---

## ✨ Возможности

| | Возможность | Подробнее |
|---|---|---|
| 🛠️ | **Ассемблер `.mic`** — двухпроходная сборка | макросы, `#define`, `#include`, условные директивы, локальные/глобальные метки, листинг, таблица символов |
| ⚙️ | **Исполняющий CPU-симулятор** | декодирует и выполняет каждую команду ядра ТЕСЕЙ против состояния процессора — скетчи моргания **реально переключают порт B** |
| 💾 | **Программаторы** | `lpt` (параллельный порт), `usb` (официальный протокол Uprg1878.exe), `sim` (протокольный симулятор), `mock` (in-memory) |
| 📦 | **Форматы файлов** | `.sav`, `.dat`, `.arp`, `.sim` (+ чтение/запись официального `.sav` с 256-словным заголовком) |
| 🖥️ | **Настольное приложение** | Tauri 2 + SvelteKit: редактор, сборка, программирование, дамп |
| 🔬 | **ISA-справочник** | полное описание системы команд своими словами, сверено с реализацией |

---

## 🚀 Быстрый старт

### Сборка и прошивка на симуляторе

```sh
# сборка .mic -> .sav
cargo run -- assemble demo.mic --out demo.sav

# программирование + верификация на протокольном симуляторе (состояние в .sim)
cargo run -- write demo.mic --backend sim --sim-state chip.sim --osc 3
cargo run -- verify demo.mic --backend sim --sim-state chip.sim
cargo run -- read --backend sim --sim-state chip.sim --out dump.sav
```

### 💡 Исполнение программы (CPU-симулятор)

```sh
cargo run -- run examples/blink_ram.mic --steps 200000 --dump
```

> `run` исполняет `.sav`/`.mic` на аккуратном CPU-симуляторе ядра ТЕСЕЙ
> (`kr1878-core::cpu`): декодирует каждую команду и выполняет её против состояния
> процессора (PC/RS/SR, память данных, стеки, IR0/IR1). Скетчи моргания **реально
> переключают порт B** — запустите с разным числом шагов и сравните `port B`
> (`0x01 ↔ 0x00`). `--dump` печатает память данных, `--eeprom` — data EEPROM.

### 🔌 Программирование реального железа

```sh
# LPT-программатор (Linux/FreeBSD x86-64)
cargo run -- write demo.mic --backend lpt

# USB/COM-мост (официальный протокол Uprg1878.exe, write-only)
cargo run -- ports
cargo run -- write demo.mic --backend usb --port /dev/ttyUSB0 --gen quartz-high --no-delay
```

> **USB/COM-мост** расшифрован из официального бинаря (см.
> [`docs/HARDWARE_PROTOCOL.md`](docs/HARDWARE_PROTOCOL.md) §7): UART **38400-8N1**,
> блоки ровно **16 байт**, подтверждение байтом **`'R'`** (0x52) на каждый блок;
> data EEPROM и память программ пишутся дважды; конфигурационное слово
> (RDE/OT1/OT2, биты утроены) передаётся отдельным блоком. Протокол **write-only** —
> read/verify через USB-мост не поддерживаются (нет read-back).

---

## 🖥️ Настольное приложение

```sh
cd desktop
npm install
npm run tauri dev          # режим разработки
npm run tauri build        # релизная сборка
```

Редактор исходников `.mic`, сборка, программирование/проверка/чтение устройства,
дамп памяти и data EEPROM, выбор бэкенда и файла состояния — всё в одном окне.

---

## 🎨 Примеры (скетчи)

| Пример | Описание |
|--------|----------|
| [`blink_led.mic`](examples/blink_led.mic) | Моргание светодиодом на **PB0** (настройка порта через подрегистры конфигурации, счётчики в ЭСППЗУ-регионе) |
| [`blink_ram.mic`](examples/blink_ram.mic) | То же, но счётчики в **ОЗУ** — короче и нагляднее для инструкционного симулятора |

Схемы подключения и команды — в [`examples/README.md`](examples/README.md).

---

## 🧱 Структура

```
crates/
  core/         # ISA-кодирование, память, типаж Backend, операции, CPU-симулятор
  protocol/     # нижний протокольный слой (битбанг, CRC, Device, симулятор)
  assembler/    # .mic -> MemoryImage + листинг + символы
  programmer/   # бэкенды: Mock / Sim / LPT / USB
  file-format/  # .sav, .dat, .arp, .sim
  cli/          # консольный интерфейс (assemble/read/write/verify/ports/run)
desktop/        # Tauri 2 + SvelteKit UI
examples/       # тестовые скетчи (.mic)
docs/           # ISA-справочник, исследование, стандарты, ADR, форматы файлов
```

---

## 🧭 Документация

| Документ | О чём |
|----------|-------|
| [`docs/ISA_REFERENCE.md`](docs/ISA_REFERENCE.md) | Полный справочник по системе команд КР1878ВЕ1: кодировки, семантика, директивы ассемблера (составлен своими словами; факты не охраняются АП) |
| [`docs/REVERSE_ENGINEERING.md`](docs/REVERSE_ENGINEERING.md) | Как была восстановлена ISA и протоколы из бинарников |
| [`docs/HARDWARE_PROTOCOL.md`](docs/HARDWARE_PROTOCOL.md) | Протоколы программаторов: LPT-битбанг и USB/COM-мост Uprg1878.exe |
| [`docs/FILE_FORMATS.md`](docs/FILE_FORMATS.md) | Форматы файлов `.sav/.dat/.arp/.sim` |
| [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) | Целевая архитектура (слои: UI → CLI → core → protocol → hardware) |
| [`docs/adr/`](docs/adr/) | Реестр архитектурных решений (ADR) |

---

## ✅ Качество

- **112 тестов** зелёные (105 workspace + 7 desktop): ISA-кодирование,
  round-trip decode/encode, исполнение CPU, ассемблер, форматы файлов,
  протокол, CLI e2e, USB-бэкенд.
- **clippy `--workspace --all-features -- -D warnings`** — чисто.
- Скетчи проходят полный цикл: **собрать → прошить (sim) → верифицировать → прочитать**.
- ⚠️ Требуется проверка на железе: полярность BUSY/ACK и тайминги LPT
  смоделированы, но не подтверждены на физическом программаторе
  ([`docs/adr/ADR-004`](docs/adr/ADR-004-lint-and-testutil.md) §2).

---

## ⚖️ Лицензия

[MIT](LICENSE) — код написан с нуля по результатам реверс-инжиниринга;
документация излагает факты (не охраняются авторским правом) своими словами.
