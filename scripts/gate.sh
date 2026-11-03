#!/bin/bash
# kr1878: родной гейт репозитория для оркестратора Jenkins.
#
# ПОЧЕМУ ЭТОТ ФАЙЛ ЕСТЬ. Jenkins запускает гейт-скрипт репозитория, когда он
# есть (Jenkinsfile ищет scripts/gate.sh, scripts/ci.sh, scripts/ci/check.sh,
# tools/gate.sh) — именно чтобы оркестратор НЕ дублировал проверки проекта.
# Пока такого файла не было, задание kr1878/main в Jenkins шло по «базовому
# набору» для rust:
#
#   cargo fmt --all -- --check || echo "WARN: ..."   # не роняет сборку
#   cargo clippy --all-targets --all-features        # без -D warnings
#   cargo test --all-targets                         # БЕЗ --workspace
#
# И там три дыры, каждая из которых стоила красных сборок или тихого дефекта:
#
#   1. `cargo test --all-targets` без `--workspace` гоняет тесты только
#      default-members = ["crates/cli"]. Юнит-тесты внутри crates/core,
#      crates/assembler, crates/protocol, crates/file-format, crates/programmer
#      (модули `#[cfg(test)]` в src/) в CI не выполнялись ВООБЩЕ.
#   2. `cargo fmt --check` был только предупреждением — расхождения
#      форматирования копились и не мешали сборке пройти.
#   3. `cargo clippy` без `-D warnings` — новые линты (например, появившиеся
#      в Rust 1.99) проходили молча.
#
# ЧТО ОН РАНИТ. Ровно те гейты, которые считаются настоящими для этого
# проекта, и ничего сверх того:
#
#   1. cargo fmt --all -- --check                     — форматирование, строго;
#   2. cargo clippy --workspace --all-targets --all-features -- -D warnings;
#   3. cargo test --workspace --all-targets           — тесты ВСЕГО воркспейса;
#   4. cargo test --workspace --doc                   — док-тесты (в --all-targets
#                                                       они не попадают);
#   5. проверка дерева на зашитые ключи — по правилу владельца репозитория
#      «перед коммитом вычищать sk-/ключи»: ключ, попавший в коммит, должен
#      падать в CI, а не всплывать в ревью.
#
# ОСОБЫЙ СЛУЧАЙ — LPT-бэкенд. Код портового ввода-вывода лежит за
# #[cfg(all(target_os = "linux", target_arch = "x86_64"))], поэтому на macOS и
# на Windows он просто вырезается из сборки, и крейт kr1878-programmer на этих
# платформах компилируется. Именно поэтому ошибка `cannot find function
# 'outb' in crate 'libc'` жила в дереве незамеченной: на Linux x86-64
# (линукс-раннер Jenkins) она честно валила сборку, на macOS её не было видно.
# Гейт ниже закрывает обе стороны: на Linux-раннере путь LPT собирается
# нативно, а на других машинах его можно проверить явно:
#
#   KR1878_GATE_LINUX_TARGET=1 ./scripts/gate.sh
#
# (нужен установленный target x86_64-unknown-linux-gnu и системная libudev для
# libudev-sys; на macOS без неё проверка честно падает с текстом ошибки).
#
# ОКРУЖЕНИЕ ДЛЯ ТОГО, КТО ЗАПУСКАЕТ ВРУЧНУЮ:
#   * гейт обязан идти внутри git-чекаута: проверка дерева спрашивает git;
#   * версия тулчейна берётся из rust-toolchain.toml репозитория;
#   * скрипт не оставляет фоновых процессов: Jenkins зовёт его под
#     `flock --close`, и переживший проверку хелпер держал бы блокировку
#     после выхода гейта (см. комментарий про flock --close в Jenkinsfile).
set -uo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO" || exit 1

echo "== kr1878 gate: $(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo 'no git') =="
if ! git rev-parse --git-dir >/dev/null 2>&1; then
	echo "FATAL: гейту нужен git checkout — без индекса проверка дерева не работает."
	exit 1
fi

failed=0
step() {
	name="$1"
	shift
	echo
	echo "-- $name: $*"
	if "$@"; then
		echo "-- $name: OK"
	else
		echo "-- $name: FAIL"
		failed=1
	fi
}

# 1. Форматирование. В базовом наборе Jenkins это был предупреждение, здесь —
#    полноценный гейт: расхождение rustfmt означает, что дерево не соответствует
#    тому, чем оно будет собрано в релиз.
step "fmt" cargo fmt --all -- --check

# 2. Clippy с -D warnings: предупреждение — это дефект, который доживает до
#    релиза. --workspace обязателен (см. шап 1: без него часть крейтов не
#    проверяется вовсе).
step "clippy" cargo clippy --workspace --all-targets --all-features -- -D warnings

# 3. Тесты всего воркспейса, а не только default-members.
step "test" cargo test --workspace --all-targets

# 4. Док-тесты: в --all-targets они не попадают.
step "doc" cargo test --workspace --doc

# 5. Ключи в дереве. Правило владельца репозитория: «перед коммитом вычищать
#    sk-/ключи». Узкие шаблоны, чтобы не ловить мусор:
#      sk-…            — ключи API (OpenAI-совместимые);
#      ghp_… / github_pat_… — токены GitHub;
#      -----BEGIN … PRIVATE KEY----- — приватные ключи.
step "secrets" bash -c '
set -o pipefail
if git grep -nEI "(sk-[A-Za-z0-9_-]{20,}|ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{20,}|-----BEGIN [A-Z ]*PRIVATE KEY-----)" -- . \
	| grep -vE "^(docs/|README|CHANGELOG|scripts/gate\.sh)" ; then
	echo "в дереве найдены похожие на ключи строки — см. список выше"
	exit 1
fi
echo "ключей в дереве не найдено"
'

# 6. Явная проверка LPT-пути на Linux x86_64 — по требованию, не по умолчанию:
#    на Linux-раннере Jenkins он и так собирается нативно.
if [ "${KR1878_GATE_LINUX_TARGET:-0}" = "1" ]; then
	step "linux-target" cargo check --target x86_64-unknown-linux-gnu -p kr1878-programmer
else
	echo
	echo "-- linux-target: пропущен (KR1878_GATE_LINUX_TARGET=1 — включить)"
fi

echo
if [ "$failed" -ne 0 ]; then
	echo "== kr1878 gate: RED =="
	exit 1
fi
echo "== kr1878 gate: GREEN =="
exit 0
