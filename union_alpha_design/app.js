const $ = (id) => document.getElementById(id);
const modes = {
  convert: { title: 'New format. Same peace of mind.', description: "Bring your documents together. We'll take care of the format.", label: 'Convert', eyebrow: 'CONVERSION', formats: ['PDF', 'DOCX', 'TXT', 'HTML', 'MD'], action: 'Preview conversion', hint: 'PDF, Word, Excel, PowerPoint, text and more' },
  images: { title: 'The right size. A fresh perspective.', description: 'Change formats and dimensions without touching your original images.', label: 'Images', eyebrow: 'IMAGE OUTPUT', formats: ['PNG', 'JPG', 'WEBP', 'PDF'], action: 'Preview image conversion', hint: 'PNG, JPG, WebP, TIFF and BMP' },
  clean: { title: 'Share your work. Leave the history.', description: 'Make a cleaner copy before your files leave your hands.', label: 'Clean for sharing', eyebrow: 'CLEAN COPIES', formats: [], action: 'Preview cleaned copies', hint: 'Word documents, PDFs and photos' },
  encrypt: { title: 'For your eyes. Or theirs alone.', description: 'Protect a separate copy, and keep the original within reach.', label: 'Encrypt', eyebrow: 'FILE PROTECTION', formats: [], action: 'Preview encryption', hint: 'Any file for .age encryption, or PDFs for PDF protection' },
  decrypt: { title: 'Your files, back in your hands.', description: 'Explore restoring an encrypted file or unlocking a protected PDF.', label: 'Decrypt', eyebrow: 'RESTORE A COPY', formats: [], action: 'Preview decryption', hint: 'Password-protected PDF and .age files' }
};
const samples = {
  convert: [['Project brief.docx', 248320], ['Quarterly report.xlsx', 1150976], ['Meeting notes.md', 18432], ['Brand guidelines.pptx', 3774873]],
  images: [['Studio portrait.jpg', 2831155], ['Brand symbol.png', 435200], ['Product detail.webp', 991232]],
  clean: [['Client proposal.docx', 517120], ['Annual report.pdf', 1456128], ['Team photo.jpg', 2516582]],
  encrypt: [['Private agreement.pdf', 421888], ['Financial records.xlsx', 850944], ['Project archive.zip', 5242880]],
  decrypt: [['Private agreement.pdf', 430080], ['Project archive.zip.age', 5251072]]
};
let mode = 'convert';
let files = [];
let sequence = 0;
let batchFormat = 'PDF';
let busy = false;
let timer;
let demoItems = [];
let completed = 0;
const imageExtensions = ['png', 'jpg', 'jpeg', 'webp', 'tif', 'tiff', 'bmp'];
const documentExtensions = ['pdf', 'docx', 'doc', 'odt', 'rtf', 'xlsx', 'xls', 'ods', 'pptx', 'ppt', 'odp', 'txt', 'md', 'html', 'htm'];
const extension = (name) => name.split('.').pop().toLowerCase();
const selected = () => files.filter((file) => file.selected);
const sizeLabel = (size) => size >= 1048576 ? `${(size / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(size / 1024))} KB`;
const show = (id, visible) => { $(id).hidden = !visible; };
const icon = (name) => {
  const element = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
  use.setAttribute('href', `#i-${name}`);
  element.setAttribute('aria-hidden', 'true');
  element.append(use);
  return element;
};
const element = (tag, className, text) => {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
};
function eligible(file) {
  const ext = extension(file.name);
  if (mode === 'convert') return documentExtensions.includes(ext);
  if (mode === 'images') return imageExtensions.includes(ext);
  if (mode === 'clean') return ['docx', 'pdf', ...imageExtensions].includes(ext);
  if (mode === 'encrypt') return $('security-type').value === 'age' || ext === 'pdf';
  return ['age', 'pdf'].includes(ext);
}
function target(file) {
  if (mode === 'clean') return imageExtensions.includes(extension(file.name)) ? 'PNG' : extension(file.name).toUpperCase();
  if (mode === 'encrypt') return $('security-type').value === 'age' ? 'AGE' : 'PDF';
  if (mode === 'decrypt') return extension(file.name) === 'pdf' ? 'PDF' : 'Original';
  if (mode === 'convert' && $('merge').checked) return 'PDF';
  return file.format;
}
function needsPassword() {
  return ['encrypt', 'decrypt'].includes(mode) || (mode === 'convert' && $('protect').checked);
}
function loadSamples() {
  if (busy) return;
  files = samples[mode].map(([name, size]) => ({ id: ++sequence, name, size, selected: true, format: batchFormat, status: '' }));
  show('result', false);
  render();
}
function addFiles(list) {
  if (busy) return;
  for (const file of list) files.push({ id: ++sequence, name: file.name, size: file.size, selected: true, format: batchFormat, status: '' });
  $('file-input').value = '';
  show('result', false);
  render();
}
function changeMode(next) {
  if (busy || next === mode) return;
  mode = next;
  batchFormat = modes[mode].formats[0] || '';
  for (const file of files) { file.format = batchFormat; file.status = ''; }
  $('password').value = '';
  $('confirm-password').value = '';
  $('password').type = 'password';
  $('show-password').textContent = 'Show';
  $('show-password').setAttribute('aria-label', 'Show password');
  $('merge').checked = false;
  $('protect').checked = false;
  $('watermark').checked = false;
  show('result', false);
  render();
}
function renderFormats() {
  const formats = modes[mode].formats;
  $('batch-format').replaceChildren();
  $('format-tiles').replaceChildren();
  for (const format of formats) {
    const option = new Option(format === 'MD' ? 'Markdown · .md' : `${format} · .${format.toLowerCase()}`, format);
    $('batch-format').add(option);
    const button = element('button', `tile${batchFormat === format ? ' active' : ''}`, format);
    button.setAttribute('aria-pressed', String(batchFormat === format));
    button.setAttribute('aria-label', `Set selected output to ${format}`);
    button.disabled = busy || (mode === 'convert' && $('merge').checked);
    button.addEventListener('click', () => setFormat(format));
    $('format-tiles').append(button);
  }
  $('batch-format').value = batchFormat;
  $('batch-format').disabled = busy || (mode === 'convert' && $('merge').checked);
}
function setFormat(format) {
  if (busy) return;
  batchFormat = format;
  for (const file of selected()) { file.format = format; file.status = ''; }
  render();
}
function renderRows() {
  const previousPreview = $('preview-file').value;
  $('file-rows').replaceChildren();
  $('preview-file').replaceChildren();
  for (const file of files) {
    const allowed = eligible(file);
    const row = element('tr', file.selected ? 'checked' : '');
    const selectCell = element('td');
    const checkbox = element('input', 'row-checkbox');
    checkbox.type = 'checkbox';
    checkbox.checked = file.selected;
    checkbox.disabled = busy;
    checkbox.setAttribute('aria-label', `Select ${file.name}`);
    checkbox.addEventListener('change', () => { file.selected = checkbox.checked; file.status = ''; render(); });
    selectCell.append(checkbox);
    const nameCell = element('td');
    const fileCell = element('div', 'file-cell');
    const ext = extension(file.name);
    const category = imageExtensions.includes(ext) ? 'image' : ['doc', 'docx'].includes(ext) ? 'word' : ['xls', 'xlsx'].includes(ext) ? 'sheet' : ['ppt', 'pptx'].includes(ext) ? 'slides' : ext === 'pdf' ? 'pdf' : 'text';
    const badge = element('span', `file-type ${category}`, ext.toUpperCase().slice(0, 4));
    const stack = element('div', 'name-stack');
    const name = element('div', 'name', file.name);
    name.title = file.name;
    stack.append(name, element('div', 'size', sizeLabel(file.size)));
    fileCell.append(badge, stack);
    nameCell.append(fileCell);
    const outputCell = element('td');
    if (['convert', 'images'].includes(mode) && allowed) {
      const output = element('select', 'output-select');
      for (const format of modes[mode].formats) output.add(new Option(format, format));
      output.value = target(file);
      output.disabled = busy || (mode === 'convert' && $('merge').checked);
      output.setAttribute('aria-label', `Output format for ${file.name}`);
      output.addEventListener('change', () => { file.format = output.value; file.status = ''; updateSummary(); });
      outputCell.append(output);
    } else outputCell.append(element('span', 'output-label', allowed ? target(file) : '—'));
    const statusCell = element('td');
    const status = !allowed ? 'skipped' : file.status || (file.selected ? 'ready' : 'unselected');
    const statusNames = { ready: 'Ready', skipped: 'Not eligible', unselected: 'Not selected', working: 'Demo running', done: 'Demo complete' };
    statusCell.append(element('span', `status ${status}`, statusNames[status]));
    const removeCell = element('td');
    const remove = element('button', 'row-remove');
    remove.setAttribute('aria-label', `Remove ${file.name}`);
    remove.disabled = busy;
    remove.append(icon('close'));
    remove.addEventListener('click', () => { files = files.filter((item) => item.id !== file.id); render(); });
    removeCell.append(remove);
    row.append(selectCell, nameCell, outputCell, statusCell, removeCell);
    $('file-rows').append(row);
    $('preview-file').add(new Option(file.name, String(file.id)));
  }
  if (files.some((file) => String(file.id) === previousPreview)) $('preview-file').value = previousPreview;
  if (!files.length) {
    const row = element('tr');
    const cell = element('td', 'empty-queue', 'Your workspace is clear. Add files or load samples to try it.');
    cell.colSpan = 5;
    row.append(cell);
    $('file-rows').append(row);
    $('preview-file').add(new Option('Add a file to explore the preview', ''));
  }
  $('preview-file').disabled = busy || !files.length;
  updatePreviewLabel();
}
function updatePreviewLabel() {
  const file = files.find((item) => String(item.id) === $('preview-file').value);
  $('preview-note').textContent = file ? `Exploring ${file.name}. The page above is illustrative sample content, not a rendering of this file.` : 'Add a file or load samples to explore the output settings. This page uses sample content only.';
}
function updateSummary() {
  const active = selected();
  const ready = active.filter(eligible);
  const skipped = active.length - ready.length;
  const allPdf = ready.length > 0 && ready.every((file) => target(file) === 'PDF');
  const watermarkAvailable = mode === 'convert' && allPdf;
  $('watermark').disabled = busy || !watermarkAvailable;
  $('protect').disabled = busy || !allPdf;
  if (!allPdf && mode === 'convert') { $('watermark').checked = false; $('protect').checked = false; }
  show('password-options', needsPassword());
  show('confirm-wrap', mode !== 'decrypt');
  show('paper-watermark', watermarkAvailable && $('watermark').checked);
  show('watermark-text', watermarkAvailable && $('watermark').checked);
  $('quality').disabled = busy || (mode === 'images' && !ready.some((file) => ['JPG', 'WEBP'].includes(target(file))));
  $('file-count').textContent = files.length;
  $('selection-count').textContent = `${active.length} selected`;
  $('applies-to').textContent = `${active.length} files selected`;
  $('total-size').textContent = `${files.length} files · ${sizeLabel(files.reduce((sum, file) => sum + file.size, 0))}`;
  $('select-all').checked = files.length > 0 && active.length === files.length;
  $('select-all').indeterminate = active.length > 0 && active.length < files.length;
  $('select-all').disabled = busy || files.length === 0;
  $('ready-summary').textContent = `${ready.length} ${ready.length === 1 ? 'file' : 'files'} ready${skipped ? ` · ${skipped} skipped` : ''}`;
  const targets = new Set(ready.map(target));
  $('output-summary').textContent = mode === 'convert' && $('merge').checked ? '1 combined PDF' : targets.size > 1 ? 'Mixed output' : [...targets][0] || 'No output';
  let validation = '';
  if (!ready.length) validation = active.length ? 'No eligible files. Load samples for this tool or add other files.' : 'Select at least one file to try the workflow.';
  if (needsPassword() && !$('password').value) validation = 'Enter a sample password to continue.';
  if (needsPassword() && mode !== 'decrypt' && $('password').value !== $('confirm-password').value) validation = 'The passwords do not match.';
  if (mode === 'convert' && $('merge').checked && !$('merge-name').value.trim()) validation = 'Give the combined PDF a filename.';
  if (mode === 'convert' && $('watermark').checked && !$('watermark-text').value.trim()) validation = 'Enter some watermark text.';
  $('validation').textContent = validation || (skipped ? `${skipped} selected ${skipped === 1 ? 'file is' : 'files are'} not eligible for this operation.` : '');
  show('validation', Boolean($('validation').textContent));
  $('run').disabled = busy || Boolean(validation) || !ready.length;
  $('run-label').textContent = busy ? 'Preview in progress…' : mode === 'convert' && $('merge').checked ? 'Preview combined PDF' : modes[mode].action;
}
function render() {
  const current = modes[mode];
  document.querySelectorAll('[data-mode]').forEach((button) => {
    button.classList.toggle('active', button.dataset.mode === mode);
    button.setAttribute('aria-pressed', String(button.dataset.mode === mode));
    button.disabled = busy;
  });
  $('breadcrumb-mode').textContent = current.label;
  $('page-title').textContent = current.title;
  $('page-description').textContent = current.description;
  $('settings-eyebrow').textContent = current.eyebrow;
  $('drop-hint').textContent = current.hint;
  show('format-section', ['convert', 'images'].includes(mode));
  show('convert-options', mode === 'convert');
  show('image-options', mode === 'images');
  show('clean-options', mode === 'clean');
  show('security-options', mode === 'encrypt');
  show('advanced', mode === 'convert');
  show('merge-field', mode === 'convert' && $('merge').checked);
  $('paper').classList.toggle('muted-preview', !['convert', 'images'].includes(mode));
  document.querySelectorAll('.settings-body input, .settings-body select, [data-orientation]').forEach((control) => { control.disabled = busy; });
  for (const id of ['add-files', 'file-input', 'dropzone', 'samples', 'clear', 'show-password']) $(id).disabled = busy;
  renderFormats();
  renderRows();
  updateSummary();
}
function finish(cancelled) {
  clearInterval(timer);
  busy = false;
  for (const file of files) if (file.status === 'working') file.status = '';
  $('password').value = '';
  $('confirm-password').value = '';
  $('result-title').textContent = cancelled ? 'Demo cancelled' : 'That is how your workflow would finish.';
  $('result-description').textContent = cancelled ? `${completed} of ${demoItems.length} steps previewed. No files were processed or saved.` : `${demoItems.length} ${demoItems.length === 1 ? 'file' : 'files'} previewed. Nothing was converted, encrypted, cleaned or saved.`;
  $('result-action').textContent = 'Dismiss';
  if (!cancelled) $('progress').value = 100;
  render();
}
function runDemo() {
  if ($('run').disabled || busy) return;
  busy = true;
  demoItems = selected().filter(eligible);
  completed = 0;
  for (const file of files) file.status = '';
  demoItems[0].status = 'working';
  $('progress').value = 0;
  $('result-title').textContent = 'Previewing the workflow';
  $('result-description').textContent = `Simulating step 1 of ${demoItems.length}. No file contents are read.`;
  $('result-action').textContent = 'Cancel demo';
  show('result', true);
  render();
  $('result').scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth', block: 'nearest' });
  $('result-action').focus({ preventScroll: true });
  timer = setInterval(() => {
    demoItems[completed].status = 'done';
    completed += 1;
    $('progress').value = completed / demoItems.length * 100;
    if (completed === demoItems.length) { finish(false); return; }
    demoItems[completed].status = 'working';
    $('result-description').textContent = `Simulating step ${completed + 1} of ${demoItems.length}. No file contents are read.`;
    renderRows();
  }, 700);
}
document.querySelectorAll('[data-mode]').forEach((button) => button.addEventListener('click', () => changeMode(button.dataset.mode)));
$('add-files').addEventListener('click', () => $('file-input').click());
$('dropzone').addEventListener('click', () => $('file-input').click());
$('file-input').addEventListener('change', (event) => addFiles(event.target.files));
$('samples').addEventListener('click', loadSamples);
$('clear').addEventListener('click', () => { files = []; show('result', false); render(); });
$('select-all').addEventListener('change', () => { for (const file of files) file.selected = $('select-all').checked; render(); });
$('batch-format').addEventListener('change', () => setFormat($('batch-format').value));
$('merge').addEventListener('change', render);
$('protect').addEventListener('change', updateSummary);
$('security-type').addEventListener('change', () => {
  $('security-hint').textContent = $('security-type').value === 'age' ? 'Make an encrypted copy of any file.' : 'Only PDF inputs are eligible. Convert other documents first.';
  render();
});
for (const id of ['password', 'confirm-password', 'merge-name', 'watermark-text']) $(id).addEventListener('input', () => {
  $('paper-watermark').textContent = $('watermark-text').value;
  updateSummary();
});
$('show-password').addEventListener('click', () => {
  const visible = $('password').type === 'password';
  $('password').type = visible ? 'text' : 'password';
  $('show-password').textContent = visible ? 'Hide' : 'Show';
  $('show-password').setAttribute('aria-label', visible ? 'Hide password' : 'Show password');
});
$('watermark').addEventListener('change', updateSummary);
$('quality').addEventListener('input', () => { $('quality-value').textContent = `${$('quality').value}%`; });
$('preview-file').addEventListener('change', updatePreviewLabel);
$('margins').addEventListener('change', () => { $('paper').dataset.margins = $('margins').value; });
document.querySelectorAll('[data-orientation]').forEach((button) => button.addEventListener('click', () => {
  document.querySelectorAll('[data-orientation]').forEach((item) => {
    item.classList.toggle('active', item === button);
    item.setAttribute('aria-pressed', String(item === button));
  });
  $('paper').classList.toggle('landscape', button.dataset.orientation === 'landscape');
}));
for (const event of ['dragover', 'drop']) document.addEventListener(event, (e) => e.preventDefault());
$('dropzone').addEventListener('dragover', () => { if (!busy) $('dropzone').classList.add('armed'); });
$('dropzone').addEventListener('dragleave', () => $('dropzone').classList.remove('armed'));
$('dropzone').addEventListener('drop', (event) => { $('dropzone').classList.remove('armed'); addFiles(event.dataTransfer.files); });
$('run').addEventListener('click', runDemo);
$('result-action').addEventListener('click', () => {
  if (busy) finish(true);
  else { show('result', false); $('run').focus(); }
});
$('about').addEventListener('click', () => $('about-dialog').showModal());
for (const id of ['close-about', 'back-to-workspace']) $(id).addEventListener('click', () => $('about-dialog').close());
$('about-dialog').addEventListener('click', (event) => { if (event.target === $('about-dialog')) $('about-dialog').close(); });
loadSamples();
