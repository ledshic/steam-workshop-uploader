<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { open } from '@tauri-apps/plugin-dialog';
  import { exists } from '@tauri-apps/plugin-fs';
  import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener';

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

  interface DescriptionUpdatePayload {
    appId: number;
    publishedFileId: number;
    description: string;
    language?: string;
    changeNote?: string;
  }

  interface PreviewUpdatePayload {
    appId: number;
    publishedFileId: number;
    previewFile: string;
    changeNote?: string;
  }

  interface QueryWorkshopItemPayload {
    appId: number;
    publishedFileId: number;
    language?: string;
  }

  interface QueriedWorkshopItem {
    appId: number;
    publishedFileId: number;
    title: string;
    description: string;
    visibility: 0 | 1 | 2;
    tags: string[];
  }

  interface SteamClientStatus {
    available: boolean;
    appId: number;
    steamId?: number;
    personaName?: string;
    loggedOn?: boolean;
    error?: string;
  }

  interface RimWorldModInfo {
    modRoot: string;
    contentFolder: string;
    aboutXmlPath: string;
    name: string;
    description: string;
    author?: string;
    packageId?: string;
    url?: string;
    supportedVersions: string[];
    previewFile?: string;
    publishedFileId?: number;
    publishedFileIdPath?: string;
    tags: string[];
    warnings: string[];
    detectedFiles: string[];
    isPackaged: boolean;
  }

  type UploadMethod = 'sdk' | 'steamcmd';

  /** App is specialized for RimWorld Workshop uploads. */
  const RIMWORLD_APP_ID = 294100;
  const APP_PRESETS = [{ id: RIMWORLD_APP_ID, name: 'RimWorld' }];
  const VISIBILITY_LABELS = ['Public', 'Friends Only', 'Private'];
  const ITEM_ID_HISTORY_KEY = 'publishedFileIdHistory';
  const ITEM_ID_HISTORY_MAX = 30;
  const CLEAN_PACKAGE_KEY = 'rimworldCleanPackage';
  const STEAM_LANGUAGE_OPTIONS = [
    { code: 'english', label: 'English' },
    { code: 'schinese', label: 'Simplified Chinese' },
    { code: 'tchinese', label: 'Traditional Chinese' },
    { code: 'japanese', label: 'Japanese' },
    { code: 'koreana', label: 'Korean' },
    { code: 'russian', label: 'Russian' },
    { code: 'french', label: 'French' },
    { code: 'german', label: 'German' },
    { code: 'spanish', label: 'Spanish' },
    { code: 'latam', label: 'Spanish - Latin America' },
    { code: 'brazilian', label: 'Portuguese - Brazil' },
    { code: 'turkish', label: 'Turkish' },
    { code: 'thai', label: 'Thai' },
    { code: 'polish', label: 'Polish' },
    { code: 'ukrainian', label: 'Ukrainian' },
    { code: 'vietnamese', label: 'Vietnamese' },
  ];

  let item = $state<WorkshopItem>({
    appId: RIMWORLD_APP_ID,
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
  let lastSteamStatusCheckedAt = $state<string | null>(null);
  let lastSteamStatusCheckSource = $state<'auto' | 'manual' | null>(null);

  let showSettings = $state(false);
  let tagInput = $state('');
  let publishedFileIdInput = $state('');
  let rememberedItemIds = $state<string[]>([]);
  let descriptionLanguage = $state('english');
  let isQueryingItem = $state(false);
  let isDetectingMod = $state(false);
  let isPackaging = $state(false);
  let cleanPackage = $state(true);
  let modRootPath = $state('');
  let packagePath = $state('');
  let rimworldInfo = $state<RimWorldModInfo | null>(null);
  let hasTempPackage = $derived(
    Boolean(packagePath) || Boolean(rimworldInfo?.isPackaged),
  );

  let unlistenLog: UnlistenFn | null = null;
  let unlistenComplete: UnlistenFn | null = null;

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
  let canUpdateDescription = $derived(
    uploadMethod === 'sdk' &&
      !isUploading &&
      getPublishedFileIdFromInput() !== undefined,
  );
  let canUpdatePreview = $derived(
    uploadMethod === 'sdk' &&
      !isUploading &&
      getPublishedFileIdFromInput() !== undefined &&
      (item.previewFile?.trim().length ?? 0) > 0,
  );
  let canQueryItem = $derived(
    uploadMethod === 'sdk' &&
      !isUploading &&
      !isQueryingItem &&
      getPublishedFileIdFromInput() !== undefined,
  );
  let selectedPreset = $derived(
    APP_PRESETS.find(preset => preset.id === item.appId),
  );

  let isDragging = $state(false);
  let logFilter = $state<'all' | 'stdout' | 'stderr' | 'info'>('all');
  let filteredLogs = $derived(
    logFilter === 'all' ? logs : logs.filter(l => l.stream === logFilter),
  );

  function addLog(line: string, stream: LogEntry['stream'] = 'info') {
    logs = [...logs, { line, stream, ts: new Date().toLocaleTimeString() }];
  }

  function clearLogs() {
    logs = [];
  }

  function copyLogs() {
    navigator.clipboard.writeText(logText);
  }

  function getPublishedFileIdFromInput(): number | undefined {
    const normalized = publishedFileIdInput.trim();
    if (!normalized || !/^\d+$/.test(normalized)) return undefined;

    const asNumber = Number(normalized);
    if (!Number.isSafeInteger(asNumber) || asNumber <= 0) return undefined;
    return asNumber;
  }

  function applyPublishedFileIdInputToModel() {
    item.publishedFileId = getPublishedFileIdFromInput();
  }

  function normalizeLanguageInput(): string | undefined {
    const normalized = descriptionLanguage.trim().toLowerCase();
    return normalized.length > 0 ? normalized : undefined;
  }

  function loadRememberedItemIds() {
    try {
      const raw = localStorage.getItem(ITEM_ID_HISTORY_KEY);
      if (!raw) {
        rememberedItemIds = [];
        return;
      }

      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed)) {
        rememberedItemIds = [];
        return;
      }

      rememberedItemIds = parsed
        .map((value: unknown) => String(value).trim())
        .filter(value => /^\d+$/.test(value) && Number(value) > 0)
        .slice(0, ITEM_ID_HISTORY_MAX);
    } catch {
      rememberedItemIds = [];
    }
  }

  function saveRememberedItemIds() {
    localStorage.setItem(ITEM_ID_HISTORY_KEY, JSON.stringify(rememberedItemIds));
  }

  function rememberPublishedFileId(id: number) {
    const normalized = String(id).trim();
    if (!/^\d+$/.test(normalized) || Number(normalized) <= 0) return;

    rememberedItemIds = [
      normalized,
      ...rememberedItemIds.filter(existing => existing !== normalized),
    ].slice(0, ITEM_ID_HISTORY_MAX);
    saveRememberedItemIds();
  }

  function removeRememberedItemId(id: string) {
    rememberedItemIds = rememberedItemIds.filter(existing => existing !== id);
    saveRememberedItemIds();
  }

  function applyRimWorldInfo(info: RimWorldModInfo, opts?: { keepPackage?: boolean }) {
    rimworldInfo = info;
    modRootPath = info.modRoot;
    item.appId = RIMWORLD_APP_ID;
    item.contentFolder = info.contentFolder;
    item.title = info.name;
    item.description = info.description;
    item.previewFile = info.previewFile || '';
    item.tags = info.tags.length > 0 ? [...info.tags] : ['Mod'];

    if (info.isPackaged) {
      packagePath = info.contentFolder;
    } else if (!opts?.keepPackage) {
      packagePath = '';
    }

    if (info.publishedFileId) {
      item.publishedFileId = info.publishedFileId;
      publishedFileIdInput = String(info.publishedFileId);
      rememberPublishedFileId(info.publishedFileId);
    }

    for (const warning of info.warnings) {
      addLog(`[RimWorld] ${warning}`, 'info');
    }
    addLog(
      `[RimWorld] Detected "${info.name}"` +
        (info.packageId ? ` (${info.packageId})` : '') +
        (info.publishedFileId
          ? ` → update #${info.publishedFileId}`
          : ' → new upload') +
        (info.isPackaged ? ' [temp package]' : ''),
      'info',
    );
  }

  /** Scan mod metadata only — does not create a temp upload package. */
  async function detectRimWorldFromPath(
    path: string,
  ): Promise<RimWorldModInfo | null> {
    isDetectingMod = true;
    try {
      addLog('[RimWorld] Scanning mod structure...', 'info');
      const info = await invoke<RimWorldModInfo>('detect_rimworld_mod', { path });
      applyRimWorldInfo(info);
      return info;
    } catch (err: any) {
      addLog(`[RimWorld] Detect failed: ${err}`, 'stderr');
      item.contentFolder = path;
      modRootPath = path;
      packagePath = '';
      rimworldInfo = null;
      alert(`RimWorld detect failed: ${err}`);
      return null;
    } finally {
      isDetectingMod = false;
    }
  }

  /**
   * One-click: build a clean temp directory for Workshop upload
   * (excludes Source / .git / build artifacts), then open it in the file manager.
   */
  async function generateTempPackage(
    options: { openAfter?: boolean } = { openAfter: true },
  ): Promise<RimWorldModInfo | null> {
    const path = modRootPath || item.contentFolder;
    if (!path) {
      alert('Please select a RimWorld mod folder first.');
      return null;
    }
    if (isPackaging || isDetectingMod) return null;

    isPackaging = true;
    try {
      addLog(
        '[RimWorld] Generating temp upload package (excluding Source/VCS/build)...',
        'info',
      );
      const info = await invoke<RimWorldModInfo>('prepare_rimworld_package', {
        req: { modRoot: path },
      });
      applyRimWorldInfo(info);
      packagePath = info.contentFolder;
      addLog(`[RimWorld] Temp package ready: ${info.contentFolder}`, 'info');

      if (options.openAfter !== false) {
        await openPackageInFileManager(info.contentFolder);
      }
      return info;
    } catch (err: any) {
      addLog(`[RimWorld] Package failed: ${err}`, 'stderr');
      alert(`Failed to generate temp package: ${err}`);
      return null;
    } finally {
      isPackaging = false;
    }
  }

  async function openPackageInFileManager(targetPath?: string) {
    const path = targetPath || packagePath || item.contentFolder;
    if (!path) {
      alert('No package directory to open.');
      return;
    }
    try {
      // Prefer opening the folder itself in Finder / Explorer / file manager.
      await openPath(path);
      addLog(`[RimWorld] Opened package folder: ${path}`, 'info');
    } catch (openErr: any) {
      try {
        await revealItemInDir(path);
        addLog(`[RimWorld] Revealed package in file manager: ${path}`, 'info');
      } catch (revealErr: any) {
        addLog(
          `[RimWorld] Could not open file manager: ${openErr}; ${revealErr}`,
          'stderr',
        );
        alert(`Could not open file manager:\n${openErr}`);
      }
    }
  }

  async function selectContentFolder() {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select RimWorld mod folder (contains About/)',
    });
    if (selected && typeof selected === 'string') {
      await detectRimWorldFromPath(selected);
    }
  }

  async function rescanRimWorldMod() {
    const path = modRootPath || item.contentFolder;
    if (!path) {
      await selectContentFolder();
      return;
    }
    // Rescan metadata only; keep existing temp package if any until regenerated.
    const previousPackage = packagePath;
    const info = await detectRimWorldFromPath(path);
    if (info && previousPackage && !info.isPackaged) {
      // Restore upload target to previous package if still desired
      // User must regenerate to refresh package contents.
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
    item.appId = appId || RIMWORLD_APP_ID;
  }


  async function persistPublishedFileId(id: number) {
    const root = modRootPath || rimworldInfo?.modRoot || item.contentFolder;
    if (!root || !id) return;
    try {
      const written = await invoke<string>('write_rimworld_published_file_id', {
        req: {
          modRoot: root,
          publishedFileId: id,
        },
      });
      addLog(`[RimWorld] Wrote PublishedFileId.txt → ${written}`, 'info');
    } catch (err: any) {
      addLog(`[RimWorld] Could not write PublishedFileId.txt: ${err}`, 'stderr');
    }
  }

  async function refreshSteamClientStatus(source: 'auto' | 'manual' = 'manual') {
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
      lastSteamStatusCheckedAt = new Date().toLocaleTimeString();
      lastSteamStatusCheckSource = source;
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
    const parsedPublishedFileId = getPublishedFileIdFromInput();
    item.publishedFileId = parsedPublishedFileId;

    return {
      appId: item.appId,
      publishedFileId: parsedPublishedFileId,
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
        publishedFileIdInput = String(publishedFileId);
        rememberPublishedFileId(publishedFileId);
        void persistPublishedFileId(publishedFileId);
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
      const payload = buildUploadPayload();
      if (payload.publishedFileId) {
        rememberPublishedFileId(payload.publishedFileId);
      }

      const result = await invoke<UploadResult>('upload_via_steamworks', {
        item: payload,
      });

      isUploading = false;
      item.publishedFileId = result.publishedFileId;
      publishedFileIdInput = String(result.publishedFileId);
      rememberPublishedFileId(result.publishedFileId);
      await persistPublishedFileId(result.publishedFileId);
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

  /** One-click: pick folder if needed → detect → temp package → upload/update. */
  async function oneClickRimWorldUpload() {
    if (isUploading || isDetectingMod || isPackaging) return;

    let path = modRootPath || item.contentFolder;
    if (!path) {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select RimWorld mod folder (contains About/)',
      });
      if (!selected || typeof selected !== 'string') return;
      path = selected;
      const detected = await detectRimWorldFromPath(path);
      if (!detected) return;
      path = modRootPath || path;
    }

    // Prefer a fresh clean package for upload when enabled.
    if (cleanPackage) {
      const packaged = await generateTempPackage({ openAfter: false });
      if (!packaged) return;
    } else if (!item.title.trim() || !item.contentFolder.trim()) {
      const detected = await detectRimWorldFromPath(path);
      if (!detected) return;
    }

    if (!item.title.trim() || !item.contentFolder.trim()) {
      alert('RimWorld mod detection did not produce a valid title/content folder.');
      return;
    }

    await startUpload();
  }

  async function updateDescriptionOnly() {
    const publishedFileId = getPublishedFileIdFromInput();
    if (!publishedFileId) {
      alert('Please provide a valid Published File ID.');
      return;
    }

    const payload: DescriptionUpdatePayload = {
      appId: item.appId,
      publishedFileId,
      description: item.description,
      language: normalizeLanguageInput(),
      changeNote: item.changeNote || undefined,
    };
    rememberPublishedFileId(publishedFileId);

    logs = [];
    isUploading = true;
    uploadStatus = 'running';
    lastResult = null;

    addLog('Starting description-only update via Steamworks SDK...', 'info');

    try {
      await prepareUploadListeners();
      const result = await invoke<UploadResult>('update_description_via_steamworks', {
        req: payload,
      });

      isUploading = false;
      item.publishedFileId = result.publishedFileId;
      publishedFileIdInput = String(result.publishedFileId);
      rememberPublishedFileId(result.publishedFileId);
      uploadStatus = 'success';
      lastResult = result.needsLegalAgreement
        ? `Description updated. PublishedFileID: ${result.publishedFileId}. Accept the Workshop legal agreement in Steam.`
        : `Description updated. PublishedFileID: ${result.publishedFileId}`;
    } catch (err: any) {
      isUploading = false;
      uploadStatus = 'error';
      addLog(`Description update failed: ${err}`, 'stderr');
      cleanupListeners();
      alert(`Description update error: ${err}`);
    }
  }

  async function updatePreviewOnly() {
    const publishedFileId = getPublishedFileIdFromInput();
    if (!publishedFileId) {
      alert('Please provide a valid Published File ID.');
      return;
    }
    if (!item.previewFile?.trim()) {
      alert('Please choose a preview image first.');
      return;
    }

    const payload: PreviewUpdatePayload = {
      appId: item.appId,
      publishedFileId,
      previewFile: item.previewFile,
      changeNote: item.changeNote || undefined,
    };
    rememberPublishedFileId(publishedFileId);

    logs = [];
    isUploading = true;
    uploadStatus = 'running';
    lastResult = null;

    addLog('Starting preview-image-only update via Steamworks SDK...', 'info');

    try {
      await prepareUploadListeners();
      const result = await invoke<UploadResult>('update_preview_via_steamworks', {
        req: payload,
      });

      isUploading = false;
      item.publishedFileId = result.publishedFileId;
      publishedFileIdInput = String(result.publishedFileId);
      rememberPublishedFileId(result.publishedFileId);
      uploadStatus = 'success';
      lastResult = result.needsLegalAgreement
        ? `Preview image updated. PublishedFileID: ${result.publishedFileId}. Accept the Workshop legal agreement in Steam.`
        : `Preview image updated. PublishedFileID: ${result.publishedFileId}`;
    } catch (err: any) {
      isUploading = false;
      uploadStatus = 'error';
      addLog(`Preview image update failed: ${err}`, 'stderr');
      cleanupListeners();
      alert(`Preview image update error: ${err}`);
    }
  }

  async function queryItemAndFillForm() {
    const publishedFileId = getPublishedFileIdFromInput();
    if (!publishedFileId) {
      alert('Please provide a valid Published File ID.');
      return;
    }

    const payload: QueryWorkshopItemPayload = {
      appId: item.appId,
      publishedFileId,
      language: normalizeLanguageInput(),
    };

    isQueryingItem = true;
    addLog(
      `Querying item ${publishedFileId} via Steamworks SDK...`,
      'info',
    );

    try {
      const queried = await invoke<QueriedWorkshopItem>(
        'query_workshop_item_by_id',
        {
          req: payload,
        },
      );

      if (queried.appId !== item.appId) {
        addLog(
          `Queried item belongs to AppID ${queried.appId}; keeping current AppID ${item.appId} to avoid SDK status check crashes.`,
          'info',
        );
      }
      item.publishedFileId = queried.publishedFileId;
      item.title = queried.title;
      item.description = queried.description;
      item.visibility = queried.visibility;
      item.tags = queried.tags;

      publishedFileIdInput = String(queried.publishedFileId);
      rememberPublishedFileId(queried.publishedFileId);

      addLog(
        `Loaded item ${queried.publishedFileId} into form fields.`,
        'info',
      );
    } catch (err: any) {
      addLog(`Item query failed: ${err}`, 'stderr');
      alert(`Item query error: ${err}`);
    } finally {
      isQueryingItem = false;
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
      if (ok || confirm('File name does not look like steamcmd. Use it anyway?')) {
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
      appId: RIMWORLD_APP_ID,
      publishedFileId: undefined,
      contentFolder: '',
      previewFile: '',
      title: '',
      description: '',
      changeNote: '',
      visibility: 0,
      tags: ['Mod'],
    };
    publishedFileIdInput = '';
    generatedVdf = '';
    logs = [];
    uploadStatus = 'idle';
    lastResult = null;
    modRootPath = '';
    packagePath = '';
    rimworldInfo = null;
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

    // Tauri/web file drops usually only expose the file name; fall back to picker.
    // Prefer path when available (desktop webviews sometimes provide it).
    const file = e.dataTransfer?.files?.[0] as File & { path?: string } | undefined;
    if (file?.path) {
      await detectRimWorldFromPath(file.path);
      return;
    }
    await selectContentFolder();
  }

  function setLogFilter(f: typeof logFilter) {
    logFilter = f;
  }

  $effect(() => {
    const saved = localStorage.getItem('steamcmdPath');
    if (saved) steamcmdPath = saved;
    const savedMethod = localStorage.getItem('uploadMethod');
    if (savedMethod === 'sdk' || savedMethod === 'steamcmd') {
      uploadMethod = savedMethod;
    }
    const savedClean = localStorage.getItem(CLEAN_PACKAGE_KEY);
    if (savedClean === '0' || savedClean === 'false') {
      cleanPackage = false;
    } else if (savedClean === '1' || savedClean === 'true') {
      cleanPackage = true;
    }
    // Always default to RimWorld
    item.appId = RIMWORLD_APP_ID;
    loadRememberedItemIds();
  });

  $effect(() => {
    if (steamcmdPath) {
      localStorage.setItem('steamcmdPath', steamcmdPath);
    }
  });

  $effect(() => {
    localStorage.setItem('uploadMethod', uploadMethod);
  });

  $effect(() => {
    localStorage.setItem(CLEAN_PACKAGE_KEY, cleanPackage ? '1' : '0');
  });

  $effect(() => {
    if (showSettings && uploadMethod === 'sdk') {
      refreshSteamClientStatus('auto');
    }
  });

  $effect(() => {
    return () => {
      cleanupListeners();
    };
  });
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
      <span class="text-xs text-zinc-500 hidden sm:inline">· RimWorld</span>
    </div>
    <div class="flex items-center gap-2">
      <div
        class="px-3 py-0.5 text-xs bg-zinc-900 border border-zinc-700 rounded-full text-amber-400 flex items-center gap-1.5"
      >
        <span class="w-1.5 h-1.5 bg-amber-400 rounded-full"></span>
        RimWorld
      </div>
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
              <div class="text-lg font-semibold">RimWorld Workshop</div>
            </div>
            <div
              class="text-xs px-2 py-1 bg-zinc-950 border border-zinc-800 rounded font-mono text-zinc-500"
            >
              AppID {RIMWORLD_APP_ID}
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
                readonly
              />
            </div>
          </div>

          <div class="mt-3 text-xs text-zinc-500">
            Specialized for RimWorld mods: auto-reads
            <span class="text-zinc-300">About/About.xml</span>,
            <span class="text-zinc-300">Preview</span>, and
            <span class="text-zinc-300">PublishedFileId.txt</span>.
          </div>

          <div class="mt-3">
            <label
              for="published-file-id"
              class="text-xs text-zinc-500 block mb-1.5"
              >Published File ID (auto from About/PublishedFileId.txt)</label
            >
            <div class="flex gap-2">
              <input
                id="published-file-id"
                type="text"
                inputmode="numeric"
                pattern="[0-9]*"
                list="published-file-id-history"
                placeholder="Leave empty for new upload"
                bind:value={publishedFileIdInput}
                onblur={applyPublishedFileIdInputToModel}
                class="path-input w-full"
              />
              <button
                type="button"
                onclick={queryItemAndFillForm}
                disabled={!canQueryItem}
                class="btn-secondary px-3 text-xs whitespace-nowrap disabled:opacity-50"
              >
                {isQueryingItem ? 'Querying...' : 'Query & Fill'}
              </button>
            </div>
            <datalist id="published-file-id-history">
              {#each rememberedItemIds as rememberedId}
                <option value={rememberedId}></option>
              {/each}
            </datalist>

            {#if rememberedItemIds.length > 0}
              <div class="mt-2 flex flex-wrap gap-1.5">
                {#each rememberedItemIds as rememberedId}
                  <div
                    class="inline-flex items-center gap-1 rounded-lg border border-zinc-700 bg-zinc-950 px-2 py-1 text-[11px]"
                  >
                    <button
                      type="button"
                      class="font-mono text-zinc-300 hover:text-white"
                      onclick={() => {
                        publishedFileIdInput = rememberedId;
                        applyPublishedFileIdInputToModel();
                      }}>{rememberedId}</button
                    >
                    <button
                      type="button"
                      class="text-zinc-500 hover:text-red-300"
                      aria-label={`Delete remembered ID ${rememberedId}`}
                      onclick={() => removeRememberedItemId(rememberedId)}>×</button
                    >
                  </div>
                {/each}
              </div>
            {/if}
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
          <div class="flex items-center justify-between mb-3">
            <div class="text-sm font-semibold text-zinc-400 tracking-wider">
              RIMWORLD MOD FOLDER
            </div>
            <label class="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer select-none">
              <input
                type="checkbox"
                bind:checked={cleanPackage}
                class="rounded border-zinc-600 bg-zinc-900"
              />
              Upload via temp package
            </label>
          </div>

          <button
            type="button"
            onclick={selectContentFolder}
            disabled={isDetectingMod || isPackaging}
            class="group w-full border border-dashed border-zinc-700 hover:border-zinc-500 rounded-2xl p-7 flex flex-col items-center justify-center cursor-pointer transition-all bg-zinc-950/60 hover:bg-zinc-950 active:scale-[0.995] disabled:opacity-60"
          >
            <div
              class="w-10 h-10 rounded-2xl bg-zinc-800 group-hover:bg-zinc-700 flex items-center justify-center mb-3"
            >
              {#if isDetectingMod}
                <span
                  class="w-5 h-5 border-2 border-zinc-500 border-t-zinc-200 rounded-full animate-spin"
                ></span>
              {:else}
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
              {/if}
            </div>
            <div class="font-medium text-sm">
              {isDetectingMod
                ? 'Scanning About.xml…'
                : 'Drop mod folder or click to browse'}
            </div>
            <div class="text-xs text-zinc-500 mt-1 text-center max-w-sm">
              Auto-fills title, description, preview &amp; Workshop ID from
              <span class="text-zinc-400">About/</span>
            </div>
          </button>

          {#if modRootPath || item.contentFolder}
            <div
              class="mt-3 text-xs font-mono bg-zinc-950 border border-zinc-700 text-zinc-300 rounded-xl px-4 py-2 truncate"
              title={modRootPath || item.contentFolder}
            >
              <span class="text-zinc-500">Mod · </span
              >{modRootPath || item.contentFolder}
            </div>
          {/if}

          <!-- Temp package actions (after mod selected) -->
          {#if modRootPath || item.contentFolder}
            <div
              class="mt-3 rounded-xl border border-zinc-800 bg-zinc-950/80 p-3 space-y-2.5"
            >
              <div class="flex items-center justify-between gap-2">
                <div class="text-xs font-semibold tracking-wide text-zinc-400">
                  TEMP UPLOAD PACKAGE
                </div>
                {#if hasTempPackage}
                  <span
                    class="text-[10px] px-2 py-0.5 rounded-full bg-emerald-500/10 text-emerald-400 border border-emerald-500/25"
                    >Ready</span
                  >
                {:else}
                  <span
                    class="text-[10px] px-2 py-0.5 rounded-full bg-zinc-800 text-zinc-500 border border-zinc-700"
                    >Not generated</span
                  >
                {/if}
              </div>
              <div class="text-[11px] text-zinc-500 leading-relaxed">
                Copies the mod into a clean temp folder (drops Source / .git /
                bin / obj / project files). Upload uses this folder.
              </div>
              <div class="flex gap-2">
                <button
                  type="button"
                  onclick={() => generateTempPackage({ openAfter: true })}
                  disabled={isPackaging || isDetectingMod || isUploading}
                  class="btn-primary text-xs flex-1 py-2.5 disabled:opacity-50"
                >
                  {#if isPackaging}
                    <span class="inline-flex items-center gap-2">
                      <span
                        class="w-3 h-3 border-2 border-white/30 border-t-white rounded-full animate-spin"
                      ></span>
                      Generating…
                    </span>
                  {:else}
                    {hasTempPackage
                      ? 'Regenerate temp package'
                      : 'Generate temp package'}
                  {/if}
                </button>
                <button
                  type="button"
                  onclick={() => openPackageInFileManager()}
                  disabled={!hasTempPackage || isPackaging}
                  class="btn-secondary text-xs px-3 whitespace-nowrap disabled:opacity-50"
                  title="Open temp package in system file manager"
                >
                  Open folder
                </button>
              </div>
              {#if packagePath}
                <button
                  type="button"
                  class="w-full text-left text-[11px] font-mono bg-zinc-900 border border-emerald-900/50 text-emerald-400 rounded-lg px-3 py-2 truncate hover:border-emerald-700 transition"
                  title="Click to open: {packagePath}"
                  onclick={() => openPackageInFileManager(packagePath)}
                >
                  {packagePath}
                </button>
              {/if}
            </div>
          {/if}

          <div class="mt-3 flex gap-2">
            <button
              type="button"
              onclick={rescanRimWorldMod}
              disabled={isDetectingMod ||
                isPackaging ||
                (!modRootPath && !item.contentFolder)}
              class="btn-secondary text-xs flex-1 disabled:opacity-50"
            >
              {isDetectingMod ? 'Scanning…' : 'Rescan mod'}
            </button>
            <button
              type="button"
              onclick={oneClickRimWorldUpload}
              disabled={isUploading || isDetectingMod || isPackaging}
              class="btn-primary text-xs flex-1 py-2 disabled:opacity-50"
            >
              One-click upload / update
            </button>
          </div>

          {#if rimworldInfo}
            <div
              class="mt-3 rounded-xl border border-zinc-800 bg-zinc-950/70 p-3 text-xs space-y-1.5"
            >
              <div class="flex justify-between gap-3">
                <span class="text-zinc-500">packageId</span>
                <span class="text-zinc-300 font-mono truncate"
                  >{rimworldInfo.packageId || '—'}</span
                >
              </div>
              {#if rimworldInfo.author}
                <div class="flex justify-between gap-3">
                  <span class="text-zinc-500">author</span>
                  <span class="text-zinc-300 truncate">{rimworldInfo.author}</span>
                </div>
              {/if}
              <div class="flex justify-between gap-3">
                <span class="text-zinc-500">versions</span>
                <span class="text-zinc-300"
                  >{rimworldInfo.supportedVersions.join(', ') || '—'}</span
                >
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-zinc-500">mode</span>
                <span class="text-zinc-300">
                  {rimworldInfo.publishedFileId
                    ? `Update #${rimworldInfo.publishedFileId}`
                    : 'New upload'}
                  {hasTempPackage ? ' · temp package' : ''}
                </span>
              </div>
              <div class="flex justify-between gap-3">
                <span class="text-zinc-500">upload path</span>
                <span
                  class="text-zinc-300 font-mono truncate max-w-[14rem]"
                  title={item.contentFolder}>{item.contentFolder || '—'}</span
                >
              </div>
              {#if rimworldInfo.warnings.length > 0}
                <div class="pt-1 border-t border-zinc-800 text-amber-400/90 space-y-0.5">
                  {#each rimworldInfo.warnings as warning}
                    <div>· {warning}</div>
                  {/each}
                </div>
              {/if}
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
              PREVIEW / COVER
            </div>
            <button
              onclick={selectPreviewFile}
              class="w-full border border-dashed border-zinc-700 hover:border-zinc-500 rounded-2xl py-4 text-sm flex items-center justify-center gap-2 text-zinc-400 transition-colors"
            >
              {item.previewFile
                ? 'Change preview image'
                : 'Auto: About/Preview.png (or pick)'}
            </button>
            {#if item.previewFile}<div
                class="text-[11px] mt-1.5 font-mono text-emerald-400 truncate"
                title={item.previewFile}
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
              rows={6}
              placeholder="Filled from About/About.xml <description>"
              class="path-input w-full"
            ></textarea>
            <div class="mt-2">
              <label
                for="description-language"
                class="text-xs text-zinc-500 block mb-1.5"
                >Description language code</label
              >
              <select
                id="description-language"
                bind:value={descriptionLanguage}
                class="path-input w-full"
              >
                {#each STEAM_LANGUAGE_OPTIONS as language}
                  <option value={language.code}>
                    {language.label} ({language.code})
                  </option>
                {/each}
              </select>
              <div class="mt-1 text-[11px] text-zinc-500">
                Used by "Update Description Only" in Steamworks SDK mode.
              </div>
            </div>
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
              onclick={oneClickRimWorldUpload}
              disabled={isUploading || isDetectingMod || isPackaging}
              class="btn-primary py-4 text-[15px]"
            >
              {#if isUploading}
                <span class="inline-flex items-center gap-2">
                  <span
                    class="w-3.5 h-3.5 border-2 border-white/30 border-t-white rounded-full animate-spin"
                  ></span>
                  Uploading…
                </span>
              {:else if isPackaging}
                Generating package…
              {:else if isDetectingMod}
                Scanning mod…
              {:else}
                ↑ One-click RimWorld upload / update
              {/if}
            </button>

            <button
              onclick={startUpload}
              disabled={!canUpload}
              class="w-full py-3 font-semibold rounded-2xl bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 disabled:opacity-50 transition active:scale-[0.985]"
            >
              Upload form as-is ({uploadMethod === 'sdk' ? 'SDK' : 'steamcmd'})
            </button>

            <button
              onclick={updateDescriptionOnly}
              disabled={!canUpdateDescription}
              class="w-full py-3 font-semibold rounded-2xl bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 disabled:opacity-50 transition active:scale-[0.985]"
            >
              Update Description Only ({normalizeLanguageInput() || 'default'})
            </button>

            <button
              onclick={updatePreviewOnly}
              disabled={!canUpdatePreview}
              class="w-full py-3 font-semibold rounded-2xl bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 disabled:opacity-50 transition active:scale-[0.985]"
            >
              Update Preview Only
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
              {#if lastSteamStatusCheckedAt}
                <div class="text-[11px] text-zinc-500 mt-1">
                  Last check: {lastSteamStatusCheckedAt}
                  ({lastSteamStatusCheckSource === 'auto'
                    ? 'auto'
                    : 'manual'})
                </div>
              {/if}
            </div>
            <button
              onclick={() => refreshSteamClientStatus('manual')}
              disabled={isCheckingSteamClient}
              class="btn-secondary text-xs py-1.5 px-3"
            >
              {isCheckingSteamClient ? 'Checking...' : 'Refresh'}
            </button>
          </div>

          {#if isCheckingSteamClient}
            <div class="flex items-center gap-2 text-xs text-blue-400 mb-3">
              <span
                class="w-3 h-3 border-2 border-blue-400/30 border-t-blue-400 rounded-full animate-spin"
              ></span>
              Detecting Steam client status...
            </div>
          {/if}

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
