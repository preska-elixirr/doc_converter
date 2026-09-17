import { useEffect, useRef, useState } from 'react';
import * as pdfjs from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

const MAX_PAGES = 40;
const PAGE_WIDTH = 380;

export type PreviewLabels = {
  empty: string;
  error: string;
  page: (number: number) => string;
  more: (count: number) => string;
};

type Props = { data: ArrayBuffer | null; labels: PreviewLabels; onPages?: (pages: number) => void };

/** Renders every page of a PDF (bundled PDF.js, no network) as a canvas. */
export function PdfPreview({ data, labels, onPages }: Props) {
  const container = useRef<HTMLDivElement>(null);
  const labelsRef = useRef(labels);
  labelsRef.current = labels;
  const [error, setError] = useState('');

  useEffect(() => {
    const host = container.current;
    if (!host) return;
    host.replaceChildren();
    setError('');
    if (!data) return;
    let cancelled = false;
    const task = pdfjs.getDocument({ data: new Uint8Array(data.slice(0)) });
    (async () => {
      try {
        const doc = await task.promise;
        if (cancelled) return;
        onPages?.(doc.numPages);
        const count = Math.min(doc.numPages, MAX_PAGES);
        for (let number = 1; number <= count; number++) {
          const page = await doc.getPage(number);
          if (cancelled) return;
          const base = page.getViewport({ scale: 1 });
          const scale = (PAGE_WIDTH / base.width) * (window.devicePixelRatio || 1);
          const viewport = page.getViewport({ scale });
          const wrap = document.createElement('div');
          wrap.className = 'page-wrap';
          const canvas = document.createElement('canvas');
          canvas.className = 'page-sheet';
          canvas.width = viewport.width;
          canvas.height = viewport.height;
          canvas.style.width = `${viewport.width / (window.devicePixelRatio || 1)}px`;
          canvas.style.height = `${viewport.height / (window.devicePixelRatio || 1)}px`;
          const caption = document.createElement('p');
          caption.className = 'page-caption';
          caption.textContent = labelsRef.current.page(number);
          wrap.append(canvas, caption);
          host.append(wrap);
          await page.render({ canvas, viewport }).promise;
        }
        if (doc.numPages > MAX_PAGES) {
          const more = document.createElement('p');
          more.className = 'page-caption';
          more.textContent = labelsRef.current.more(doc.numPages - MAX_PAGES);
          host.append(more);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
      task.destroy().catch(() => undefined);
    };
  }, [data, onPages]);

  return (
    <div className="pages">
      {error && <p className="empty">{labels.error} {error}</p>}
      {!data && !error && <p className="empty">{labels.empty}</p>}
      <div ref={container} className="pages-inner" />
    </div>
  );
}
