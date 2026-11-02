# Известные падения CI — что сейчас красное и почему

> **Этот файл — маркер для агента и для владельца.** Каждый пункт воспроизведён
> на реальном прогоне CI (2026-11-02) и указывает точное место `файл:строка`.
> Починил пункт — убери его отсюда и скажи об этом в сообщении коммита.
>
> Контекст: репозиторий собирает Jenkins (multibranch-проект `kr1878`, ветка
> `main`). Собственного CI на GitHub Actions у репозитория нет — Jenkins здесь
> единственный гейт.

---

## 1. `libc::outb` / `libc::inb` не существуют → крейт не компилируется — **открыто**

**Симптом**

```
error[E0425]: cannot find function `outb` in crate `libc`
error[E0425]: cannot find function `inb` in crate `libc`
error[E0308]: mismatched types
error: could not compile `kr1878-programmer` (lib) due to 4 previous errors
```

**Где**

- `crates/programmer/src/lpt.rs:167` — `libc::outb(val, addr);`
- `crates/programmer/src/lpt.rs:186` — `return Ok(libc::inb(addr));`

**Причина.** Таких функций в крейте `libc` **нет ни в одной версии** — проверено
по исходникам 0.2.69, 0.2.155, 0.2.189, 0.2.191 и по docs.rs (`libc::outb` → 404).
В `libc` есть только `ioperm`. То есть `kr1878-programmer` **никогда не
компилировался**: в `Cargo.toml` воркспейса `default-members = ["crates/cli"]`,
поэтому обычная сборка этот крейт не трогает. Полный гейт
(`cargo test --workspace`) собирает весь воркспейс и находит это сразу.

**Как починить**

1. заменить вызовы на inline-ассемблер:
   `core::arch::asm!("out dx, al", in("dx") addr, in("al") val)` /
   `core::arch::asm!("in al, dx", out("al") val, in("dx") addr)`;
2. либо взять крейт `x86` (у него есть `x86::in` / `x86::out`);
3. либо вывести `kr1878-programmer` из воркспейса, если LPT-бэкенд больше не нужен.

**Проверка**

```bash
cargo build -p kr1878-programmer
cargo test --workspace
```

---

## 2. `cargo fmt --all --check` показывает расхождения — **открыто**

Тот же прогон падает на форматировании: код отформатирован не под `rustfmt`.
Гейт в CI начинается с `cargo fmt --all --check`, поэтому пункт 1 не виден, пока
не пройден формат.

```bash
cargo fmt --all
```

---

## 3. Требования к сборочному окружению

Для этого репозитория в контейнере сборки нужен `libudev-dev`: крейт
`libudev-sys` ищет системную библиотеку через `pkg-config` и без неё падает с
«The system library `libudev` required by crate `libudev-sys` was not found».
