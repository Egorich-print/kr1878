# AGENTS.md — kr1878

## Правила работы в репозитории

### Сборка зелёная — проверяй гейтом, а не «на глаз»

Родной гейт репозитория — `scripts/gate.sh`. Он же зовут оба CI
(Jenkins и GitHub Actions), поэтому его вердикт и есть вердикт проекта:

```bash
./scripts/gate.sh
```

Гейт строже базового набора Jenkins: `cargo fmt --all -- --check` (жёстко),
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo test --workspace --all-targets`, `cargo test --workspace --doc`.

**Не ограничивайся `cargo build` и `cargo test` без `--workspace`.**
В воркспейсе `default-members = ["crates/cli"]`, поэтому сборка только
дефолтных крейтов не трогает крейты `core`, `assembler`, `protocol`,
`file-format` и `programmer` — а их юнит-тесты при этом не выполняются.

### Платформенная ловушка: LPT-код есть только на Linux x86_64

Код портового ввода-вывода (`crates/programmer/src/lpt.rs`) за
`#[cfg(all(target_os = "linux", target_arch = "x86_64"))]`. На macOS и
Windows он вырезается из сборки — и крейт `kr1878-programmer` там
компилируется, даже если код сломан. Ошибка, видимая только на Linux, на
macOS не проявляется (именно так в дерево попали несуществующие
`libc::outb`/`libc::inb`).

Проверяй Linux-таргет явно:

```bash
KR1878_GATE_LINUX_TARGET=1 ./scripts/gate.sh
```

Нужны target `x86_64-unknown-linux-gnu` и системная `libudev` (на macOS
она не ставится штатно — сборка `libudev-sys` упадёт на build-скрипте, и
это ожидаемо, а не дефект кода).

### Портовый ввод-вывод — только инструкциями x86

В крейте `libc` **нет** функций `inb()`/`outb()` — это инструкции x86, а не
вызовы libc. `libc` даёт только `ioperm()`. Перенос байта делается
inline-ассемблером `out dx, al` / `in al, dx` (см. `port_io` в
`crates/programmer/src/lpt.rs`). Не «чини» код возвратом к `libc::outb`.

### Тулчейн и MSRV

* Версия тулчейна — `rust-toolchain.toml` (канал `1.99.0`, `rustfmt` +
  `clippy`). Не меняй её молча: смена канала меняет и набор линтов clippy.
* MSRV — `rust-version = "1.85"` в корневом `Cargo.toml`. Новый крейт
  **обязан** наследовать его (`rust-version.workspace = true`), иначе
  объявление перестаёт работать и clippy начинает требовать API новее MSRV.

### Коммиты

* Перед коммитом вычищай зашитые ключи (`sk-…`, `ghp_…`, приватные ключи) —
  гейт проверяет дерево на них и уронит сборку, но чистить надо до коммита.
* Историю не переписывай; коммит — одним куском на логическую правку.

### Известные падения CI

Список красного с точными `файл:строка` — в `CI-KNOWN-FAILURES.md`. Читай
её **до** правки кода. Сейчас открытых пунктов нет.

## Что где лежит

| Путь | Что это |
|---|---|
| `crates/core` | ISA: кодирование/декодирование, модель памяти, операции |
| `crates/assembler` | ассемблер TESSA (`.mic` → образ памяти/листинг/символы) |
| `crates/file-format` | читатели/писатели `.sav`/`.dat`/`.arp`/`.lst`/`.sym` |
| `crates/protocol` | низкоуровневый протокол программирования |
| `crates/programmer` | бэкенды: LPT bit-bang, serial/USB-мост, симулятор |
| `crates/cli` | CLI `kr1878` (имя пакета — `kr1878`, бинарник — `kr1878`) |
| `desktop/` | приложение Tauri 2 + Svelte, **вне** воркспейса (свой граф сборки) |
| `scripts/gate.sh` | родной гейт репозитория |
| `docs/TOOLCHAIN_OPTIONS.md` | варианты компиляции под ТЕСЕЙ (C/C++/Zig/Rust/Goose/Lua/Lisp), IDE и API |
| `docs/` | архитектура, форматы файлов, протокол железа, ISA |
