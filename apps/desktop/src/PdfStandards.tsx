import { useEffect, useState } from 'react';
import { api, desktop, type AttachmentSelection, type OutputFormat, type ValidationReport } from './api';
import { translate, type Language } from './i18n';

export const STANDARD_FORMATS: OutputFormat[] = ['pdfa1b', 'pdfa2b', 'pdfa3b', 'pdfa4', 'pdfa4f', 'pdfua1'];
export const standardPdf = (format: string) => STANDARD_FORMATS.includes(format as OutputFormat);
export const attachmentFormat = (format: string) => format === 'pdfa3b' || format === 'pdfa4f';
export const formatLabel = (format: string) => ({ pdfa1b: 'PDF/A-1b', pdfa2b: 'PDF/A-2b', pdfa3b: 'PDF/A-3b', pdfa4: 'PDF/A-4', pdfa4f: 'PDF/A-4f', pdfua1: 'PDF/UA-1' }[format] ?? format.toUpperCase());

export function ValidationDetails({ report, language }: { report: ValidationReport; language: Language }) {
  const t = (key: Parameters<typeof translate>[1]) => translate(language, key);
  return <details className="validation-details" open={!report.passed}>
    <summary>{report.profile}: {t(report.passed ? report.human_review_required ? 'standards.machine_passed' : 'standards.passed' : 'standards.failed')}</summary>
    {report.human_review_required && <p>{t('standards.human')}</p>}
    {!!report.issues.length && <ul>{report.issues.map((issue, i) => <li key={i}>{issue}</li>)}</ul>}
  </details>;
}

export function PdfStandards({ language, busy, setBusy, validator, attachments, onAttachments, pdfId, pdfName }: {
  language: Language; busy: boolean; setBusy: (value: boolean) => void; validator: boolean;
  attachments: AttachmentSelection[]; onAttachments: (value: AttachmentSelection[]) => void;
  pdfId?: string; pdfName?: string;
}) {
  const t = (key: Parameters<typeof translate>[1]) => translate(language, key);
  const [error, setError] = useState('');
  const [profile, setProfile] = useState<OutputFormat>('pdfa2b');
  const [report, setReport] = useState<ValidationReport | null>(null);
  useEffect(() => { setReport(null); setError(''); }, [pdfId, profile]);
  async function attach() {
    setError('');
    try {
      const files = await api.pickFiles();
      if (attachments.length + files.length > 20) throw new Error(t('standards.attachment_limit'));
      onAttachments([...attachments, ...files.map((f) => ({ id: f.id, name: f.name, relationship: 'Supplement' as const, description: '' }))]);
    } catch (e) { setError(String(e)); }
  }
  async function validate() {
    if (!pdfId) return;
    setBusy(true); setError(''); setReport(null);
    try { setReport(await api.validatePdf(pdfId, profile)); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  return <section className="pdf-standards" aria-label={t('standards.title')}>
    <p className="hint">{t(validator ? 'standards.validator_ready' : 'standards.validator_missing')}</p>
    <p className="field-label">{t('standards.attachments')}</p>
    <p className="hint">{t('standards.attachments_hint')}</p>
    <button className="secondary" disabled={!desktop || busy || attachments.length >= 20} onClick={attach}>{t('standards.add_attachment')}</button>
    {attachments.map((file, i) => <div className="attachment-item" key={file.id}>
      <strong>{file.name}</strong>
      <label>{t('standards.relationship')}<select disabled={busy} value={file.relationship} onChange={(e) => onAttachments(attachments.map((a, n) => n === i ? { ...a, relationship: e.target.value as AttachmentSelection['relationship'] } : a))}>
        {(['Supplement', 'Data', 'Source', 'Alternative', 'Unspecified'] as const).map((r) => <option key={r} value={r}>{t(`standards.relationship.${r}`)}</option>)}
      </select></label>
      <label>{t('standards.description')}<input disabled={busy} maxLength={400} value={file.description} onChange={(e) => onAttachments(attachments.map((a, n) => n === i ? { ...a, description: e.target.value } : a))} /></label>
      <button className="text-button" disabled={busy} aria-label={`${t('standards.remove')} ${file.name}`} onClick={() => onAttachments(attachments.filter((_, n) => n !== i))}>{t('standards.remove')}</button>
    </div>)}
    <div className="separator" />
    <p className="field-label">{t('standards.validate_existing')}</p>
    <p className="hint">{pdfName ?? t('standards.select_pdf')}</p>
    <label>{t('standards.profile')}<select disabled={busy} value={profile} onChange={(e) => setProfile(e.target.value as OutputFormat)}>{STANDARD_FORMATS.map((f) => <option key={f} value={f}>{formatLabel(f)}</option>)}</select></label>
    <button className="secondary" disabled={busy || !pdfId || !validator || !desktop} onClick={validate}>{t('standards.validate')}</button>
    <div aria-live="polite">{report && <ValidationDetails report={report} language={language} />}</div>
    {error && <p className="error" role="alert">{error}</p>}
  </section>;
}
