<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type AssembleResult = {
    words: number[];
    data: number[];
    config: number | null;
    listing: string;
    symbols: string[];
  };
  type ImageContents = {
    program: number[];
    data: number[];
    config: number | null;
  };
  type SessionSpec = {
    program: number[];
    data: number[] | null;
    config: number | null;
    osc: number | null;
    erase: boolean;
  };

  // ---- state ------------------------------------------------------------
  const demoSource = `; KR1878VE1 — демо
; бесконечный цикл: счётчик на %a1, прыжок назад
start:  inc     %a1
        jmp     start
`;

  let source = $state("");
  let backends = $state<[string, string][]>([]);
  let backend = $state("mock");
  let simState = $state("");
  let port = $state("");
  let connection = $state("");
  let connected = $state(false);
  let oscHex = $state("");
  let erase = $state(true);

  let wordCount = $state(0);
  let symbols = $state<string[]>([]);
  let listing = $state("");
  let assembledWords = $state<number[]>([]);
  let assembledData = $state<number[]>([]);
  let assembledConfig = $state<number | null>(null);
  let assembled = $state(false);
  let sourceDirty = $state(false);

  let status = $state("");
  let error = $state("");
  let busy = $state(false);
  let dump = $state("");
  let dumpData = $state("");
  let dumpConfig = $state("");

  // ---- helpers ----------------------------------------------------------
  function fmtWords(program: number[], width = 16): string {
    const lines: string[] = [];
    for (let i = 0; i < program.length; i += width) {
      const row = program
        .slice(i, i + width)
        .map((w) => w.toString(16).padStart(4, "0").toUpperCase())
        .join(" ");
      lines.push(`${(i >>> 0).toString(16).padStart(4, "0").toUpperCase()}: ${row}`);
    }
    return lines.join("\n");
  }

  function fmtData(data: number[]): string {
    const lines: string[] = [];
    for (let i = 0; i < data.length; i += 16) {
      const row = data
        .slice(i, i + 16)
        .map((b) => b.toString(16).padStart(2, "0").toUpperCase())
        .join(" ");
      lines.push(`${(i >>> 0).toString(16).padStart(2, "0").toUpperCase()}: ${row}`);
    }
    return lines.join("\n");
  }

  function hexByte(s: string): number | null {
    const t = s.trim();
    if (!/^[0-9a-fA-F]{1,2}$/.test(t)) return null;
    return parseInt(t, 16);
  }

  function setStatus(msg: string) {
    status = msg;
    error = "";
  }

  function setError(e: unknown) {
    const msg =
      typeof e === "object" && e && "message" in e
        ? String((e as { message: unknown }).message)
        : String(e);
    error = msg || "Неизвестная ошибка";
    status = "";
  }

  async function withBusy<T>(fn: () => Promise<T>): Promise<T | undefined> {
    busy = true;
    error = "";
    try {
      return await fn();
    } catch (e) {
      setError(e);
      return undefined;
    } finally {
      busy = false;
    }
  }

  function spec(): SessionSpec {
    const osc = hexByte(oscHex);
    return {
      program: assembledWords,
      data: assembledData,
      config: assembledConfig,
      osc,
      erase,
    };
  }

  // ---- actions ----------------------------------------------------------
  onMount(() => {
    loadBackends();
  });

  async function loadBackends() {
    await withBusy(async () => {
      backends = await invoke<[string, string][]>("list_backends");
    });
  }

  function loadDemo() {
    source = demoSource;
    sourceDirty = false;
    error = "";
  }

  function onSourceEdit() {
    sourceDirty = assembled;
  }

  function onBackendChange() {
    connection = "";
    connected = false;
    status = "";
    error = "";
  }

  async function connectBackend() {
    const r = await withBusy(() =>
      invoke<string>("connect", {
        backend,
        simState: simState || null,
        port: port || null,
      }),
    );
    if (r !== undefined) {
      connection = r;
      connected = true;
      setStatus(`Подключено: ${r}`);
    }
  }

  async function doAssemble() {
    const r = await withBusy(() => invoke<AssembleResult>("assemble", { source }));
    if (!r) return;
    wordCount = r.words.length;
    symbols = r.symbols;
    listing = r.listing;
    assembledWords = r.words;
    assembledData = r.data;
    assembledConfig = r.config;
    assembled = true;
    sourceDirty = false;
    setStatus(
      `Сборка: ${r.words.length} слов, ${r.symbols.length} символов, ` +
        `data: ${r.data.length} байт, config: ${r.config === null ? "не задан" : "0x" + r.config.toString(16).toUpperCase()}`,
    );
  }

  async function doProgram() {
    const ok = await withBusy(() => invoke<boolean>("program", { spec: spec() }));
    if (ok !== undefined) setStatus(ok ? "Программирование + проверка: ОК" : "Программирование + проверка: НЕ УДАЛОСЬ");
  }

  async function doVerify() {
    const ok = await withBusy(() => invoke<boolean>("verify", { spec: spec() }));
    if (ok !== undefined) setStatus(ok ? "Проверка: ОК" : "Проверка: НЕ УДАЛОСЬ");
  }

  async function doRead() {
    const r = await withBusy(() => invoke<ImageContents>("read"));
    if (!r) return;
    dump = fmtWords(r.program);
    dumpData = fmtData(r.data);
    dumpConfig =
      r.config === null ? "не задан" : "0x" + r.config.toString(16).toUpperCase();
    setStatus(
      `Чтение: ${r.program.length} слов, data: ${r.data.length} байт, config: ${dumpConfig}`,
    );
  }

  const canProgram = $derived(assembled && !sourceDirty && connected && !busy);
  const canAssemble = $derived(!busy);
  const canConnect = $derived(!busy);
