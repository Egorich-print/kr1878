# PROJECT_STATE — КР1878ВЕ1 Toolkit

> Статус: **research → analysis complete; core, file-format, assembler, protocol and programmer implemented**
> Дата: 2026-08-06
> Цель: современная кроссплатформенная реализация набора инструментов для микроконтроллера КР1878ВЕ1 («Тесей») без изменения пользовательского сценария оригинала.

---

## 1. Миссия проекта

Создать современную реализацию устаревшего ПО для МК КР1878ВЕ1 с использованием стека
**Rust (core) + Tauri 2 / Svelte (desktop UI)**, с сохранением:

- пользовательского сценария (загрузить проект → ассемблировать → прошить/проверить);
- форматов файлов (`.mic`, `.sav`, `.lst`, `.dat`, `.arp`);
- алгоритмов работы (эрase → write → verify, чтение ЭСППЗУ команд и данных);
- совместимости с существующим оборудованием (программатор через LPT1, отладочный модуль по COM).

НЕ создаётся новый универсальный программатор. Вся логика — в Rust Core; аппаратура — максимально простой исполнитель команд.

---

## 2. Статус этапов

| Этап | Название | Статус |
|------|----------|--------|
| 0 | Изучение стандартов организации | ✅ завершён |
| 1 | Анализ исходного ПО и документации | ✅ завершён (документы см. `docs/`) |
| 2 | Минимальный Rust Core (без UI) | ✅ завершён (core+file-format+assembler+protocol+programmer) |
| 3 | CLI-инструмент `kr1878` | 🔄 в работе |
| 4 | Tauri 2 + Svelte desktop | ⏳ запланирован |

## 3. Анализ исходного материала — итоги

**Исходники:** `/Users/egorich/Downloads/КР1878`
- `prog/` — пакет ПО: реальные бинарники — `E03_PROG.EXE` (Win95 программator), `P_WMCT.EXE` (DOS programas), `MC_WIN.EXE` (DOS debugger). Часть `.ex_` — сохранённые веб-страницы Yandex (мусор).
- `КР1878ВЕ1-doc/` — даташит + 12 глав (система команд, TESSA, ESPPZU, debugger).
- `MCT_IV/` — конфиги отладочной среды (`.ini`, `.fnt`, `MC_WIN.EX_`).

**Ключевые восстановленные спецификации (можные ссылки):**
- `docs/REVERSE_ENGINEERING.md` — результат обратного проектирования ISA и бинарников.
- `docs/ARCHITECTURE.md` — целевая архитектура нового проекта.
- `docs/FILE_FORMATS.md` — спецификация форматов файлов.
- `docs/HARDWARE_PROTOCOL.md` — аппаратный протокол программования/отладки.

---

## 4. Архитектурные решения

ADR-реестр: `docs/adr/`. Открытые решения фиксируются немедленно.

- **ADR-001** — языки/стек: Rust edition 2024 core, Tauri 2 + Svelte UI, CLI.
- Планируются: выбор ProgrammerBackend стратегий, формат `.sav`, порядок переноса.

---

## 5. Что уже работает

| Компонент | Статус |
|-----------|--------|
| `core` (ISA кодирование, память, ops, backend trait) | ✅ реализован, 14 тестов |
| `file-format` (`.sav`, `.dat`, `.arp`, `.sim`) | ✅ реализован, 16 тестов |
| `assembler` (`kr1878-assembler`) | ✅ двухпроходный, 16 тестов |
| `protocol` (`kr1878-protocol`) | ✅ битбанг + драйвер `Device`, 16 тестов, канал OSC/config, `testutil` (feature) с полярностной моделью |
| `programmer` (`kr1878-programmer`) | ✅ LPT/mock/sim/wire backends, 15 тестов (вкл. WireBackend e2e и SimBackend с `.sim`-персистентностью) |
| `cli` (`kr1878`) | ✅ assemble/read/write/verify (clap; `--osc`/`--arp`/`--backend sim`/`--sim-state`), 3 e2e теста |

**Стек реализованного ассемблера:**
- Препроцессор: `#define`, `#include` (через loader), `.macro/.endmacro` с подстановкой аргументов и авто-переименованием `@N`, `.if/.else/.endif`.
- Лексер языка из `docs/FILE_FORMATS.md` §1 (базы `h/o/b`, `;`-комментарии, операторы `* / & + - \ ! ` и `^`, скобки).
- Арифметический парсер по правилам приоритета документации (`()` → `* / &` → `+ - \ !`).
- Двухпроходная сборка: pass 1 — символы/адреса, pass 2 — кодирование; глобальные метки, локальные `$n`, константы `name=const`.
- Псевдо: `inc/dec/clr`; битовые `BICK/ВIS/BTG/BTT` с суффиксами `l/h`; псевдо-загрузка адресов `mial/miah/mdal/mdah/ldal/ldah`.
- Директивы: `.byte/.word/.even/.bpt/.=`, `.end/.list`.
- Полная таблица опкодов из `docs/REVERSE_ENGINEERING.md` §5 (двухоперандные, литеральные, однооперандные, сервисные SR, ветвления, фикс. слова).

