const $ = (id) => document.getElementById(id);
let mode = 'convert', protection = 'pdf', busy = false, timer, serial = 0;
let files = [];
const formats = ['PDF', 'DOCX', 'TXT', 'HTML', 'MD'];
const imageTypes = ['JPG','JPEG','PNG','WEBP','TIFF','TIF','BMP'];
const imageFormats = ['PNG','JPG','WEBP','PDF'];
const merging = () => mode==='convert' && $('merge').checked;
const outputFor = f => merging()?'PDF':mode==='images'?f.imageTarget || $('image-format').value:f.target;
const escapeHtml = (s) => String(s).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function makeFile(name, size, demo = true) {
  return {id: ++serial, name, size, demo, type: name.split('.').pop().toUpperCase(), target:'PDF', selected:true, status:''};
}
function loadSamples() {
  files = mode === 'images' ? [makeFile('Product photo.jpg','3.2 MB'),makeFile('Company logo.png','240 KB'),makeFile('Scanned receipt.tiff','4.8 MB')] : mode === 'decrypt'
    ? [makeFile('Quarterly report 2026.pdf', '2.1 MB'), makeFile('Client agreement.docx.dcenc', '184 KB')]
    : [makeFile('Quarterly report 2026.docx','2.1 MB'),makeFile('Invoice_0342.pdf','640 KB'),makeFile('notes.md','12 KB'),makeFile('Team offsite deck.pptx','9.6 MB')];
  clearResult(); render();
}
function eligible(file) {
  if(mode === 'images') return imageTypes.includes(file.type);
  if(mode === 'encrypt' && protection === 'pdf') return file.type === 'PDF';
  if(mode === 'decrypt') return ['PDF','DCENC'].includes(file.type);
  if(mode === 'convert') return ['PDF','DOCX','PPTX','XLSX','MD','HTML','TXT','ODT'].includes(file.type);
  return true;
}
function needsPassword() { return ['encrypt','decrypt'].includes(mode) || (mode==='convert' && $('protect').checked && selected().some(f=>outputFor(f)==='PDF')); }
function selected() { return files.filter(f=>f.selected && eligible(f)); }
function render() {
  $('count').textContent = files.length;
  $('files').innerHTML = files.length ? files.map(f=> {
    const allowed=eligible(f), reason=mode==='decrypt'?'Unsupported file':mode==='encrypt'?'PDF required':'Unsupported file';
    const output=merging()?'<span class="output-label">Merged PDF</span>':['convert','images'].includes(mode)?`<select class="row-format" data-target="${f.id}" aria-label="Output format for ${escapeHtml(f.name)}" ${busy?'disabled':''}>${(mode==='images'?imageFormats:formats).map(t=>`<option ${t===outputFor(f)?'selected':''}>${t}</option>`).join('')}</select>`:`<span class="output-label">${mode==='encrypt'?(protection==='pdf'?'PDF + key':'.dcenc'):'Original'}</span>`;
    return `<tr><td><input type="checkbox" data-select="${f.id}" aria-label="Select ${escapeHtml(f.name)}" ${f.selected?'checked':''} ${busy?'disabled':''}></td><td><div class="file"><span class="file-type ${f.type.toLowerCase()}">${escapeHtml(f.type.slice(0,5))}</span><div style="min-width:0"><span class="filename" title="${escapeHtml(f.name)}">${escapeHtml(f.name)}</span><span class="file-meta">${escapeHtml(f.size)}${f.demo?' · Example file':''}</span></div></div></td><td>${output}</td><td><span class="status ${!allowed?'blocked':''}">${!allowed?reason:(f.status || 'Ready')}</span></td><td><button class="remove" data-remove="${f.id}" aria-label="Remove ${escapeHtml(f.name)}" ${busy?'disabled':''}>×</button></td></tr>`;
  }).join('') : '<tr><td colspan="5" class="empty">Your queue is empty. Add files to get started.</td></tr>';
  const checked=files.filter(f=>f.selected).length;
  $('selection-count').textContent=`${checked} selected`;
  $('select-all').checked=files.length>0 && checked===files.length;
  $('select-all').indeterminate=checked>0 && checked<files.length;
  $('select-all').disabled=busy || !files.length;
  $('password-settings').hidden=!needsPassword();
  $('action-summary').textContent=`${selected().length} document${selected().length===1?'':'s'} ready`;
  const outputs=new Set(selected().map(outputFor));
  $('summary-type').textContent=merging()?'1 combined PDF':['convert','images'].includes(mode)?(outputs.size===1?[...outputs][0]:'Mixed formats'):mode==='encrypt'?(protection==='pdf'?'Protected PDF':'Encrypted copy'):'Restored copy';
  $('clear').disabled=busy || !files.length;
  ['add-files','samples','dropzone','batch-format','protect','password','confirm-password'].forEach(id=>$(id).disabled=busy);
  document.querySelectorAll('[data-mode],input[name="encryption-type"]').forEach(el=>el.disabled=busy);
  validate();
  if(window.renderLayout) window.renderLayout();
}
function validate() {
  const password=$('password').value, confirmation=$('confirm-password').value;
  const valid=!needsPassword() || (mode==='decrypt'?password.length>0:password.length>=12 && password===confirmation);
  $('password-error').textContent=mode!=='decrypt' && confirmation && password!==confirmation?'Passwords do not match.':'';
  $('confirm-password').setAttribute('aria-invalid',String(Boolean(confirmation && password!==confirmation)));
  $('run').disabled=busy || !selected().length || !valid || (merging() && !$('merge-name').value.trim());
  $('run').innerHTML=busy?'Preview in progress…':`Preview ${merging()?'merge':['convert','images'].includes(mode)?'conversion':mode==='encrypt'?'encryption':'decryption'} <span aria-hidden="true">→</span>`;
  const skipped=files.filter(f=>f.selected && !eligible(f)).length;
  $('action-note').textContent=skipped?`${skipped} unsupported file${skipped===1?' is':'s are'} skipped · preview only`:'Preview only · no files are processed';
}
function clearResult() { $('result').hidden=true; files.forEach(f=>f.status=''); }
function switchMode(next) {
  if(busy) return;
  mode=next;
  try { localStorage.setItem('doc-converter-mode',mode); } catch {}
  clearResult();
  $('password').value=''; $('confirm-password').value='';
  $('password').type='password'; $('confirm-password').type='password'; $('show-password').textContent='Show'; $('show-password').setAttribute('aria-label','Show password');
  document.querySelectorAll('[data-mode]').forEach(b=>{b.classList.toggle('active',b.dataset.mode===mode);b.setAttribute('aria-pressed',String(b.dataset.mode===mode));});
  ['convert','encrypt','decrypt'].forEach(m=>$(m+'-settings').hidden=m!==mode);
  $('image-settings').hidden=mode!=='images';
  $('confirm-field').hidden=mode==='decrypt';
  $('password-label').textContent=mode==='decrypt'?'Document password':'Create a password';
  $('password').autocomplete=mode==='decrypt'?'current-password':'new-password';
  $('password-hint').textContent=mode==='decrypt'?'Files with different passwords should be processed separately.':'Use at least 12 characters. A longer phrase is easier to remember.';
  $('settings-title').textContent=mode==='convert'?'Output settings':mode==='encrypt'?'Protection settings':'Decryption settings';
  $('settings-subtitle').textContent=mode==='convert'?'Make these files work for you.':mode==='encrypt'?'Choose how to protect your documents.':'Restore a readable copy.';
  $('queue-description').textContent=mode==='convert'?'Choose an output format for each document.':mode==='encrypt'?'Choose the documents you want to protect.':'Add files you have the password for.';
  $('drop-title').textContent=mode==='decrypt'?'Drop protected files here':'Drop documents here';
  $('drop-types').textContent=mode==='decrypt'?'Password-protected PDF or .dcenc files':'PDF, Word, Excel, PowerPoint, text & more';
  $('output-heading').textContent=mode==='decrypt'?'RESTORE TO':'OUTPUT';
  if(mode==='images') {
    $('settings-title').textContent='Image settings'; $('settings-subtitle').textContent='The right format. The right size.';
    $('queue-description').textContent='Convert and resize your images.';
    $('drop-title').textContent='Drop images here'; $('drop-types').textContent='JPG, PNG, WebP, TIFF and BMP';
  }
  render();
}
function addFiles(list) {
  if(busy) return;
  for(const f of list) files.push(makeFile(f.name,f.size>=1048576?`${(f.size/1048576).toFixed(1)} MB`:`${Math.max(1,Math.round(f.size/1024))} KB`,false));
  clearResult(); render();
}
document.querySelectorAll('[data-mode]').forEach(b=>b.onclick=()=>switchMode(b.dataset.mode));
document.querySelectorAll('[name="encryption-type"]').forEach(r=>r.onchange=()=>{protection=r.value;clearResult();$('encryption-note').textContent=protection==='pdf'?'PDF files only. Convert other documents to PDF first.':'Creates a .dcenc copy. Use Decrypt to return it to its original format.';render();});
$('files').addEventListener('change',e=>{if(busy)return;const id=Number(e.target.dataset.select || e.target.dataset.target),f=files.find(f=>f.id===id);if(!f)return;if(e.target.dataset.select)f.selected=e.target.checked;else if(mode==='images')f.imageTarget=e.target.value;else f.target=e.target.value;clearResult();render();});
$('files').addEventListener('click',e=>{const button=e.target.closest('[data-remove]');if(!button||busy)return;files=files.filter(f=>f.id!==Number(button.dataset.remove));clearResult();render();});
$('select-all').onchange=e=>{files.forEach(f=>f.selected=e.target.checked);clearResult();render();};
$('batch-format').onchange=e=>{files.filter(f=>f.selected).forEach(f=>f.target=e.target.value);clearResult();render();};
$('protect').onchange=()=>{clearResult();render();};
$('password').oninput=validate; $('confirm-password').oninput=validate;
$('show-password').onclick=()=>{const show=$('password').type==='password';$('password').type=show?'text':'password';$('confirm-password').type=show?'text':'password';$('show-password').textContent=show?'Hide':'Show';$('show-password').setAttribute('aria-label',show?'Hide password':'Show password');};
$('clear').onclick=()=>{files=[];clearResult();render();};
$('samples').onclick=loadSamples;
$('add-files').onclick=$('dropzone').onclick=()=>$('file-input').click();
$('file-input').onchange=e=>{addFiles(e.target.files);e.target.value='';};
$('dropzone').ondragover=e=>{e.preventDefault();if(!busy)$('dropzone').classList.add('drag');};
$('dropzone').ondragleave=()=>$('dropzone').classList.remove('drag');
$('dropzone').ondrop=e=>{e.preventDefault();$('dropzone').classList.remove('drag');addFiles(e.dataTransfer.files);};
// The design preview never reads file contents, processes documents, or persists passwords.
$('run').onclick=()=>{
  if($('run').disabled)return;
  busy=true;const batch=selected();let step=0;
  batch.forEach(f=>f.status='Queued');
  $('result').hidden=false;$('progress').hidden=false;$('progress').value=0;
  $('result-title').textContent='Previewing the workflow…';
  $('result-detail').textContent='Simulated progress. Your files are not being changed.';
  $('result-action').textContent='Cancel preview';render();
  $('result-action').focus();
  timer=setInterval(()=>{
    step++; const completed=Math.floor(step/3);
    batch.forEach((f,i)=>f.status=i<completed?'Demo complete':i===completed?'Previewing…':'Queued');
    $('progress').value=Math.min(100,step/(batch.length*3)*100);
    if(completed>=batch.length){clearInterval(timer);busy=false;$('result-title').textContent='Workflow preview complete';$('result-detail').textContent=`${batch.length} document${batch.length===1?'':'s'} demonstrated. No converted, encrypted, or decrypted files were created.`;$('result-action').textContent='Dismiss';$('password').value='';$('confirm-password').value='';}
    render();
  },350);
};
$('result-action').onclick=()=>{if(busy){clearInterval(timer);busy=false;files.forEach(f=>f.status='');$('result-title').textContent='Preview cancelled';$('result-detail').textContent='Your original files are unchanged.';$('progress').hidden=true;$('result-action').textContent='Dismiss';render();}else{$('result').hidden=true;$('run').focus();}};
try {const saved=localStorage.getItem('doc-converter-mode');if(['convert','images','encrypt','decrypt'].includes(saved))mode=saved;}catch{}
loadSamples();switchMode(mode);
