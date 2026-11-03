# Известные падения CI — что сейчас красное и почему

> **Этот файл — маркер для агента и для владельца.** Каждый пункт воспроизведён
> на реальном прогоне CI и указывает точное место `файл:строка`.
> Починил пункт — убери его отсюда и скажи об этом в сообщении коммита.
>
> Контекст: репозиторий проверяют **два** независимых гейта, и оба зовут один
> и тот же скрипт `scripts/gate.sh`:
>
>   * **Jenkins** (self-hosted, LXC 120 на pve-xeon, multibranch-папка
>     `ci-github`, ветка `main`) — оркестратор аккаунта: единая очередь, общий
>     кэш cargo, артефакты в `/srv/jenkins/archive`, релиз по тегу;
>   * **GitHub Actions** (`.github/workflows/ci.yml`) — тонкая обёртка над тем же
>     гейт-скриптом: видимый статус на странице репозитория и проверки на PR.
>
> Логика проверок живёт в репозитории, а не в обоих CI: Jenkinsfile находит
> `scripts/gate.sh` и вызывает его вместо базового набора.

---

## Известных падений нет — 2026-11-03

Красных пунктов в этом списке сейчас нет. Последний прогон Jenkins
(`ci-github/kr1878/main`) до починки — **#9, FAILURE**, с той же ошибкой,
что и в issue #1; после починки гейт зелёный локально на macOS
(Rust 1.99.0) и на цели `x86_64-unknown-linux-gnu` (Rust 1.99.1, тот же
компилятор, что у репортёра).

Что было починено 2026-11-03 (историческая справка — из этого списка пункты
убраны по правилу выше):

| Что | Где | Как починено |
|---|---|---|
| `libc::outb` / `libc::inb` не существуют ни в одной версии `libc` → крейт не компилировался на Linux x86_64 | `crates/programmer/src/lpt.rs` | портовый ввод-вывод переведён на inline-ассемблер `out dx, al` / `in al, dx` (в `libc` этих функций нет — это инструкции x86, а не вызовы libc); `ioperm` — с правильной сигнатурой `c_ulong` и проверкой результата |
| `cargo fmt --all --check` показывал расхождения, а в базовом наборе Jenkins был только WARN | весь воркспейс | `cargo fmt --all`; в собственном гейте форматирование — полноценный гейт |
| `cargo clippy` без `-D warnings`: новые линты Rust 1.99 проходили молча | весь воркспейс | гейт гоняет `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| MSRV `rust-version = "1.85"` был объявлен, но **не наследовался ни одним крейтом** → clippy не учитывал MSRV и требовал API новее заявленного | `crates/*/Cargo.toml` | `rust-version.workspace = true` во всех шести крейтах |
| неиспользуемый операнд `b3` в списке покрытия инструкций | `crates/core/src/isa.rs` | операнд использован по назначению (`Mov` с B-сегментом) — покрытие выросло, а не просто подавлено |
| `vec!` там, где хватает массива | `crates/programmer/src/usb.rs` | массив |
| в репозитории **не было родного гейт-скрипта** → Jenkins гонял базовый набор, а `cargo test --all-targets` без `--workspace` не выполнял юнит-тесты пяти крейтов | `scripts/gate.sh` | добавлен родной гейт; Jenkinsfile вызывает его вместо базового набора |
| копия `Jenkinsfile` в репозитории отставала от каноничной: без `flock --close` (зависшие сборки) и с урезанным таймаутом | `Jenkinsfile` | заменён на каноничный `infrastructure/jenkins/Jenkinsfile.generic` |

---

## Требования к сборочному окружению

Системная зависимость одна, и она неочевидна: крейт `kr1878-programmer` тянет
`serialport` → `libudev-sys`, а его `build.rs` ищет системную библиотеку
`libudev` через `pkg-config`. Без пакета сборка падает с

```
The system library `libudev` required by crate `libudev-sys` was not found
```

Где требование выполнено:

| Среда | Как |
|---|---|
| Контейнер Jenkins (LXC 120) | `libudev-dev` установлен пакетом `infrastructure/proxmox/xeon-install-jenkins-build-deps.sh` в LXC 120 |
| GitHub Actions | `.github/workflows/ci.yml` ставит `libudev-dev` и `pkg-config` явно |

## Как устроен CI

* **Тулчейн** — `rust-toolchain.toml`: канал `1.99.0`, профиль `minimal`,
  компоненты `rustfmt` и `clippy`. В контейнере Jenkins эта версия уже
  стоит (там же `1.98.0` и `stable`), rustup доустанавливает её сам.
* **MSRV** — `rust-version = "1.85"` в корневом `Cargo.toml`. Крейты обязаны
  наследовать его (`rust-version.workspace = true`): без этого объявление
  ничего не значит, и clippy начинает требовать API новее заявленного.
* **Гейт** — `scripts/gate.sh`: `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets --all-features -- -D warnings`, `cargo test
  --workspace --all-targets`, `cargo test --workspace --doc` и проверка дерева
  на зашитые ключи.
* **Платформенная ловушка** — код LPT-бэкенда за
  `#[cfg(all(target_os = "linux", target_arch = "x86_64"))]`, поэтому на macOS
  и Windows он вырезается из сборки, и крейт там компилируется. Ошибка,
  видимая только на Linux, на macOS не проявляется: проверяй Linux-таргет
  явно — `KR1878_GATE_LINUX_TARGET=1 ./scripts/gate.sh` (нужен установленный
  target `x86_64-unknown-linux-gnu`; на macOS дополнительно требуется
  системная libudev для `libudev-sys`).
