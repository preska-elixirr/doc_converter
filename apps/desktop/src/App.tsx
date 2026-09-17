import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import {
  api, desktop, formatBytes,
  type Asset, type BatchRequest, type EngineStatus, type ImageFormat, type ItemReport, type KeyPair,
  type Layout, type Margins, type Mode, type Orientation, type OutlineEntry, type OutputFormat,
  type Spacing, type Status,
} from './api';
import { PdfPreview, type PreviewLabels } from './Preview';
import { PdfStandards, ValidationDetails, STANDARD_FORMATS, standardPdf, attachmentFormat, formatLabel } from './PdfStandards';
import type { AttachmentSelection, ValidationReport } from './api';
import { detectLanguage, LANGUAGES, translate, translateDetail, translatePlural, type Key, type Language } from './i18n';

type RowState = 'ready' | Status;
type Row = Asset & {
  selected: boolean;
  target: OutputFormat;
  imageTarget: ImageFormat;
  state: RowState;
  detail: string;
  output?: string;
  validation?: ValidationReport | null;
};
type Protection = 'pdf' | 'file' | 'key';
type ResultBar = { title: string; detail: string; done: number; total: number; finished: boolean };
type T = (key: Key, vars?: Record<string, string | number>) => string;
type TN = (key: Key, count: number, vars?: Record<string, string | number>) => string;

const FORMATS: OutputFormat[] = ['pdf', ...STANDARD_FORMATS, 'docx', 'txt', 'html', 'md'];
const IMAGE_FORMATS: ImageFormat[] = ['png', 'jpg', 'webp', 'pdf'];
const MODES: [Mode, string, Key][] = [
  ['convert', '↔', 'tab.convert'],
  ['images', '▧', 'tab.images'],
  ['clean', '✧', 'tab.clean'],
  ['encrypt', '◇', 'tab.encrypt'],
  ['decrypt', '↳', 'tab.decrypt'],
  ['license', '◉', 'tab.license'],
];
const SCALES = [0.9, 1, 1.1, 1.25, 1.5];
const LANGUAGE_PREF = 'doc-converter-language';
const SCALE_PREF = 'doc-converter-scale';

function readPref(key: string): string | null {
  try { return window.localStorage.getItem(key); } catch { return null; }
}
function writePref(key: string, value: string) {
  try { window.localStorage.setItem(key, value); } catch { /* preferences are optional */ }
}
function initialLanguage(): Language {
  const saved = readPref(LANGUAGE_PREF);
  return LANGUAGES.some((l) => l.code === saved) ? (saved as Language) : detectLanguage();
}
function initialScale(): number {
  const saved = Number(readPref(SCALE_PREF));
  return SCALES.includes(saved) ? saved : 1;
}

function defaultTarget(asset: Asset): OutputFormat {
  const available = asset.outputs.filter((o) => o.available);
  return (available.find((o) => o.format === 'pdf') ?? available[0])?.format ?? 'pdf';
}

function toRow(asset: Asset, imageTarget: ImageFormat): Row {
  return { ...asset, selected: true, target: defaultTarget(asset), imageTarget, state: 'ready', detail: '' };
}

/** Why a row cannot take part in the current mode, as a label key, or null when it can. */
function blocked(row: Row, mode: Mode, protection: Protection): Key | null {
  switch (mode) {
    case 'clean':
      if (row.kind === 'pdf') return row.pdf?.encrypted ? 'reason.clean_locked' : null;
      return row.kind === 'docx' || row.image ? null : 'reason.unsupported';
    case 'convert':
      return row.outputs.length ? null : 'reason.unsupported';
    case 'images':
      return row.image ? null : 'reason.unsupported';
    case 'encrypt':
      if (protection !== 'pdf') return null;
      if (row.kind !== 'pdf') return 'reason.pdf_required';
      return row.pdf?.encrypted ? 'reason.already_protected' : null;
    case 'decrypt':
      if (row.kind === 'age') return null;
      if (row.kind === 'pdf') return row.pdf?.encrypted ? null : 'reason.no_password';
      return 'reason.unsupported';
    default:
      return 'reason.unsupported';
  }
}

function statusText(row: Row, reason: Key | null, t: T, language: Language): { text: string; className: string } {
  if (reason) return { text: t(reason), className: 'status blocked' };
  const detail = translateDetail(language, row.detail);
  switch (row.state) {
    case 'queued': return { text: t('status.queued'), className: 'status' };
    case 'working': return { text: row.detail ? detail : t('status.working'), className: 'status working' };
    case 'done': return { text: row.detail ? detail : t('status.saved'), className: 'status' };
    case 'failed': return { text: row.detail || t('status.failed'), className: 'status failed' };
    case 'cancelled': return { text: t('status.cancelled'), className: 'status blocked' };
    default: return { text: t('status.ready'), className: 'status' };
  }
}

