# КР1878ВЕ1 Toolkit

Современная кроссплатформенная (macOS / Linux / Windows 10-11) реализация набора инструментов
для микроконтроллера **КР1878ВЕ1** («Тесей»): ассемблер TESSA, программатор и отладочная среда.

Стек: **Rust (core)** · **Tauri 2 / Svelte (desktop UI)** · **CLI**.

## Возможно/задачи

- `assemble` — ассемблерная трансляция `.mic` → `.sav`/`.lst`/`.sym`.
- `read` / `write` / `verify` — работа с ЭСПЗУ команд и данных через `ProgrammerBackend`.
- Совместимость с оборудованием оригинала (программатор через LPT1; modern USB-мост; отладочный COM).

## Документация

- `docs/PROJECT_STATE.md` — текущий статус и план.
- `docs/ARCHITECTURE.md` — целевая архитектура.
- `docs/REVERSE_ENGINEERING.md` — восстановленная спецификация МК (ISA, регистры, векторы).
- `docs/FILE_FORMATS.md` — спецификация форматов файлов.
- `docs/HARDWARE_PROTOCOL.md` — аппаратный протокол.
- `docs/adr/` — регистр архитектурных решений.

## Сборка

```bash
cargo build --workspace
cargo test  --workspace
```

## Статус

См. `docs/PROJECT_STATE.md`. Проект инициализирован (этап 1 завершён, этап 2 — core в разработке).