import { useEffect, useRef, useState } from 'react';
import { api, type PdfSecurityReport } from './api';
import { PdfPreview, type PreviewLabels } from './Preview';
import { translate, type Key, type Language } from './i18n';

export function SecurityInspector({ id, name, language, busy, labels }: {
  id: string; name: string; language: Language; busy: boolean; labels: PreviewLabels;
}) {
  const [report, setReport] = useState<PdfSecurityReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [opening, setOpening] = useState(false);
  const [error, setError] = useState('');
  const [data, setData] = useState<ArrayBuffer | null>(null);
  const [attempt, setAttempt] = useState(0);
  const generation = useRef(0);
  const t = (key: Key) => translate(language, key);

  useEffect(() => {
    const current = ++generation.current;
    setReport(null); setData(null); setError(''); setLoading(true); setOpening(false);
    if (busy) return;
    api.inspectPdf(id).then((result) => { if (generation.current === current) setReport(result); })
      .catch((e) => { if (generation.current === current) setError(String(e)); })
      .finally(() => { if (generation.current === current) setLoading(false); });
    return () => { generation.current++; };
  }, [id, attempt, busy]);

  async function open() {
    if (!report?.fingerprint || busy || opening) return;
    const current = generation.current;
    setOpening(true); setError('');
    try {
      const bytes = await api.openInspectedPdf(id, report.fingerprint);
      if (current === generation.current) setData(bytes);
    } catch (e) {
      if (current === generation.current) { setData(null); setReport(null); setError(String(e)); }
    } finally { if (current === generation.current) setOpening(false); }
  }

  const finding = (value: boolean | null | undefined) => t(value === true ? 'security.found' : value === false ? 'security.not_found' : 'security.unknown');
  return <section className="security-inspector" aria-labelledby="security-title" aria-busy={loading || opening}>
    <div className="section-head"><div><h2 id="security-title">{t('security.title')}</h2><p className="security-name">{name}</p></div>
      <button className="secondary" disabled={busy || loading || opening} onClick={() => setAttempt((n) => n + 1)}>{t('security.rescan')}</button></div>
    <p className="hint">{t('security.intro')}</p>
    <dl className="security-findings" aria-live="polite">
      {(['javascript', 'embedded_files', 'internet_links'] as const).map((key) => <div key={key}>
        <dt>{t(`security.${key}`)}</dt><dd className={report?.[key] ? 'security-found' : ''}>{finding(report?.[key])}</dd>
      </div>)}
      <div><dt>{t('security.encryption')}</dt><dd>{t(`security.algorithm.${report?.encryption.algorithm ?? 'unknown'}`)}
        {report?.encryption.algorithm === 'rc4' && report.encryption.bits ? ` · ${report.encryption.bits}-bit` : ''}
        {report?.encryption.revision != null ? ` · R${report.encryption.revision}` : ''}</dd></div>
    </dl>
    <p role="status">{loading ? t('security.loading') : report ? t(`security.status.${report.status}`) : t('security.status.unreadable')}</p>
    <p className="hint">{t('security.limits')}</p>
    {error && <p className="error" role="alert">{error}</p>}
    {!data && <button className="secondary security-open" disabled={busy || loading || opening || !report?.fingerprint} onClick={open}>{t(opening ? 'security.opening' : 'security.open')}</button>}
    {data && !busy && <PdfPreview data={data} labels={labels} />}
  </section>;
}