</script>

<main class="wrap">
  <header class="head">
    <h1>KR1878VE1 Toolkit</h1>
    <button onclick={loadDemo} disabled={busy}>Загрузить демо</button>
  </header>

  <section>
    <h2>Устройство</h2>
    <div class="row">
      <select bind:value={backend} onchange={onBackendChange} disabled={busy}>
        {#each backends as [id, label]}
          <option value={id}>{label}</option>
        {/each}
      </select>
      {#if backend === 'sim'}
        <input
          placeholder="файл состояния .sim (необязательно)"
          bind:value={simState}
          class="wide"
          disabled={busy}
        />
      {/if}
      {#if backend === 'usb'}
        <input
          placeholder="последовательный порт (например /dev/ttyUSB0)"
          bind:value={port}
          class="wide"
          disabled={busy}
        />
      {/if}
      <button onclick={connectBackend} disabled={canConnect ? undefined : true}>
        Подключить
      </button>
      {#if connected}
        <span class="badge ok">подключено: {connection}</span>
      {:else}
        <span class="badge">не подключено</span>
      {/if}
    </div>
  </section>

  <section>
    <h2>Исходный код (.mic)</h2>
    <textarea
      class="src"
      placeholder={demoSource}
      bind:value={source}
      oninput={onSourceEdit}
      disabled={busy}
    ></textarea>
    <div class="row">
      <button onclick={doAssemble} disabled={canAssemble ? undefined : true}>
        Собрать
      </button>
      <span>
        слов: {wordCount}, символов: {symbols.length}
        {#if sourceDirty}
          <span class="warn">исходник изменён — пересоберите</span>
        {/if}
      </span>
    </div>
  </section>

  <section>
    <h2>Программа / проверка</h2>
    <div class="row">
      <label>
        OSC (hex, 00–FF)
        <input
          class="osc"
          placeholder="03"
          bind:value={oscHex}
          disabled={busy}
        />
      </label>
      <label class="chk">
        <input type="checkbox" bind:checked={erase} disabled={busy} />
        стирать перед записью
      </label>
    </div>
    {#if !connected}
      <p class="warn">Сначала подключите устройство.</p>
    {:else if !assembled}
      <p class="warn">Сначала соберите исходный код.</p>
    {:else if sourceDirty}
      <p class="warn">Исходник изменён после сборки — пересоберите.</p>
    {/if}
    <div class="row">
      <button onclick={doProgram} disabled={canProgram ? undefined : true}>
        {busy ? "Работаю…" : "Программировать + проверить"}
      </button>
      <button onclick={doVerify} disabled={canProgram ? undefined : true}>
        Проверить
      </button>
      <button onclick={doRead} disabled={canConnect ? undefined : true}>
        Прочитать
      </button>
    </div>
  </section>

  {#if status}<p class="status ok">{status}</p>{/if}
  {#if error}<p class="status err">{error}</p>{/if}

  {#if symbols.length > 0}
    <section>
      <h2>Символы</h2>
      <div class="tags">{#each symbols as s}<span class="tag">{s}</span>{/each}</div>
    </section>
  {/if}

  {#if listing}
    <section>
      <h2>Листинг</h2>
      <pre class="code">{listing}</pre>
    </section>
  {/if}

  {#if dump}
    <section>
      <h2>Дамп устройства</h2>
      <h3>Программа (конфиг: {dumpConfig})</h3>
      <pre class="code">{dump}</pre>
      <h3>Data EEPROM</h3>
      <pre class="code">{dumpData}</pre>
    </section>
  {/if}
</main>

<style>
  .wrap {
    max-width: 980px;
    margin: 0 auto;
    padding: 1.5rem;
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    color: #0f0f0f;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  section {
    margin: 1rem 0;
    padding: 0.75rem 1rem;
    border: 1px solid #ddd;
    border-radius: 8px;
  }
  h1 {
    font-size: 1.4rem;
  }
  h2 {
    font-size: 1rem;
    margin: 0 0 0.5rem;
  }
  h3 {
    font-size: 0.85rem;
    margin: 0.6rem 0 0.3rem;
  }
  .row {
    display: flex;
    gap: 0.6rem;
    align-items: center;
    flex-wrap: wrap;
  }
  .src {
    width: 100%;
    height: 8em;
    font-family: ui-monospace, monospace;
    font-size: 0.85rem;
    margin-bottom: 0.5rem;
  }
  .wide {
    flex: 1;
    min-width: 240px;
  }
  .osc {
    width: 6em;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  .badge {
    font-size: 0.75rem;
    background: #f0f0f0;
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
  }
  .badge.ok {
    background: #d4edda;
    color: #1a7f37;
  }
  .ok {
    color: #1a7f37;
  }
  .err {
    color: #b02a37;
  }
  .warn {
    color: #9a6700;
    font-size: 0.8rem;
  }
  .status {
    padding: 0.4rem 0.8rem;
    border-radius: 6px;
  }
  .status.ok {
    background: #eaf6ee;
  }
  .status.err {
    background: #fdeaea;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  .tag {
    background: #f0f0f0;
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
    font-family: ui-monospace, monospace;
    font-size: 0.75rem;
  }
  .code {
    max-height: 320px;
    overflow: auto;
    background: #fafafa;
    border: 1px solid #eee;
    padding: 0.6rem;
    border-radius: 6px;
    font-size: 0.75rem;
    line-height: 1.35;
  }
  button:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }
</style>