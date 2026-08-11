<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

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
  let source = $state("");
  const demoSource = `; KR1878VE1 demo
; infinite loop: count on %a1, jump back
start:  inc     %a1
        jmp     start
`;
  let backends = $state<[string, string][]>([]);
  let backend = $state("mock");
  let simState = $state("");
  let connection = $state("");
  let oscHex = $state("");
  let erase = $state(true);

  let wordCount = $state(0);
  let symbols = $state<string[]>([]);
  let listing = $state("");
  let assembledWords = $state<number[]>([]);

  let status = $state("");
  let dump = $state("");

  // ---- helpers ----------------------------------------------------------
  function fmtWords(program: number[]): string {
    const lines: string[] = [];
    for (let i = 0; i < program.length; i += 8) {
      const row = program
        .slice(i, i + 8)
        .map((w) => w.toString(16).padStart(4, "0").toUpperCase())
        .join(" ");
      lines.push(row);
    }
    return lines.join("\n");
  }

  function spec(): SessionSpec {
    const oscHex_ = oscHex.trim();
    return {
      program: assembledWords,
      data: null,
      config: null,
      erase,
      osc: oscHex_ ? parseInt(oscHex_, 16) : null,
    };
  }

  // ---- actions ----------------------------------------------------------
  async function loadBackends() {
    backends = await invoke<[string, string][]>("list_backends");
  }

  async function connectBackend() {
    connection = await invoke<string>("connect", {
      backend,
      simState: simState || null,
    });
    status = "";
  }

  async function doAssemble() {
    const r = await invoke<AssembleResult>("assemble", { source });
    wordCount = r.words.length;
    symbols = r.symbols;
    listing = r.listing;
    assembledWords = r.words;
    status = `assembled: ${r.words.length} words, ${r.symbols.length} symbols`;
  }

  async function doProgram() {
    const ok = await invoke<boolean>("program", { spec: spec() });
    status = ok ? "program + verify: OK" : "program + verify: FAILED";
  }

  async function doVerify() {
    const ok = await invoke<boolean>("verify", { spec: spec() });
    status = ok ? "verify: OK" : "verify: FAILED";
  }

  async function doRead() {
    const r = await invoke<ImageContents>("read");
    dump = fmtWords(r.program);
    status = `read: ${r.program.length} words, config=${
      r.config === null ? "none" : "0x" + r.config.toString(16).toUpperCase()
    }`;
  }

  $effect(() => {
    loadBackends();
  });
</script>

<main class="wrap">
  <h1>KR1878VE1 Toolkit</h1>

  <section>
    <h2>Device</h2>
    <div class="row">
      <select bind:value={backend}>
        {#each backends as [id, label]}
          <option value={id}>{label}</option>
        {/each}
      </select>
      {#if backend === 'sim'}
        <input
          placeholder="sim state file (.sim, optional)"
          bind:value={simState}
          class="wide"
        />
      {/if}
      <button onclick={connectBackend}>Connect</button>
    </div>
    {#if connection}<p class="ok">{connection}</p>{/if}
  </section>

  <section>
    <h2>Source (.mic)</h2>
    <textarea class="src" placeholder={demoSource} bind:value={source}></textarea>
    <div class="row">
      <button onclick={doAssemble}>Assemble</button>
      <span>words: {wordCount} symbols: {symbols.length}</span>
    </div>
  </section>

  <section>
    <h2>Program / Verify</h2>
    <div class="row">
      <label>
        OSC (hex)
        <input class="osc" placeholder="03" bind:value={oscHex} />
      </label>
      <label class="chk">
        <input type="checkbox" bind:checked={erase} />
        erase first
      </label>
    </div>
    <div class="row">
      <button onclick={doProgram}>Program + Verify</button>
      <button onclick={doVerify}>Verify</button>
      <button onclick={doRead}>Read</button>
    </div>
  </section>

  {#if status}<p class="status">{status}</p>{/if}

  {#if symbols.length > 0}
    <section>
      <h2>Symbols</h2>
      <div class="tags">{#each symbols as s}<span class="tag">{s}</span>{/each}</div>
    </section>
  {/if}

  {#if listing}
    <section>
      <h2>Listing</h2>
      <pre class="code">{listing}</pre>
    </section>
  {/if}

  {#if dump}
    <section>
      <h2>Device dump</h2>
      <pre class="code">{dump}</pre>
    </section>
  {/if}
</main>

<style>
  .wrap {
    max-width: 960px;
    margin: 0 auto;
    padding: 1.5rem;
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
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
    width: 5em;
  }
  .chk {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }
  .ok {
    color: #1a7f37;
  }
  .status {
    padding: 0.4rem 0.8rem;
    background: #eef4ff;
    border-radius: 6px;
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
</style>