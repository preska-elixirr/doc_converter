import { invoke, isTauri } from '@tauri-apps/api/core';

export type InputKind =
  | 'pdf' | 'docx' | 'odt' | 'pptx' | 'xlsx' | 'md' | 'html' | 'txt'
  | 'png' | 'jpg' | 'bmp' | 'webp' | 'tiff' | 'age' | 'other';
export type OutputFormat = 'pdf' | 'pdfa1b' | 'pdfa2b' | 'pdfa3b' | 'pdfa4' | 'pdfa4f' | 'pdfua1' | 'docx' | 'txt' | 'html' | 'md';
export type ImageFormat = 'png' | 'jpg' | 'webp' | 'pdf';
export type Mode = 'convert' | 'images' | 'clean' | 'encrypt' | 'decrypt' | 'license';

export type Availability = {
  format: OutputFormat;
  available: boolean;
  engine: string;
  reason: string | null;
};

export type Asset = {
  id: string;
  name: string;
  bytes: number;
  kind: InputKind;
  label: string;
  outputs: Availability[];
  pdf: { pages: number; encrypted: boolean } | null;
  image: { kind: InputKind; width: number; height: number } | null;
};

export type Orientation = 'keep' | 'portrait' | 'landscape';
export type Margins = 'narrow' | 'normal' | 'wide';
export type Spacing = 'compact' | 'comfortable' | 'spacious';
export type Layout = {
  orientation: Orientation;
  margins: Margins;
  spacing: Spacing;
  page_breaks: number[];
};

export type OutlineEntry = { index: number; kind: string; text: string };
export type Status = 'queued' | 'working' | 'done' | 'failed' | 'cancelled';
export type ItemReport = { index: number; status: Status; detail: string; output: string | null; validation: ValidationReport | null };
export type ValidationReport = { profile: string; passed: boolean; human_review_required: boolean; issues: string[] };
export type AttachmentSelection = { id: string; name: string; relationship: 'Source' | 'Data' | 'Supplement' | 'Alternative' | 'Unspecified'; description: string };
export type EngineStatus = {
  validator: boolean;
  ready: boolean;
  office: { path: string; version: string; markdown: boolean } | null;
};

export type BatchRequest = {
  mode: Exclude<Mode, 'license'>;
  items: { id: string; format?: string; page_breaks?: number[] }[];
  merge: boolean;
  merge_name: string;
  layout: Layout;
  password: string;
  protect: boolean;
  encryption: 'pdf' | 'file';
  image: { max_edge: number; quality: number };
  attachments: Omit<AttachmentSelection, 'name'>[];
};
export type BatchOutcome = { result: 'saved' | 'cancelled' | 'nothing'; reports: ItemReport[] };
export type AssetOutputs = { id: string; outputs: Availability[] };
export type PdfSecurityReport = {
  javascript: boolean | null;
  embedded_files: boolean | null;
  internet_links: boolean | null;
  encryption: { algorithm: 'none' | 'rc4' | 'aes128' | 'aes256' | 'mixed' | 'unknown'; bits: number | null; revision: number | null };
  status: 'complete' | 'locked' | 'incomplete' | 'unreadable' | 'too_large';
  fingerprint: string | null;
};

export const desktop = isTauri();

export const api = {
  validatePdf: (id: string, format: OutputFormat) => invoke<ValidationReport>('validate_pdf', { id, format }),
  inspectPdf: (id: string) => invoke<PdfSecurityReport>('inspect_pdf', { id }),
  openInspectedPdf: (id: string, fingerprint: string) => invoke<ArrayBuffer>('open_inspected_pdf', { id, fingerprint }),
  pickFiles: () => invoke<Asset[]>('pick_files'),
  addPaths: (paths: string[]) => invoke<Asset[]>('add_paths', { paths }),
  engineStatus: () => invoke<EngineStatus>('engine_status'),
  runBatch: (request: BatchRequest) => invoke<BatchOutcome>('run_batch', { request }),
  cancel: () => invoke<void>('cancel_job'),
  outline: (id: string) => invoke<OutlineEntry[]>('outline', { id }),
  preview: (id: string, format: string, layout: Layout) => invoke<ArrayBuffer>('preview', { id, format, layout }),
  refreshOutputs: (ids: string[]) => invoke<AssetOutputs[]>('refresh_outputs', { ids }),
};

export function formatBytes(bytes: number): string {
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