---

## 6. Найденные проблемы и риски

- `.sav` бинарный формат: точная байтовая структура (заголовок/карта адресов) не задокументирована и достоверно не восстановлена из бинарников (блокируется MFC-хелпером чтения). Требуется эмпирическая проверка / допущение (раскрыто в FILE_FORMATS.md).
- Оригинал работает через LPT (в современных системах отсутствует) → нужна абстракция ProgrammerBackend и обсудимый modern USB-мост.
- `0x378`/`0x379`/`0x37A` — базовый адрес LPT1 встроен (не конфигурируется в Win build).
- Разлиsee между ISA и TESSA-документами: опкоды извлечены вручную, требуют юнит-тестов на валидных примерах.

---

## 7. Следующие задачи

1. ✅ (готово в docs) — пять документов этапа 1.
2. ✅ Rust workspace `core` / `assembler` / `programmer` / `protocol` / `file-format` / `cli`.
3. ✅ ISA-кодирование + юнит-тесты (`core`, 12 тестов).
4. ✅ «облегчённый» ассемблер TESSA (двухпроходный, `.mic` → `MemoryImage`+листинг+символы).
5. ✅ парсер/райтер `.sav`, `.dat`, `.arp` (`file-format`, 12 тестов).
6. ✅ `protocol` — битбанг-примитивы + драйвер `Device::run` (14 тестов).
7. ✅ `programmer` — бэкенды `Mock` + `Wire`/`LPT` (LPT-полярность не проверена на железе).
8. ✅ CLI `kr1878 assemble|read|write|verify` (clap, `--backend mock|lpt`).
9. ✅ Канал OSC/config завершён (protocol `CMD_CONFIG=0x01`, `run`/verify через `expected_config`; sim store; `config_back`); `Device::write_config/read_config`; bulk read 1K слов покрыт тестом.
10. ✅ `.arp`/OSC выведены на CLI: `kr1878 write|verify --osc N --arp file` (phase-gates `WriteOptions` в `core::ops`).
11. ✅ Интеграционное покрытие `WireBackend` по реальному протокольному пути
    (`protocol::testutil` под feature; polarity-адаптер; `erase → write → read → verify` без `MockBackend`).
    LPT-полярность смоделирована; **проверка на железе остаётся TODO** (ADR-004 §2).
12. ✅ CLI productization: `--backend sim` (реальный protocol-путь без железа) +
    `--sim-state FILE` (состояние переживает запуски через `.sim`-снапшот) +
    авто-ассемблирование `.mic` в `write`/`verify`; 3 e2e-теста бинаря.
13. ✅ Desktop-приложение (Tauri 2 + SvelteKit): тонкий UI над собранным ядром.
    Рабочий каталог `desktop/` (вне cargo-workspace — отдельный граф сборки `tauri-build`):
    - `desktop/src-tauri` — Rust-крэйт с тестируемым слоем команд `ops.rs`
      (assemble / connect / program / verify / read; живой бэкенд в Tauri-state,
      чтобы `write → verify → read` работали с одним устройством) + 7 юнит-тестов.
    - Типажи `Backend`/`Transport` помечены `Send` (требование Tauri-managed state).
    - Svelte-UI: выбор бэкенда (mock/sim/lpt), `.mic`-редактор с ассемблированием,
      OSC-флаг, program+verify / verify / read с дампом слов.
    - Сборка `npm run tauri build -- --no-bundle` → `desktop/src-tauri/target/release/desktop`.
14. 🔜 Документация пользователя + инструкция.

> Статус этапа: **этап 4 (desktop-UI) реализован и собирается; осталась полировка UI + документация пользователя (этап 5).**
> Блоки 1–3 (ADR-004): lint hygiene ✅ · protocol completion ✅ · WireBackend coverage ✅ · CLI productization ✅ · Desktop UI ✅.

---

## 8. Как проверить текущее состояние

```
cargo test --workspace              # юнит + интеграционные тесты (77)
cargo build --workspace             # сборка
cargo clippy --workspace --all-targets -- -D warnings  # обязателен
cargo fmt --all -- --check          # обязателен

# Desktop (Tauri) — отдельный граф сборки:
cd desktop && npm install
npm run check                      # svelte-kit sync + svelte-check
npm run tauri build -- --no-bundle # -> src-tauri/target/release/desktop
cd src-tauri && cargo test && cargo clippy --all-targets && cargo fmt --check
```

> Блоки 1–4 (ADR-004): lint hygiene ✅ · protocol completion ✅ · WireBackend coverage ✅ · CLI productization ✅ · Desktop UI ✅.

---

*Поддерживать этот файл в актуальном состоянии при каждом крупном шаге.*