function ShieldIcon() {
  return (
    <svg className="shield" viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" focusable="false">
      <path d="M12 2.5 4.5 5.4v5.4c0 5 3.2 9.6 7.5 10.7 4.3-1.1 7.5-5.7 7.5-10.7V5.4L12 2.5z" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" />
      <path d="m8.8 12 2.2 2.2 4.4-4.6" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function App() {
  const [language, setLanguage] = useState<Language>(initialLanguage);
  const [scale, setScale] = useState<number>(initialScale);
  const [prefsOpen, setPrefsOpen] = useState(false);
  const [mode, setMode] = useState<Mode>('convert');
  const [rows, setRows] = useState<Row[]>([]);
  const [protection, setProtection] = useState<Protection>('pdf');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [show, setShow] = useState(false);
  const [recipients, setRecipients] = useState('');
  const [identity, setIdentity] = useState('');
  const [keyPair, setKeyPair] = useState<KeyPair | null>(null);
  const [creatingKey, setCreatingKey] = useState(false);
  const [copied, setCopied] = useState(false);
  const [batchFormat, setBatchFormat] = useState<OutputFormat>('pdf');
  const [attachments, setAttachments] = useState<AttachmentSelection[]>([]);
  const [protect, setProtect] = useState(false);
  const [watermarkEnabled, setWatermarkEnabled] = useState(false);
  const [watermarkText, setWatermarkText] = useState('Confidential');
  const [merge, setMerge] = useState(false);
  const [mergeName, setMergeName] = useState('Combined documents.pdf');
  const [orientation, setOrientation] = useState<Orientation>('keep');
  const [margins, setMargins] = useState<Margins>('normal');
  const [spacing, setSpacing] = useState<Spacing>('comfortable');
  const [breaks, setBreaks] = useState<Record<string, number[]>>({});
  const [imageFormat, setImageFormat] = useState<ImageFormat>('png');
  const [imageQuality, setImageQuality] = useState(85);
  const [imageSize, setImageSize] = useState(0);
  const [previewId, setPreviewId] = useState('');
  const [outline, setOutline] = useState<OutlineEntry[]>([]);
  const [previewData, setPreviewData] = useState<ArrayBuffer | null>(null);
  const [previewState, setPreviewState] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [previewError, setPreviewError] = useState('');
  const [pageCount, setPageCount] = useState(0);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<ResultBar | null>(null);
  const [error, setError] = useState('');
  const [engine, setEngine] = useState<EngineStatus | null>(null);
  const [dragging, setDragging] = useState(false);
  const batchIds = useRef<string[]>([]);
  const prefsRef = useRef<HTMLDivElement>(null);
  const rowsRef = useRef<Row[]>([]);
  rowsRef.current = rows;

  const t = useCallback<T>((key, vars) => translate(language, key, vars), [language]);
  const tn = useCallback<TN>((key, count, vars) => translatePlural(language, key, count, vars), [language]);
  const previewLabels = useMemo<PreviewLabels>(() => ({
    empty: t('preview.empty'),
    error: t('preview.error'),
    page: (number) => t('preview.page', { number }),
    more: (count) => t('preview.more', { count }),
  }), [t]);

  useEffect(() => {
    document.documentElement.lang = language;
    writePref(LANGUAGE_PREF, language);
  }, [language]);

  useEffect(() => {
    writePref(SCALE_PREF, String(scale));
    const fallback = () => document.documentElement.style.setProperty('zoom', String(scale));
    if (desktop) getCurrentWebview().setZoom(scale).catch(fallback);
    else fallback();
  }, [scale]);

  useEffect(() => {
    if (!prefsOpen) return;
    const onPointer = (event: MouseEvent) => {
      if (!prefsRef.current?.contains(event.target as Node)) setPrefsOpen(false);
    };
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') setPrefsOpen(false); };
    document.addEventListener('mousedown', onPointer);
    document.addEventListener('keydown', onKey);
    return () => { document.removeEventListener('mousedown', onPointer); document.removeEventListener('keydown', onKey); };
  }, [prefsOpen]);

  const merging = mode === 'convert' && merge;
  const outputFor = useCallback(
    (row: Row): string => (merging ? 'pdf' : mode === 'clean' ? (row.image ? 'png' : row.kind) : mode === 'images' ? row.imageTarget : row.target),
    [merging, mode],
  );
  const selected = useMemo(
    () => rows.filter((r) => r.selected && !blocked(r, mode, protection)),
    [rows, mode, protection],
  );
  const hasArchival = mode === 'convert' && selected.some((r) => standardPdf(r.target));
  const attachmentConflict = mode === 'convert' && ((attachments.length > 0 && (merging || selected.some((r) => !attachmentFormat(r.target)))) || (attachments.length === 0 && selected.some((r) => r.target === 'pdfa4f')));
  const archivalConflict = hasArchival && (protect || merging);
  const watermarkConflict = mode === 'convert' && watermarkEnabled && (
    !watermarkText.trim() || [...watermarkText].length > 80 || /[\u0000-\u001f\u007f-\u009f\u2028\u2029]/.test(watermarkText) ||
    hasArchival || (!merging && selected.some((r) => r.target !== 'pdf')));
  const skipped = rows.filter((r) => r.selected && blocked(r, mode, protection)).length;
  const keyMode = mode === 'encrypt' && protection === 'key';
  const needsPassword =
    (mode === 'encrypt' && !keyMode) || mode === 'decrypt' ||
    (mode === 'convert' && protect && (merging || selected.some((r) => r.target === 'pdf')));
  const passwordValid = !needsPassword || (mode === 'decrypt' ? password.length > 0 || identity.trim().length > 0 : [...password].length >= 12 && password === confirm);
  // The backend parses every line; the page only insists on one non-comment line.
  const recipientsValid = !keyMode || recipients.split(/\r?\n/).some((line) => line.trim() && !line.trim().startsWith('#'));
  const targetsValid = mode !== 'convert' || merging ||
    selected.every((r) => r.outputs.some((o) => o.format === r.target && o.available));
  const valid = desktop && selected.length > 0 && passwordValid && recipientsValid && targetsValid && !archivalConflict && !watermarkConflict && !attachmentConflict && (!hasArchival || !!engine?.validator) && (!merging || mergeName.trim().length > 0);
  const layoutVisible = mode === 'convert' && (merging || selected.some((r) => r.target === 'pdf' || standardPdf(r.target) || r.target === 'docx'));
  const previewCandidates = useMemo(
    () => selected.filter((r) => merging || r.target === 'pdf' || standardPdf(r.target) || r.target === 'docx'),
    [selected, merging],
  );
  const layoutFor = useCallback(
    (id: string): Layout => ({ orientation, margins, spacing, page_breaks: breaks[id] ?? [] }),
    [orientation, margins, spacing, breaks],
  );

  const refreshEngine = useCallback(() => {
    if (!desktop) return;
    api.engineStatus().then((status) => {
      setEngine(status);
      const ids = rowsRef.current.map((r) => r.id);
      if (!status.ready || !ids.length) return undefined;
      // Files added before detection finished carry stale convert options.
      return api.refreshOutputs(ids).then((updates) => setRows((current) => current.map((r) => {
        const update = updates.find((u) => u.id === r.id);
        if (!update) return r;
        const next = { ...r, outputs: update.outputs };
        const reachable = next.outputs.some((o) => o.format === next.target && o.available);
        return reachable ? next : { ...next, target: defaultTarget(next) };
      })));
    }).catch(() => undefined);
  }, []);

  const addAssets = useCallback((assets: Asset[]) => {
    if (!assets.length) return;
    setRows((current) => [...current, ...assets.map((a) => toRow(a, imageFormat))]);
    setResult(null);
  }, [imageFormat]);

  useEffect(() => {
    if (!desktop) return;
    refreshEngine();
    const stops: Promise<() => void>[] = [
      listen('engines-ready', refreshEngine),
      listen<ItemReport>('batch-progress', (event) => {
        const report = event.payload;
        const id = batchIds.current[report.index];
        if (!id) return;
        setRows((current) => current.map((r) => (r.id === id
          ? { ...r, state: report.status, detail: report.detail, output: report.output ?? undefined, validation: report.validation }
          : r)));
        setResult((current) => (current ? { ...current, title: '', detail: `${report.index + 1}|${report.detail}` } : current));
      }),
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === 'drop') {
          setDragging(false);
          api.addPaths(event.payload.paths).then(addAssets).catch((e) => setError(String(e)));
        } else if (event.payload.type === 'leave') {
          setDragging(false);
        } else {
          setDragging(true);
        }
      }),
    ];
    return () => {
      stops.forEach((p) => p.then((stop) => stop()).catch(() => undefined));
    };
  }, [refreshEngine, addAssets]);

  useEffect(() => {
    if (!previewCandidates.some((r) => r.id === previewId)) {
      setPreviewId(previewCandidates[0]?.id ?? '');
    }
  }, [previewCandidates, previewId]);

  useEffect(() => {
    if (!desktop || !previewId || !layoutVisible) { setOutline([]); return; }
    let live = true;
    api.outline(previewId).then((entries) => { if (live) setOutline(entries); }).catch(() => { if (live) setOutline([]); });
    return () => { live = false; };
  }, [previewId, layoutVisible]);

  const breaksKey = (breaks[previewId] ?? []).join(',');
  const previewFormat = merging ? 'pdf' : (rows.find((r) => r.id === previewId)?.target ?? 'pdf');
  useEffect(() => {
    if (!desktop || !previewId || !layoutVisible || busy) return;
    let live = true;
    setPreviewState('loading');
    const timer = window.setTimeout(() => {
      api.preview(previewId, previewFormat, layoutFor(previewId), watermarkEnabled ? watermarkText : null)
        .then((buffer) => { if (live) { setPreviewData(buffer); setPreviewState('ready'); setPreviewError(''); } })
        .catch((e) => { if (live) { setPreviewData(null); setPreviewState('error'); setPreviewError(String(e)); } });
    }, 700);
    return () => { live = false; window.clearTimeout(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previewId, previewFormat, orientation, margins, spacing, breaksKey, layoutVisible, busy, watermarkEnabled, watermarkText]);

  function switchMode(next: Mode) {
    if (busy) return;
    setMode(next);
    setPassword(''); setConfirm(''); setIdentity(''); setShow(false); setCopied(false);
    setResult(null); setError('');
    setRows((current) => current.map((r) => ({ ...r, state: 'ready', detail: '', output: undefined })));
  }

  async function add() {
    try { addAssets(await api.pickFiles()); } catch (e) { setError(String(e)); }
  }

  function update(id: string, patch: Partial<Row>) {
    setRows((current) => current.map((r) => (r.id === id ? { ...r, ...patch, validation: undefined } : r)));
    setResult(null);
  }

  function toggleBreak(id: string, index: number) {
    setBreaks((current) => {
      const list = current[id] ?? [];
      const next = list.includes(index) ? list.filter((i) => i !== index) : [...list, index].sort((a, b) => a - b);
      return { ...current, [id]: next };
    });
  }

  function move(id: string, delta: number) {
    setRows((current) => {
      const order = current.filter((r) => previewCandidates.some((c) => c.id === r.id));
      const index = order.findIndex((r) => r.id === id);
      const other = order[index + delta];
      if (!other) return current;
      const a = current.findIndex((r) => r.id === id);
      const b = current.findIndex((r) => r.id === other.id);
      const next = [...current];
      [next[a], next[b]] = [next[b], next[a]];
      return next;
    });
  }

  async function createKeyPair() {
    setCreatingKey(true); setError(''); setCopied(false);
    try {
      const pair = await api.createKeyPair();
      if (pair) setKeyPair(pair);
    } catch (e) {
      setError(String(e));
    } finally {
      setCreatingKey(false);
    }
  }

  async function copyPublicKey() {
    if (!keyPair) return;
    try { await navigator.clipboard.writeText(keyPair.public_key); setCopied(true); } catch { setCopied(false); }
  }

  function addOwnKey() {
    if (!keyPair) return;
    const key = keyPair.public_key;
    setRecipients((current) => {
      if (current.split(/\r?\n/).some((line) => line.trim() === key)) return current;
      const kept = current.replace(/\s+$/, '');
      return kept ? `${kept}\n${key}` : key;
    });
    setResult(null);
  }

  async function run() {
    if (!valid || mode === 'license') return;
    const batch = selected;
    const ids = batch.map((r) => r.id);
    batchIds.current = ids;
    setBusy(true); setError('');
    setRows((current) => current.map((r) => (ids.includes(r.id) ? { ...r, state: 'queued', detail: 'Queued', output: undefined, validation: undefined } : r)));
    setResult({ title: 'choose', detail: '', done: 0, total: ids.length, finished: false });
    const request: BatchRequest = {
      mode,
      items: batch.map((r) => ({ id: r.id, format: outputFor(r), page_breaks: breaks[r.id] ?? [] })),
      merge: merging,
      merge_name: mergeName,
      layout: { orientation, margins, spacing, page_breaks: [] },
      password,
      protect: mode === 'convert' && protect,
      watermark: mode === 'convert' && watermarkEnabled ? watermarkText : null,
      encryption: protection,
      recipients: keyMode ? recipients : '',
      identity: mode === 'decrypt' ? identity : '',
      image: { max_edge: imageSize, quality: imageQuality },
      attachments: mode === 'convert' ? attachments.map(({ id, relationship, description }) => ({ id, relationship, description })) : [],
    };
    try {
      const outcome = await api.runBatch(request);
      if (outcome.result === 'cancelled') {
        setRows((current) => current.map((r) => (ids.includes(r.id) ? { ...r, state: 'ready', detail: '' } : r)));
        setResult({ title: 'save_cancelled', detail: '', done: 0, total: ids.length, finished: true });
      } else {
        const done = outcome.reports.filter((r) => r.status === 'done').length;
        const failed = outcome.reports.filter((r) => r.status === 'failed').length;
        const cancelled = outcome.reports.filter((r) => r.status === 'cancelled').length;
        const first = outcome.reports.find((r) => r.output)?.output ?? '';
        setResult({
          title: failed && !done ? 'nothing' : cancelled ? 'stopped' : 'done',
          detail: `${merging ? 'merge' : done}|${failed}|${cancelled}|${first}`,
          done: outcome.reports.length, total: ids.length, finished: true,
        });
      }
    } catch (e) {
      setRows((current) => current.map((r) => (ids.includes(r.id) ? { ...r, state: 'ready', detail: '' } : r)));
      setResult({ title: 'could_not_start', detail: String(e), done: 0, total: ids.length, finished: true });
    } finally {
      setBusy(false); setPassword(''); setConfirm(''); setIdentity('');
    }
  }

  /** Result bar texts are stored as codes so a language change re-renders them. */
  function resultText(bar: ResultBar): { title: string; detail: string } {
    switch (bar.title) {
      case 'choose': return { title: t('result.choose'), detail: t('result.untouched') };
      case '': {
        const [index, detail] = bar.detail.split('|');
        return { title: t('result.processing'), detail: t('result.progress', { index, total: bar.total, detail: translateDetail(language, detail ?? '') }) };
      }
      case 'save_cancelled': return { title: t('result.save_cancelled'), detail: t('result.save_cancelled_detail') };
      case 'stopping': return { title: t('result.stopping'), detail: '' };
      case 'could_not_start': return { title: t('result.could_not_start'), detail: bar.detail };
      default: {
        const [saved, failed, cancelled, first] = bar.detail.split('|');
        const parts = [
          saved === 'merge' ? t('result.combined_saved') : tn('result.saved', Number(saved)),
          Number(failed) ? t('result.failed', { count: failed }) : '',
          Number(cancelled) ? t('result.cancelled', { count: cancelled }) : '',
        ].filter(Boolean);
        const title = bar.title === 'nothing' ? t('result.nothing') : bar.title === 'stopped' ? t('result.stopped') : t('result.done');
        return { title, detail: parts.join(' · ') + (first ? ` · ${first}` : '') };
      }
    }
  }

  const doneCount = rows.filter((r) => batchIds.current.includes(r.id) && (r.state === 'done' || r.state === 'failed' || r.state === 'cancelled')).length;
  const outputs = new Set(selected.map(outputFor));
  const summaryType = merging ? t('summary.combined')
    : mode === 'clean' ? t('clean.copies')
    : mode === 'convert' || mode === 'images' ? (outputs.size === 1 ? formatLabel([...outputs][0]) : outputs.size ? t('summary.mixed') : '—')
    : mode === 'encrypt' ? (protection === 'pdf' ? t('summary.protected') : t('summary.encrypted')) : mode === 'decrypt' ? t('summary.restored') : '';
  const actionLabel = busy ? t('action.working')
    : merging ? t('action.combine')
    : mode === 'convert' ? (selected.length ? tn('action.convert', selected.length) : t('action.convert_none'))
    : mode === 'images' ? t('action.images')
    : mode === 'clean' ? t('action.clean')
    : mode === 'encrypt' ? (protection === 'pdf' ? t('action.protect') : t('action.encrypt')) : t('action.decrypt');
  const acceptedTypes = mode === 'images' ? t('drop.types.images')
    : mode === 'clean' ? t('drop.types.clean')
    : mode === 'decrypt' ? t('drop.types.decrypt')
    : mode === 'encrypt' && protection === 'pdf' ? t('drop.types.pdf') : t('drop.types.documents');
  const lossy = selected.some((r) => ['jpg', 'webp', 'pdf'].includes(outputFor(r)));
  const officeMissing = engine?.ready && !engine.office;
  const orientationWord = t(orientation === 'landscape' ? 'layout.o.landscape' : orientation === 'portrait' ? 'layout.o.portrait' : 'layout.o.keep');
  const resultTexts = result ? resultText(result) : null;
  const keyPairPanel = (
    <div className="key-pair">
      <div className="separator"></div>
      <p className="field-label">{t('keys.title')}</p>
      <p className="hint">{t('keys.hint')}</p>
      <button className="secondary" disabled={busy || creatingKey || !desktop} onClick={createKeyPair}>{creatingKey ? t('keys.creating') : t('keys.create')}</button>
      {keyPair && (<>
        <label className="field-label" htmlFor="public-key">{t('keys.public')}</label>
        <div className="password-field"><input id="public-key" className="key" type="text" readOnly value={keyPair.public_key} onFocus={(e) => e.currentTarget.select()} /><button className="text-button" onClick={copyPublicKey}>{copied ? t('keys.copied') : t('keys.copy')}</button></div>
        {keyMode && <button className="text-button" disabled={busy} onClick={addOwnKey}>{t('recipients.add_own')}</button>}
        <p className="hint">{t('keys.saved', { path: keyPair.path })}</p>
        <p className="password-warning">{t('keys.warning')}</p>
      </>)}
    </div>
  );

  return (
    <div className={`app${dragging ? ' dragging' : ''}`}>
      <header className="topbar">
        <span className="brand"><span className="brandmark" aria-hidden="true">d.</span>Doc Converter<span className="brand-divider"></span><span className="brand-description">{t('header.tagline')}</span></span>
        <div className="topbar-right">
          <span className="local-badge"><ShieldIcon /><span>{t('header.local')}</span></span>
          <div className="prefs" ref={prefsRef}>
            <button className="prefs-button" aria-haspopup="dialog" aria-expanded={prefsOpen} title={t('prefs.open')} onClick={() => setPrefsOpen((o) => !o)}>Aa<span className="sr-only">{t('prefs.open')}</span></button>
            {prefsOpen && (
              <div className="prefs-panel" role="dialog" aria-label={t('prefs.open')}>
                <label>{t('prefs.language')}<select value={language} onChange={(e) => setLanguage(e.target.value as Language)}>{LANGUAGES.map((l) => <option key={l.code} value={l.code}>{l.label}</option>)}</select></label>
                <label>{t('prefs.scale')}<select value={scale} onChange={(e) => setScale(Number(e.target.value))}>{SCALES.map((s) => <option key={s} value={s}>{Math.round(s * 100)}%</option>)}</select></label>
              </div>
            )}
          </div>
        </div>
      </header>
      <main>
        <div className="intro"><div><p className="eyebrow">{t('intro.eyebrow')}</p><h1>{t('intro.title')}</h1><p>{t('intro.subtitle')}</p></div></div>
        {!desktop && <div className="notice">{t('browser.notice')}</div>}
        <nav className="modes" aria-label="Document operation">
          {MODES.map(([id, icon, label]) => (
            <button key={id} className={mode === id ? 'active' : ''} aria-pressed={mode === id} disabled={busy} onClick={() => switchMode(id)}>
              <span aria-hidden="true">{icon}</span> {t(label)}
            </button>
          ))}
        </nav>
        {mode === 'license' ? (
          <section className="placeholder"><p className="eyebrow">{t('license.eyebrow')}</p><h2>{t('license.title')}</h2><p>{t('license.p1')}</p><p>{t('license.p2')}</p></section>
        ) : (
          <div className="workspace">
            <div className="left-column">
              <section className="queue" aria-labelledby="queue-title">
                <div className="section-head"><div><h2 id="queue-title">{t('queue.title')} <span className="count">{rows.length}</span></h2><p>{t(`queue.desc.${mode}` as Key)}</p></div><button className="secondary" disabled={busy || !desktop} onClick={add}><span aria-hidden="true">＋</span> {t('queue.add')}</button></div>
                <div className="queue-tools">
                  <label><input type="checkbox" disabled={busy || !rows.length} checked={rows.length > 0 && rows.every((r) => r.selected)} ref={(el) => { if (el) el.indeterminate = rows.some((r) => r.selected) && !rows.every((r) => r.selected); }} onChange={(e) => { const on = e.target.checked; setRows((c) => c.map((r) => ({ ...r, selected: on }))); setResult(null); }} /> <span>{t('queue.selected', { count: rows.filter((r) => r.selected).length })}</span></label>
                  <button className="text-button" disabled={busy || !rows.length} onClick={() => { setRows([]); setResult(null); }}>{t('queue.clear')}</button>
                </div>
                <div className="table-scroll">
                  <table>
                    <thead><tr><th scope="col"><span className="sr-only">{t('queue.col.selected')}</span></th><th scope="col">{t('queue.col.document')}</th><th scope="col">{mode === 'decrypt' ? t('queue.col.restore') : t('queue.col.output')}</th><th scope="col">{t('queue.col.status')}</th><th scope="col"><span className="sr-only">{t('queue.col.remove')}</span></th></tr></thead>
                    <tbody>
                      {rows.length ? rows.map((row) => {
                        const reason = blocked(row, mode, protection);
                        const status = statusText(row, reason, t, language);
                        const meta = [
                          formatBytes(row.bytes),
                          row.pdf ? tn('meta.pages', row.pdf.pages) + (row.pdf.encrypted ? ` · ${t('meta.locked')}` : '') : '',
                          row.image ? `${row.image.width} × ${row.image.height}` : '',
                        ].filter(Boolean).join(' · ');
                        return (
                          <tr key={row.id}>
                            <td><input type="checkbox" aria-label={t('queue.select_row', { name: row.name })} checked={row.selected} disabled={busy} onChange={(e) => update(row.id, { selected: e.target.checked })} /></td>
                            <td><div className="file"><span className={`file-type ${row.kind}`}>{row.label}</span><div style={{ minWidth: 0 }}><span className="filename" title={row.output ?? row.name}>{row.name}</span><span className="file-meta">{meta}</span></div></div></td>
                            <td>
                              {merging ? <span className="output-label">{t('queue.merged')}</span>
                                : mode === 'clean' ? <span className="output-label">{outputFor(row).toUpperCase()}</span>
                                : mode === 'convert' ? (
                                  <select className="row-format" aria-label={t('queue.format_row', { name: row.name })} disabled={busy || !!reason} value={row.target} onChange={(e) => update(row.id, { target: e.target.value as OutputFormat })}>
                                    {FORMATS.map((f) => { const a = row.outputs.find((o) => o.format === f); return <option key={f} value={f} disabled={!a?.available} title={a?.reason ?? ''}>{formatLabel(f)}{a && !a.available ? ' ✕' : ''}</option>; })}
                                  </select>
                                ) : mode === 'images' ? (
                                  <select className="row-format" aria-label={t('queue.format_row', { name: row.name })} disabled={busy || !!reason} value={row.imageTarget} onChange={(e) => update(row.id, { imageTarget: e.target.value as ImageFormat })}>
                                    {IMAGE_FORMATS.map((f) => <option key={f} value={f}>{formatLabel(f)}</option>)}
                                  </select>
                                ) : <span className="output-label">{mode === 'encrypt' ? (protection === 'pdf' ? t('queue.pdf_key') : t('queue.age')) : t('queue.original')}</span>}
                            </td>
                            <td><span className={status.className} title={row.output ?? row.detail}>{status.text}</span>{row.validation && <ValidationDetails report={row.validation} language={language} />}</td>
                            <td><button className="remove" aria-label={t('queue.remove_row', { name: row.name })} disabled={busy} onClick={() => { setRows((c) => c.filter((r) => r.id !== row.id)); setResult(null); }}>×</button></td>
                          </tr>
                        );
                      }) : <tr><td colSpan={5} className="empty">{t('queue.empty')}</td></tr>}
                    </tbody>
                  </table>
                </div>
                <button className={`dropzone${dragging ? ' drag' : ''}`} disabled={busy || !desktop} onClick={add}><span className="drop-symbol" aria-hidden="true">＋</span><strong>{mode === 'images' ? t('drop.images') : mode === 'decrypt' ? t('drop.protected') : t('drop.documents')}</strong><span>{t('drop.browse')}</span><small>{acceptedTypes}</small></button>
                <div className="queue-bottom"><span>{t('queue.kept')}</span><span>{officeMissing ? t('engine.missing') : engine?.office ? t('engine.ready', { version: engine.office.version }) : desktop ? t('engine.checking') : ''}</span></div>
              </section>

              {layoutVisible && (
                <section className="layout-preview" aria-labelledby="preview-title">
                  <div className="section-head"><div><p className="eyebrow">{t('layout.eyebrow')}</p><h2 id="preview-title">{t('layout.title')}</h2><p>{t('layout.desc')}</p></div><span className="count">{previewState === 'loading' ? t('layout.rendering') : pageCount ? tn('layout.pages', pageCount, { orientation: orientationWord }) : t('layout.real')}</span></div>
                  {merging ? (
                    <div className="merge-order"><p className="field-label">{t('order.title')}</p>
                      {previewCandidates.map((row, i) => (
                        <div className="order-row" key={row.id}><span className="order-number">{String(i + 1).padStart(2, '0')}</span><button className={`order-name${previewId === row.id ? ' active' : ''}`} onClick={() => setPreviewId(row.id)} title={t('order.preview')}>{row.name}</button><button aria-label={t('order.up', { name: row.name })} disabled={i === 0 || busy} onClick={() => move(row.id, -1)}>↑</button><button aria-label={t('order.down', { name: row.name })} disabled={i === previewCandidates.length - 1 || busy} onClick={() => move(row.id, 1)}>↓</button></div>
                      ))}
                    </div>
                  ) : (
                    <div className="preview-toolbar"><label>{t('preview.document')}<select aria-label={t('preview.document')} disabled={busy} value={previewId} onChange={(e) => setPreviewId(e.target.value)}>{previewCandidates.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}</select></label></div>
                  )}
                  <div className="page-controls">
                    <label>{t('margins.label')}<select disabled={busy} value={margins} onChange={(e) => setMargins(e.target.value as Margins)}><option value="normal">{t('margins.normal')}</option><option value="narrow">{t('margins.narrow')}</option><option value="wide">{t('margins.wide')}</option></select></label>
                    <label>{t('spacing.label')}<select disabled={busy} value={spacing} onChange={(e) => setSpacing(e.target.value as Spacing)}><option value="compact">{t('spacing.compact')}</option><option value="comfortable">{t('spacing.comfortable')}</option><option value="spacious">{t('spacing.spacious')}</option></select></label>
                  </div>
                  <p className="hint">{t('layout.hint')}</p>
                  <div className="break-toolbar"><p>{outline.length ? t('breaks.tick') : t('breaks.none')}</p><button className="text-button" disabled={busy || !(breaks[previewId]?.length)} onClick={() => setBreaks((c) => ({ ...c, [previewId]: [] }))}>{t('breaks.reset')}</button></div>
                  {outline.length > 0 && (
                    <div className="outline">
                      {outline.map((entry) => (
                        <label key={entry.index} className={`outline-row ${entry.kind}`}>
                          <input type="checkbox" disabled={busy || entry.index === 0} checked={(breaks[previewId] ?? []).includes(entry.index)} onChange={() => toggleBreak(previewId, entry.index)} />
                          <span className="outline-kind">{entry.kind.startsWith('heading') ? `H${entry.kind.replace('heading', '') || '1'}` : t(`outline.${entry.kind}` as Key)}</span>
                          <span className="outline-text">{entry.text}</span>
                        </label>
                      ))}
                    </div>
                  )}
                  {previewState === 'error' && <p className="error">{previewError}</p>}
                  <PdfPreview data={previewData} labels={previewLabels} onPages={setPageCount} />
                  <p className="hint">{t('preview.hint')}</p>
                </section>
              )}
            </div>

            <aside className="settings" aria-labelledby="settings-title">
              <div className="settings-heading"><span className="step">{t('settings.step')}</span><div><h2 id="settings-title">{t(`settings.title.${mode}` as Key)}</h2><p>{t(`settings.sub.${mode}` as Key)}</p></div></div>
              <div className="settings-body">
                {mode === 'clean' && (
                  <div className="clean-settings">
                    <div className="info"><strong>{t('clean.what')}</strong><p>{t('clean.definition')}</p></div>
                    <p><strong>DOCX</strong></p><p className="hint">{t('clean.docx')}</p>
                    <p><strong>{t('clean.photos_title')}</strong></p><p className="hint">{t('clean.photos')}</p>
                    <p><strong>PDF</strong></p><p className="hint">{t('clean.pdf')}</p>
                    <div className="separator"></div>
                    <p className="hint">{t('clean.limits')}</p>
                  </div>
                )}
                {mode === 'convert' && (
                  <div>
                    <label className="field-label" htmlFor="batch-format">{t('convert.batch')}</label>
                    <select id="batch-format" disabled={busy || merging} value={batchFormat} onChange={(e) => { const f = e.target.value as OutputFormat; setBatchFormat(f); setRows((c) => c.map((r) => (r.selected && r.outputs.some((o) => o.format === f && o.available) ? { ...r, target: f } : r))); setResult(null); }}>{FORMATS.map((f) => <option key={f} value={f}>{formatLabel(f)}</option>)}</select>
                    <p className="hint">{t('convert.batch_hint')}</p>
                    <p className="hint">{t('convert.archival_hint')}</p>
                    {archivalConflict && <p className="error" role="alert">{t('convert.archival_conflict')}</p>}
                    {attachmentConflict && <p className="error" role="alert">{t('standards.attachment_conflict')}</p>}
                    <PdfStandards language={language} busy={busy} setBusy={setBusy} validator={!!engine?.validator} attachments={attachments} onAttachments={setAttachments} pdfId={selected.length === 1 && selected[0].kind === 'pdf' ? selected[0].id : undefined} pdfName={selected.length === 1 && selected[0].kind === 'pdf' ? selected[0].name : undefined} />
                    <div className="separator"></div>
                    <label className="switch-row"><span><strong>{t('convert.protect')}</strong><small>{t('convert.protect_small')}</small></span><input type="checkbox" role="switch" disabled={busy || (hasArchival && !protect)} checked={protect} onChange={(e) => setProtect(e.target.checked)} /></label>
                    <p className="hint">{t('convert.protect_hint')}</p>
                    <label className="switch-row"><span><strong>{t('watermark.title')}</strong><small>{t('watermark.subtitle')}</small></span><input type="checkbox" role="switch" disabled={busy} checked={watermarkEnabled} onChange={(e) => { setWatermarkEnabled(e.target.checked); setResult(null); }} /></label>
                    {watermarkEnabled && <>
                      <label className="field-label" htmlFor="watermark-text">{t('watermark.label')}</label>
                      <input id="watermark-text" type="text" disabled={busy} value={watermarkText} placeholder="Confidential" onChange={(e) => { setWatermarkText(e.target.value); setResult(null); }} aria-describedby="watermark-hint" />
                      <p id="watermark-hint" className="hint">{t('watermark.hint')}</p>
                      {watermarkConflict && <p className="error" role="alert">{t('watermark.error')}</p>}
                    </>}
                    <div className="separator"></div>
                    <label className="switch-row"><span><strong>{t('convert.merge')}</strong><small>{t('convert.merge_small')}</small></span><input type="checkbox" role="switch" disabled={busy || (hasArchival && !merge)} checked={merge} onChange={(e) => { setMerge(e.target.checked); setResult(null); }} /></label>
                    {merging && (<div className="merge-name-field"><label className="field-label" htmlFor="merge-name">{t('convert.merge_name')}</label><input id="merge-name" type="text" disabled={busy} value={mergeName} onChange={(e) => setMergeName(e.target.value)} /><p className="hint">{t('convert.merge_hint')}</p></div>)}
                    <div className="separator"></div>
                    <span className="field-label">{t('convert.orientation')}</span>
                    <div className="orientation" role="group" aria-label={t('convert.orientation')}>
                      {(['keep', 'portrait', 'landscape'] as Orientation[]).map((o) => (
                        <button key={o} className={orientation === o ? 'active' : ''} aria-pressed={orientation === o} disabled={busy} onClick={() => setOrientation(o)}>{o !== 'keep' && <span className={`paper-icon${o === 'landscape' ? ' landscape' : ''}`}></span>}{t(`orientation.${o}` as Key)}</button>
                      ))}
                    </div>
                    <p className="hint">{t('convert.orientation_hint')}</p>
                  </div>
                )}
                {mode === 'images' && (
                  <div>
                    <label className="field-label" htmlFor="image-format">{t('images.format')}</label>
                    <select id="image-format" disabled={busy} value={imageFormat} onChange={(e) => { const f = e.target.value as ImageFormat; setImageFormat(f); setRows((c) => c.map((r) => (r.selected && r.image ? { ...r, imageTarget: f } : r))); setResult(null); }}>{IMAGE_FORMATS.map((f) => <option key={f} value={f}>{formatLabel(f)}</option>)}</select>
                    <p className="hint">{t('images.format_hint')}</p>
                    <div className="separator"></div>
                    <label className="field-label" htmlFor="image-quality">{t('images.quality')} <output>{imageQuality}%</output></label>
                    <input type="range" id="image-quality" min="10" max="100" disabled={busy || !lossy} value={imageQuality} onChange={(e) => setImageQuality(Number(e.target.value))} />
                    <p className="hint">{lossy ? t('images.quality_hint_lossy') : t('images.quality_hint_png')}</p>
                    <label className="field-label image-size-label" htmlFor="image-size">{t('images.resize')}</label>
                    <select id="image-size" disabled={busy} value={imageSize} onChange={(e) => setImageSize(Number(e.target.value))}><option value={0}>{t('images.original')}</option>{[1920, 1280, 640].map((n) => <option key={n} value={n}>{t('images.fit', { n })}</option>)}</select>
                    <p className="hint">{t('images.resize_hint')}</p>
                    <p className="hint">{selected.some((r) => ['jpg', 'pdf'].includes(outputFor(r))) ? t('images.alpha_white') : t('images.alpha_keep')}</p>
                  </div>
                )}
                {mode === 'encrypt' && (
                  <div>
                    <span className="field-label">{t('encrypt.type')}</span>
                    <label className="choice"><input type="radio" name="encryption-type" value="pdf" disabled={busy} checked={protection === 'pdf'} onChange={() => { setProtection('pdf'); setResult(null); }} /><span><strong>{t('encrypt.pdf')}</strong><small>{t('encrypt.pdf_small')}</small></span></label>
                    <label className="choice"><input type="radio" name="encryption-type" value="file" disabled={busy} checked={protection === 'file'} onChange={() => { setProtection('file'); setResult(null); }} /><span><strong>{t('encrypt.file')}</strong><small>{t('encrypt.file_small')}</small></span></label>
                    <label className="choice"><input type="radio" name="encryption-type" value="key" disabled={busy} checked={protection === 'key'} onChange={() => { setProtection('key'); setResult(null); }} /><span><strong>{t('encrypt.key')}</strong><small>{t('encrypt.key_small')}</small></span></label>
                    <p className="hint">{protection === 'pdf' ? t('encrypt.hint_pdf') : protection === 'file' ? t('encrypt.hint_file') : t('encrypt.hint_key')}</p>
                    {keyMode ? (<>
                      <div className="separator"></div>
                      <label className="field-label" htmlFor="recipients">{t('recipients.label')}</label>
                      <textarea id="recipients" rows={3} autoComplete="off" spellCheck={false} disabled={busy} value={recipients} placeholder="age1…" onChange={(e) => { setRecipients(e.target.value); setResult(null); }} aria-describedby="recipients-hint" />
                      <p id="recipients-hint" className="hint">{t('recipients.hint')}</p>
                      {keyPairPanel}
                    </>) : <div className="separator"></div>}
                  </div>
                )}
                {mode === 'decrypt' && (<div><div className="info"><strong>{t('decrypt.title')}</strong><p>{t('decrypt.text')}</p></div><div className="separator"></div></div>)}
                {needsPassword && (
                  <div className="password-settings">
                    <label className="field-label" htmlFor="password">{mode === 'decrypt' ? t('password.document') : t('password.create')}</label>
                    <div className="password-field"><input type={show ? 'text' : 'password'} id="password" autoComplete="off" spellCheck={false} disabled={busy} value={password} onChange={(e) => setPassword(e.target.value)} /><button className="text-button" disabled={busy} onClick={() => setShow(!show)} aria-label={show ? t('password.hide_label') : t('password.show_label')}>{show ? t('password.hide') : t('password.show')}</button></div>
                    <p className="hint">{mode === 'decrypt' ? t('password.hint_existing') : t('password.hint_new')}</p>
                    {mode !== 'decrypt' && (<div className="confirm-field"><label className="field-label" htmlFor="confirm-password">{t('password.confirm')}</label><input type={show ? 'text' : 'password'} id="confirm-password" autoComplete="off" disabled={busy} value={confirm} onChange={(e) => setConfirm(e.target.value)} /><p className="error" aria-live="polite">{confirm && confirm !== password ? t('password.mismatch') : ''}</p><p className="password-warning">{t('password.warning')}</p></div>)}
                  </div>
                )}
                {mode === 'decrypt' && (
                  <div className="password-settings">
                    <label className="field-label" htmlFor="identity">{t('identity.label')}</label>
                    <div className="password-field"><input type={show ? 'text' : 'password'} id="identity" className="key" autoComplete="off" spellCheck={false} disabled={busy} value={identity} placeholder="AGE-SECRET-KEY-1…" onChange={(e) => setIdentity(e.target.value)} aria-describedby="identity-hint" /><button className="text-button" disabled={busy} onClick={() => setShow(!show)} aria-label={show ? t('password.hide_label') : t('password.show_label')}>{show ? t('password.hide') : t('password.show')}</button></div>
                    <p id="identity-hint" className="hint">{t('identity.hint')}</p>
                    {keyPairPanel}
                  </div>
                )}
                <div className="separator"></div>
                <div className="destination"><span className="folder-icon" aria-hidden="true">↳</span><div><strong>{t('destination.title')}</strong><p>{selected.length > 1 && !merging ? t('destination.folder') : t('destination.location')}</p></div></div>
              </div>
              <div className="action-area">
                <div className="summary"><span>{tn(mode === 'images' ? 'summary.images' : 'summary.documents', selected.length)}</span><strong>{summaryType}</strong></div>
                <button className="primary" disabled={!valid || busy || creatingKey} onClick={run}>{actionLabel} <span aria-hidden="true">→</span></button>
                {busy && <button className="secondary cancel" onClick={() => { api.cancel().catch(() => undefined); setResult((c) => (c ? { ...c, title: 'stopping' } : c)); }}>{t('action.cancel')}</button>}
                <p className="action-note">{skipped ? `${tn('note.skipped', skipped)} · ` : ''}{!targetsValid ? `${t('note.targets')} · ` : ''}{t('note.copies')}</p>
              </div>
            </aside>
          </div>
        )}
        {error && <p role="alert" className="error notice">{error}</p>}
        {result && resultTexts && (
          <section className="result" aria-live="polite">
            <div><strong>{resultTexts.title}</strong><p>{resultTexts.detail}</p></div>
            {!result.finished && <progress max={result.total} value={doneCount} aria-label={t('result.progress_label')}></progress>}
            {result.finished ? <button className="secondary" onClick={() => setResult(null)}>{t('result.dismiss')}</button> : <button className="secondary" onClick={() => api.cancel().catch(() => undefined)}>{t('action.cancel')}</button>}
          </section>
        )}
        <footer><span><span className="footer-dot"></span>{t('footer.private')}</span><p>{t('footer.note')}</p><span className="version">{t('footer.version')}</span></footer>
      </main>
    </div>
  );
}
