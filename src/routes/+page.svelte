<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { exists } from '@tauri-apps/plugin-fs';

  // Types matching Rust
  interface WorkshopItem {
    appId: number;
    publishedFileId?: number;
    contentFolder: string;
    previewFile?: string;
    title: string;
    description: string;
    changeNote?: string;
    visibility: 0 | 1 | 2;
    tags: string[];
  }

  interface LogEntry {
    line: string;
    stream: 'stdout' | 'stderr' | 'info';
    ts: string;
  }

  interface UploadResult {
    publishedFileId: number;
    needsLegalAgreement: boolean;
    method: UploadMethod;
  }

  interface SteamClientStatus {
    available: boolean;
    appId: number;
    steamId?: number;
    personaName?: string;
    loggedOn?: boolean;
    error?: string;
  }

  type UploadMethod = 'sdk' | 'steamcmd';

  // State
  let item = $state<WorkshopItem>({
    appId: 252490,
    publishedFileId: undefined,
    contentFolder: '',
    previewFile: '',
    title: '',
    description: '',
    changeNote: '',
    visibility: 0,
    tags: ['Mod'],
  });

  let steamcmdPath = $state('');
  let uploadMethod = $state<UploadMethod>('sdk');
  let generatedVdf = $state('');
  let logs = $state<LogEntry[]>([]);
  let isUploading = $state(false);
  let uploadStatus = $state<'idle' | 'running' | 'success' | 'error'>('idle');
  let lastResult = $state<string | null>(null);
  let steamClientStatus = $state<SteamClientStatus | null>(null);
  let isCheckingSteamClient = $state(false);

  let showSettings = $state(false);
  let tagInput = $state('');

  let unlistenLog: UnlistenFn | null = null;
  let unlistenComplete: UnlistenFn | null = null;

  const APP_PRESETS = [
    { id: 252490, name: 'Rust' },
    { id: 294100, name: 'RimWorld' },
    { id: 107410, name: 'Arma 3' },
    { id: 221100, name: 'DayZ' },
    { id: 304930, name: 'Unturned' },
    { id: 440, name: 'TF2' },
    { id: 730, name: 'CS2' },
  ];

  const VISIBILITY_LABELS = ['Public', 'Friends Only', 'Private'];

  // Computed
  let isFormValid = $derived(
    item.appId > 0 &&
      item.title.trim().length > 0 &&
      item.contentFolder.trim().length > 0,
  );

  let logText = $derived(logs.map(l => l.line).join('\n'));
  let canUpload = $derived(
    isFormValid &&
      !isUploading &&
      (uploadMethod === 'sdk' || steamcmdPath.trim().length > 0),
  );
  let selectedPreset = $derived(
    APP_PRESETS.find(preset => preset.id === item.appId),
  );

  // Helpers
  function addLog(line: string, stream: LogEntry['stream'] = 'info') {
    logs = [...logs, { line, stream, ts: new Date().toLocaleTimeString() }];
  }

  function clearLogs() {
    logs = [];
  }

  function copyLogs() {
    navigator.clipboard.writeText(logText);
  }

  async function selectContentFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select mod content folder',
    });
    if (selected && typeof selected === 'string') {
      item.contentFolder = selected;
    }
  }

  async function selectPreviewFile() {
    const selected = await open({
      directory: false,
      multiple: false,
      title: 'Select preview image (jpg/png)',
      filters: [{ name: 'Images', extensions: ['jpg', 'jpeg', 'png'] }],
    });
    if (selected && typeof selected === 'string') {
      item.previewFile = selected;
    }
  }

  function setPreset(appId: number) {
    item.appId = appId;
    const preset = APP_PRESETS.find(p => p.id === appId);
    if (!item.title) {
      item.title = `${preset?.name ?? 'Steam'} Mod`;
    }
  }

  async function refreshSteamClientStatus() {
    if (uploadMethod !== 'sdk') return;

    isCheckingSteamClient = true;
    try {
      steamClientStatus = await invoke<SteamClientStatus>(
        'check_steam_client_status',
        {
          appId: item.appId,
        },
      );
    } catch (err: any) {
      steamClientStatus = {
        available: false,
        appId: item.appId,
        error: `Steamworks SDK status is unavailable: ${err}`,
      };
    } finally {
      isCheckingSteamClient = false;
    }
  }

  function addTag() {
    const t = tagInput.trim();
    if (t && !item.tags.includes(t)) {
      item.tags = [...item.tags, t];
    }
    tagInput = '';
  }

  function removeTag(tag: string) {
    item.tags = item.tags.filter(t => t !== tag);
  }

  function buildUploadPayload() {
    return {
      appId: item.appId,
      publishedFileId: item.publishedFileId || undefined,
      contentFolder: item.contentFolder,
      previewFile: item.previewFile || undefined,
      title: item.title,
      description: item.description,
      changeNote: item.changeNote || undefined,
      visibility: item.visibility,
      tags: item.tags,
    };
  }

  async function generateVdf() {
    try {
      const payload = buildUploadPayload();

      const vdf = await invoke<string>('generate_workshop_vdf', {
        item: payload,
      });
      generatedVdf = vdf;

      const tempPath = await invoke<string>('write_temp_vdf', { content: vdf });
      (item as any)._tempVdfPath = tempPath;

      addLog('VDF generated successfully', 'info');
    } catch (err: any) {
      addLog(`Failed to generate VDF: ${err}`, 'stderr');
      alert(`Error: ${err}`);
    }
  }

  function cleanupListeners() {
    if (unlistenLog) {
      unlistenLog();
      unlistenLog = null;
    }
    if (unlistenComplete) {
      unlistenComplete();
      unlistenComplete = null;
    }
  }

  async function prepareUploadListeners() {
    cleanupListeners();
    unlistenLog = await listen<{ line: string; stream: string }>(
      'workshop-log',
      event => {
        const { line, stream } = event.payload;
        addLog(line, stream as any);
      },
    );

    unlistenComplete = await listen<{
      success: boolean;
      code: number | null;
      method?: UploadMethod;
      publishedFileId?: number;
      needsLegalAgreement?: boolean;
    }>('workshop-complete', event => {
      const { success, code, method, publishedFileId, needsLegalAgreement } =
        event.payload;
      isUploading = false;
      uploadStatus = success ? 'success' : 'error';

      if (success && method === 'sdk' && publishedFileId) {
        item.publishedFileId = publishedFileId;
        lastResult = needsLegalAgreement
          ? `SDK upload completed. PublishedFileID: ${publishedFileId}. Accept the Workshop legal agreement in Steam.`
          : `SDK upload completed. PublishedFileID: ${publishedFileId}`;
      } else {
        lastResult = success
          ? 'Upload completed successfully!'
          : `Upload failed (exit code ${code ?? 'unknown'})`;
      }

      addLog(
        success
          ? 'Upload finished successfully.'
          : `Upload failed with code ${code}`,
        success ? 'info' : 'stderr',
      );
      cleanupListeners();
    });
  }

  async function startSteamcmdUpload() {
    if (!steamcmdPath) {
      alert('Please set the path to steamcmd in Settings first.');
      showSettings = true;
      return;
    }
    if (!generatedVdf || !(item as any)._tempVdfPath) {
      await generateVdf();
    }

    const vdfPath = (item as any)._tempVdfPath as string;
    if (!vdfPath) {
      alert('Generate the VDF first.');
      return;
    }

    logs = [];
    isUploading = true;
    uploadStatus = 'running';
    lastResult = null;

    addLog('Starting Steam Workshop upload via steamcmd...', 'info');

    try {
      await prepareUploadListeners();
      await invoke('start_workshop_upload', {
        steamcmdPath,
        vdfPath,
      });
    } catch (err: any) {
      isUploading = false;
      uploadStatus = 'error';
      addLog(`Failed to start upload: ${err}`, 'stderr');
      cleanupListeners();
      alert(`Upload error: ${err}`);
    }
  }

  async function startSdkUpload() {
    logs = [];
    isUploading = true;
    uploadStatus = 'running';
    lastResult = null;

    addLog('Starting Steam Workshop upload via Steamworks SDK...', 'info');

    try {
      await prepareUploadListeners();
      const result = await invoke<UploadResult>('upload_via_steamworks', {
        item: buildUploadPayload(),
      });

      isUploading = false;
      item.publishedFileId = result.publishedFileId;
      uploadStatus = 'success';
      lastResult = result.needsLegalAgreement
        ? `SDK upload completed. PublishedFileID: ${result.publishedFileId}. Accept the Workshop legal agreement in Steam.`
        : `SDK upload completed. PublishedFileID: ${result.publishedFileId}`;
    } catch (err: any) {
      isUploading = false;
      uploadStatus = 'error';
      addLog(`SDK upload failed: ${err}`, 'stderr');
      cleanupListeners();
      alert(`SDK upload error: ${err}`);
    }
  }

  async function startUpload() {
    if (uploadMethod === 'sdk') {
      await startSdkUpload();
    } else {
      await startSteamcmdUpload();
    }
  }

  async function validateSteamcmdPath(path: string) {
    if (!path) return false;
    try {
      return await invoke<boolean>('is_valid_steamcmd', { path });
    } catch {
      return false;
    }
  }

  async function browseForSteamcmd() {
    const selected = await open({
      directory: false,
      multiple: false,
      title: 'Locate steamcmd (steamcmd.sh or steamcmd.exe)',
    });
    if (selected && typeof selected === 'string') {
      const ok = await validateSteamcmdPath(selected);
      if (
        ok ||
        confirm('File name does not look like steamcmd. Use it anyway?')
      ) {
        steamcmdPath = selected;
      }
    }
  }

  async function autoDetectSteamcmd() {
    const candidates: string[] = [];
    const home = (window as any).process?.env?.HOME || '';

    if (navigator.platform.includes('Mac')) {
      candidates.push(
        '/Applications/Steam.app/Contents/MacOS/steamcmd',
        `${home}/Library/Application Support/Steam/steamcmd/steamcmd.sh`,
        `${home}/steamcmd/steamcmd.sh`,
      );
    } else if (navigator.platform.includes('Win')) {
      candidates.push(
        'C:\\steamcmd\\steamcmd.exe',
        'C:\\Program Files (x86)\\Steam\\steamcmd\\steamcmd.exe',
      );
    } else {
      candidates.push(
        `${home}/.steam/steamcmd/steamcmd.sh`,
        `${home}/steamcmd/steamcmd.sh`,
        '/usr/games/steamcmd',
      );
    }

    for (const candidate of candidates) {
      try {
        if (await exists(candidate)) {
          const ok = await validateSteamcmdPath(candidate);
          if (ok) {
            steamcmdPath = candidate;
            addLog(`Auto-detected steamcmd at ${candidate}`, 'info');
            return true;
          }
        }
      } catch (_) {}
    }
    addLog(
      'Could not auto-detect steamcmd. Please set the path manually.',
      'info',
    );
    showSettings = true;
    return false;
  }

  function resetForm() {
    item = {
      appId: 252490,
      publishedFileId: undefined,
      contentFolder: '',
      previewFile: '',
      title: '',
      description: '',
      changeNote: '',
      visibility: 0,
      tags: ['Mod'],
    };
    generatedVdf = '';
    logs = [];
    uploadStatus = 'idle';
    lastResult = null;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (
      (e.metaKey || e.ctrlKey) &&
      e.key.toLowerCase() === 'enter' &&
      isFormValid &&
      !isUploading
    ) {
      e.preventDefault();
      if (uploadMethod === 'sdk' || generatedVdf) {
        startUpload();
      } else {
        generateVdf().then(() => startUpload());
      }
    }
  }

  $effect(() => {
    const saved = localStorage.getItem('steamcmdPath');
    if (saved) steamcmdPath = saved;
    const savedMethod = localStorage.getItem('uploadMethod');
    if (savedMethod === 'sdk' || savedMethod === 'steamcmd') {
      uploadMethod = savedMethod;
    }
  });

  $effect(() => {
    if (steamcmdPath) localStorage.setItem('steamcmdPath', steamcmdPath);
  });

  $effect(() => {
    localStorage.setItem('uploadMethod', uploadMethod);
  });

  $effect(() => {
    if (showSettings && uploadMethod === 'sdk') {
      refreshSteamClientStatus();
    }
  });

  $effect(() => {
    return () => {
      cleanupListeners();
    };
  });

  // === Modern additions: Drag & Drop + Log filter ===
  let isDragging = $state(false);
  let logFilter = $state<'all' | 'stdout' | 'stderr' | 'info'>('all');

  let filteredLogs = $derived(
    logFilter === 'all' ? logs : logs.filter(l => l.stream === logFilter),
  );

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave() {
    isDragging = false;
  }

  async function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;

    const file = e.dataTransfer?.files?.[0];
    if (!file) return;

    // If it's a directory drop (Tauri gives us path via special handling in real drops)
    // For simplicity we ask the user to confirm or use the native dialog as fallback.
    // Real directory drops in webview are limited — we still use the dialog for reliability.
    if (file.name && !file.type) {
      // Likely a folder — fall back to dialog for accuracy
    }
    await selectContentFolder();
  }

  function setLogFilter(f: typeof logFilter) {
    logFilter = f;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div
  class="h-dvh max-h-dvh overflow-hidden bg-zinc-950 text-zinc-200 flex flex-col pt-(--macos-titlebar-offset)"
>
  <!-- Navbar is already in the full design above in previous attempt; we need a minimal navbar here -->
  <nav
    class="h-14 shrink-0 border-b border-zinc-800 bg-zinc-950/95 backdrop-blur flex items-center px-6 justify-between text-sm"
  >
    <div class="flex items-center gap-2">
      <div
        class="w-6 h-6 bg-blue-600 rounded-md flex items-center justify-center text-[10px] font-bold"
      >
        SW
      </div>
      <span class="font-semibold">Steam Workshop Uploader</span>
    </div>
    <div class="flex items-center gap-2">
      <div
        class="px-3 py-0.5 text-xs bg-zinc-900 border border-zinc-700 rounded-full text-blue-400 flex items-center gap-1.5"
      >
        <span class="w-1.5 h-1.5 bg-blue-400 rounded-full"></span>
        {uploadMethod === 'sdk' ? 'SDK default' : 'steamcmd mode'}
      </div>
      {#if uploadMethod === 'steamcmd' && steamcmdPath}
        <div
          class="px-3 py-0.5 text-xs bg-zinc-900 border border-zinc-700 rounded-full text-emerald-400 flex items-center gap-1.5"
        >
          <span class="w-1.5 h-1.5 bg-emerald-400 rounded-full"></span> steamcmd
          ready
        </div>
      {/if}
      <button
        onclick={() => (showSettings = true)}
        class="btn-secondary text-xs">Settings</button
      >
      <button onclick={resetForm} class="btn-secondary text-xs">Reset</button>
    </div>
  </nav>

  <div class="flex-1 min-h-0 overflow-y-auto">
    <!-- Modern two-column layout -->
    <div class="flex gap-6 p-6 max-w-360 mx-auto w-full">
      <!-- Form Column -->
      <div class="flex-1 max-w-3xl space-y-6">
        <!-- Target -->
        <div class="bg-zinc-900 border border-zinc-800 rounded-2xl p-6">
          <div class="flex items-center justify-between mb-4">
            <div>
              <div class="text-sm font-semibold text-zinc-400 tracking-wider">
                TARGET
              </div>
              <div class="text-lg font-semibold">Steam App</div>
            </div>
            <div
              class="text-xs px-2 py-1 bg-zinc-950 border border-zinc-800 rounded font-mono text-zinc-500"
            >
              AppID
            </div>
          </div>

          <div class="grid grid-cols-[minmax(0,1fr)_8rem] gap-3 items-end">
            <div>
              <label
                for="game-select"
                class="text-xs text-zinc-500 block mb-1.5">Game</label
              >
              <select
                id="game-select"
                bind:value={item.appId}
                onchange={e =>
                  setPreset(
                    Number((e.currentTarget as HTMLSelectElement).value),
                  )}
                class="path-input w-full text-sm bg-zinc-900"
              >
                {#each APP_PRESETS as preset}
                  <option value={preset.id}>{preset.name}</option>
                {/each}
              </select>
            </div>
            <div>
              <label for="app-id" class="text-xs text-zinc-500 block mb-1.5"
                >App ID</label
              >
              <input
                id="app-id"
                type="number"
                bind:value={item.appId}
                class="path-input w-full text-lg font-medium"
              />
            </div>
          </div>

          <div class="mt-3 text-xs text-zinc-500">
            {#if selectedPreset}
              Selected preset: <span class="text-zinc-300"
                >{selectedPreset.name}</span
              >
            {:else}
              Custom AppID
            {/if}
          </div>

          <div class="mt-3">
            <label
              for="published-file-id"
              class="text-xs text-zinc-500 block mb-1.5"
              >Published File ID (for updates)</label
            >
            <input
              id="published-file-id"
              type="number"
              placeholder="Leave empty for new upload"
              bind:value={item.publishedFileId}
              class="path-input w-full"
            />
          </div>
        </div>

        <!-- Content -->
        <div
          role="region"
          aria-label="Content folder drop zone"
          class="bg-zinc-900 border border-zinc-800 rounded-2xl p-6 {isDragging
            ? 'ring-2 ring-blue-500 border-blue-500/60'
            : ''}"
          ondragover={e => {
            e.preventDefault();
            isDragging = true;
          }}
          ondragleave={() => (isDragging = false)}
          ondrop={e => {
            e.preventDefault();
            handleDrop(e);
          }}
        >
          <div class="text-sm font-semibold text-zinc-400 tracking-wider mb-3">
            CONTENT FOLDER
          </div>

          <button
            type="button"
            onclick={selectContentFolder}
            class="group w-full border border-dashed border-zinc-700 hover:border-zinc-500 rounded-2xl p-7 flex flex-col items-center justify-center cursor-pointer transition-all bg-zinc-950/60 hover:bg-zinc-950 active:scale-[0.995]"
          >
            <div
              class="w-10 h-10 rounded-2xl bg-zinc-800 group-hover:bg-zinc-700 flex items-center justify-center mb-3"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                class="w-5 h-5 text-zinc-400"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
                ><path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  stroke-width="2"
                  d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z"
                /></svg
              >
            </div>
            <div class="font-medium text-sm">
              Drop folder here or click to browse
            </div>
            <div class="text-xs text-zinc-500 mt-1">
              All files inside this folder will be uploaded
            </div>
          </button>

          {#if item.contentFolder}
            <div
              class="mt-3 text-xs font-mono bg-zinc-950 border border-emerald-900/60 text-emerald-400 rounded-xl px-4 py-2 truncate"
            >
              {item.contentFolder}
            </div>
          {/if}
        </div>

        <!-- Preview + Metadata -->
        <div
          class="bg-zinc-900 border border-zinc-800 rounded-2xl p-6 space-y-6"
        >
          <div>
            <div
              class="text-sm font-semibold text-zinc-400 tracking-wider mb-2"
            >
              PREVIEW IMAGE
            </div>
            <button
              onclick={selectPreviewFile}
              class="w-full border border-dashed border-zinc-700 hover:border-zinc-500 rounded-2xl py-4 text-sm flex items-center justify-center gap-2 text-zinc-400 transition-colors"
            >
              Choose preview image (jpg/png)
            </button>
            {#if item.previewFile}<div
                class="text-[11px] mt-1.5 font-mono text-emerald-400 truncate"
              >
                {item.previewFile}
              </div>{/if}
          </div>

          <div>
            <div
              class="text-sm font-semibold text-zinc-400 tracking-wider mb-1.5"
            >
              TITLE <span class="text-red-400">*</span>
            </div>
            <input
              type="text"
              bind:value={item.title}
              placeholder="My Awesome Mod"
              class="path-input w-full"
            />
          </div>

          <div>
            <div
              class="text-sm font-semibold text-zinc-400 tracking-wider mb-1.5"
            >
              DESCRIPTION
            </div>
            <textarea
              bind:value={item.description}
              rows={3}
              placeholder="What does this mod do?"
              class="path-input w-full"
            ></textarea>
          </div>

          <div>
            <div
              class="text-sm font-semibold text-zinc-400 tracking-wider mb-1.5"
            >
              CHANGE NOTE
            </div>
            <textarea
              bind:value={item.changeNote}
              rows={2}
              placeholder="v1.3 - Fixed critical bug"
              class="path-input w-full"
            ></textarea>
          </div>
        </div>

        <!-- Publishing -->
        <div class="bg-zinc-900 border border-zinc-800 rounded-2xl p-6">
          <div class="grid grid-cols-2 gap-x-8 gap-y-6">
            <div>
              <div
                class="text-sm font-semibold text-zinc-400 tracking-wider mb-2.5"
              >
                VISIBILITY
              </div>
              <div class="segmented w-full">
                <button
                  onclick={() => (item.visibility = 0)}
                  class:active={item.visibility === 0}>Public</button
                >
                <button
                  onclick={() => (item.visibility = 1)}
                  class:active={item.visibility === 1}>Friends</button
                >
                <button
                  onclick={() => (item.visibility = 2)}
                  class:active={item.visibility === 2}>Private</button
                >
              </div>
            </div>

            <div>
              <div
                class="text-sm font-semibold text-zinc-400 tracking-wider mb-2"
              >
                TAGS
              </div>
              <div class="flex flex-wrap gap-1 mb-2 min-h-6.5">
                {#each item.tags as tag}
                  <div class="tag-chip text-xs">
                    {tag}
                    <button onclick={() => removeTag(tag)}>×</button>
                  </div>
                {/each}
              </div>
              <input
                bind:value={tagInput}
                onkeydown={e =>
                  e.key === 'Enter' && (e.preventDefault(), addTag())}
                placeholder="Add tag + Enter"
                class="path-input w-full text-xs py-1.5"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Right Review Panel -->
      <div class="w-96 shrink-0">
        <div class="sticky top-6 space-y-4">
          <div class="bg-zinc-900 border border-zinc-800 rounded-3xl p-6">
            <div
              class="uppercase text-xs font-semibold tracking-widest text-zinc-500 mb-4"
            >
              Review
            </div>
            <div class="space-y-3.25 text-sm">
              <div class="flex justify-between">
                <span class="text-zinc-400">App ID</span><span class="font-mono"
                  >{item.appId}</span
                >
              </div>
              <div class="flex justify-between">
                <span class="text-zinc-400">Method</span><span
                  >{uploadMethod === 'sdk'
                    ? 'Steamworks SDK'
                    : 'steamcmd'}</span
                >
              </div>
              <div class="flex justify-between">
                <span class="text-zinc-400">Title</span><span
                  class="truncate max-w-45 text-right"
                  >{item.title || '—'}</span
                >
              </div>
              <div class="flex justify-between">
                <span class="text-zinc-400">Visibility</span><span
                  >{VISIBILITY_LABELS[item.visibility]}</span
                >
              </div>
              <div class="flex justify-between">
                <span class="text-zinc-400">Content</span><span
                  >{item.contentFolder ? 'Ready' : '—'}</span
                >
              </div>
            </div>
          </div>

          <div
            class="bg-zinc-900 border border-zinc-800 rounded-3xl p-6 space-y-3"
          >
            {#if uploadMethod === 'steamcmd'}
              <button
                onclick={generateVdf}
                disabled={!isFormValid || isUploading}
                class="w-full py-3.25 font-semibold rounded-2xl bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 disabled:opacity-50 transition active:scale-[0.985]"
              >
                Generate workshop.vdf
              </button>
            {/if}

            <button
              onclick={startUpload}
              disabled={!canUpload}
              class="btn-primary py-4 text-[15px]"
            >
              {#if isUploading}
                <span class="inline-flex items-center gap-2">
                  <span
                    class="w-3.5 h-3.5 border-2 border-white/30 border-t-white rounded-full animate-spin"
                  ></span>
                  Uploading…
                </span>
              {:else}
                ↑ Upload with {uploadMethod === 'sdk'
                  ? 'Steamworks SDK'
                  : 'steamcmd'}
              {/if}
            </button>
            <div class="text-center text-[10px] text-zinc-500">⌘ + Enter</div>
          </div>

          {#if lastResult}
            <div
              class="rounded-2xl px-4 py-3 text-sm font-medium border {uploadStatus ===
              'success'
                ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400'
                : 'bg-red-500/10 border-red-500/30 text-red-400'}"
            >
              {lastResult}
            </div>
          {/if}

          {#if generatedVdf}
            <div
              class="bg-zinc-900 border border-zinc-800 rounded-3xl overflow-hidden"
            >
              <div
                class="px-4 py-2 text-xs flex justify-between items-center bg-zinc-950 border-b border-zinc-800"
              >
                <span class="font-medium text-zinc-400">workshop.vdf</span>
                <button
                  onclick={() => navigator.clipboard.writeText(generatedVdf)}
                  class="text-xs hover:text-white transition">Copy</button
                >
              </div>
              <pre
                class="text-[10.5px] p-3.5 max-h-52 overflow-auto bg-black/30 text-zinc-400"><code
                  >{generatedVdf}</code
                ></pre>
            </div>
          {/if}
        </div>
      </div>
    </div>

    <!-- Console -->
    <div class="border-t border-zinc-800 bg-zinc-950">
      <div class="max-w-360 mx-auto px-6">
        <div class="flex justify-between items-center py-2.5">
          <div class="flex items-center gap-3">
            <span class="font-semibold text-sm tracking-tight">Console</span>
            {#if isUploading}
              <span
                class="text-xs px-2.5 py-px rounded bg-blue-500/10 text-blue-400 border border-blue-500/20"
                >STREAMING</span
              >
            {/if}
          </div>

          <div class="flex gap-1.5 items-center text-xs">
            <div
              class="flex bg-zinc-900 border border-zinc-800 rounded-lg p-px"
            >
              <button
                onclick={() => setLogFilter('all')}
                class="px-2.5 py-1 rounded {logFilter === 'all'
                  ? 'bg-zinc-800'
                  : ''}">All</button
              >
              <button
                onclick={() => setLogFilter('stdout')}
                class="px-2.5 py-1 rounded {logFilter === 'stdout'
                  ? 'bg-zinc-800'
                  : ''}">Output</button
              >
              <button
                onclick={() => setLogFilter('stderr')}
                class="px-2.5 py-1 rounded {logFilter === 'stderr'
                  ? 'bg-zinc-800'
                  : ''}">Errors</button
              >
            </div>
            <button
              onclick={copyLogs}
              class="btn-secondary text-xs py-1 px-3"
              disabled={logs.length === 0}>Copy</button
            >
            <button
              onclick={clearLogs}
              class="btn-secondary text-xs py-1 px-3"
              disabled={logs.length === 0}>Clear</button
            >
          </div>
        </div>

        <div
          class="h-56 bg-black/50 border border-zinc-800 rounded-2xl mb-6 overflow-hidden font-mono text-sm"
        >
          {#if filteredLogs.length === 0}
            <div
              class="h-full flex items-center justify-center text-zinc-500 text-xs"
            >
              Upload output will stream here
            </div>
          {:else}
            <div class="overflow-auto h-full p-2 text-xs leading-[1.35]">
              {#each filteredLogs as entry}
                <div class="log-line" data-stream={entry.stream}>
                  <span class="text-zinc-600 mr-2.5 select-none"
                    >{entry.ts}</span
                  >{entry.line}
                </div>
              {/each}
            </div>
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>

<!-- Settings Modal -->
{#if showSettings}
  <div
    class="fixed inset-0 z-200 flex items-center justify-center pointer-events-none"
  >
    <button
      type="button"
      aria-label="Close settings"
      class="absolute inset-0 bg-black/80 pointer-events-auto"
      onclick={() => (showSettings = false)}
    ></button>
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="settings-title"
      class="relative pointer-events-auto w-full max-w-md bg-zinc-900 border border-zinc-700 rounded-3xl p-7"
    >
      <div id="settings-title" class="text-xl font-semibold mb-5">Settings</div>

      <div class="mb-6">
        <div class="block text-sm text-zinc-400 mb-1.5">Upload method</div>
        <div class="segmented w-full">
          <button
            onclick={() => (uploadMethod = 'sdk')}
            class:active={uploadMethod === 'sdk'}>Steamworks SDK</button
          >
          <button
            onclick={() => (uploadMethod = 'steamcmd')}
            class:active={uploadMethod === 'steamcmd'}>steamcmd</button
          >
        </div>
        <div class="mt-2 text-xs text-zinc-500">
          {uploadMethod === 'sdk'
            ? 'Uses the running Steam client session. Steam must be open and logged in.'
            : 'Uses steamcmd credentials cached from a prior terminal login.'}
        </div>
      </div>

      {#if uploadMethod === 'sdk'}
        <div class="mb-6 rounded-2xl border border-zinc-800 bg-zinc-950/60 p-4">
          <div class="flex items-center justify-between gap-3 mb-3">
            <div>
              <div class="text-sm font-medium text-zinc-300">Steam client</div>
              <div class="text-xs text-zinc-500">
                Checked with AppID {item.appId}
              </div>
            </div>
            <button
              onclick={refreshSteamClientStatus}
              disabled={isCheckingSteamClient}
              class="btn-secondary text-xs py-1.5 px-3"
            >
              {isCheckingSteamClient ? 'Checking...' : 'Refresh'}
            </button>
          </div>

          {#if steamClientStatus?.available}
            <div class="flex items-center gap-2 text-sm text-emerald-400 mb-3">
              <span class="w-2 h-2 rounded-full bg-emerald-400"></span>
              Steam client detected
            </div>
            <div
              class="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-xs"
            >
              <span class="text-zinc-500">User</span>
              <span class="truncate text-zinc-200"
                >{steamClientStatus.personaName || 'Unknown'}</span
              >
              <span class="text-zinc-500">Steam ID</span>
              <span class="font-mono text-zinc-300"
                >{steamClientStatus.steamId ?? 'Unknown'}</span
              >
              <span class="text-zinc-500">Logged on</span>
              <span
                class={steamClientStatus.loggedOn
                  ? 'text-emerald-400'
                  : 'text-amber-400'}
              >
                {steamClientStatus.loggedOn ? 'Yes' : 'No'}
              </span>
            </div>
          {:else if steamClientStatus}
            <div class="flex items-center gap-2 text-sm text-amber-400 mb-2">
              <span class="w-2 h-2 rounded-full bg-amber-400"></span>
              Steam client not detected
            </div>
            <div class="text-xs text-zinc-500 whitespace-pre-line">
              {steamClientStatus.error}
            </div>
          {:else}
            <div class="text-xs text-zinc-500">
              Open settings or refresh to check the running Steam client.
            </div>
          {/if}
        </div>
      {/if}

      {#if uploadMethod === 'steamcmd'}
        <div class="mb-6">
          <label for="steamcmd-path" class="block text-sm text-zinc-400 mb-1.5"
            >steamcmd path</label
          >
          <div class="flex gap-2">
            <input
              id="steamcmd-path"
              bind:value={steamcmdPath}
              class="path-input flex-1"
              placeholder="steamcmd.sh or steamcmd.exe"
            />
            <button onclick={browseForSteamcmd} class="btn-secondary"
              >Browse</button
            >
          </div>
          <button
            onclick={autoDetectSteamcmd}
            class="btn-secondary mt-2 text-xs">Auto-detect steamcmd</button
          >
        </div>

        <div class="text-xs text-zinc-400 border-t border-zinc-800 pt-5">
          For steamcmd fallback, run <span class="font-mono text-amber-400"
            >steamcmd +login YOUR_USERNAME</span
          > once in your terminal to cache credentials.
        </div>
      {/if}

      <div class="mt-6 flex justify-end gap-3">
        <button
          onclick={() => (showSettings = false)}
          class="btn-secondary px-6">Close</button
        >
        <button
          onclick={() => (showSettings = false)}
          class="btn-primary w-auto! px-7 py-2 text-sm">Save</button
        >
      </div>
    </div>
  </div>
{/if}
