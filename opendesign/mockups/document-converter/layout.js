// Sample layout model: illustrates pagination without parsing uploaded documents.
let orientation='portrait', previewId='', chosenBlock='', breaks=new Set();
const pagedFiles=()=>selected().filter(f=>['PDF','DOCX'].includes(outputFor(f)));
function refreshLayout() { clearResult(); render(); }
function sampleBlocks(f) {
  return [
    {id:`${f.id}-title`,title:f.name.replace(/\.[^.]+$/,''),text:'DOCUMENT OVERVIEW',kind:'heading'},
    {id:`${f.id}-intro`,title:'Overview',text:'This sample shows how your content will sit on the page. Adjust the margins and spacing to find a comfortable reading rhythm.'},
    {id:`${f.id}-detail`,title:'Details & observations',text:'Keep a heading with the text it introduces. If a section needs more room, select this block and move it to the next page. The space above the break stays clear.'},
    {id:`${f.id}-next`,title:'Next steps',text:'Review the complete document before saving. Your chosen orientation, paragraph spacing, and page breaks are reflected in this layout demonstration.'}
  ];
}
window.renderLayout=function() {
  const show=mode==='convert';
  $('document-layout').hidden=!show; $('layout-preview').hidden=!show;
  $('merge-name-field').hidden=!merging(); $('merge-order').hidden=!merging();
  $('batch-format').disabled=busy || merging();
  ['merge','merge-name','preview-file','margins','spacing','reset-breaks','image-format','image-size'].forEach(id=>$(id).disabled=busy);
  document.querySelectorAll('[data-orientation]').forEach(b=>b.disabled=busy);
  const lossy=selected().some(f=>['JPG','WEBP'].includes(outputFor(f)));
  $('image-quality').disabled=busy || !lossy;
  $('quality-note').textContent=lossy?'Quality applies to JPG and WebP outputs. Lower values create smaller files.':'Quality applies to JPG and WebP outputs; PNG and PDF ignore this setting.';
  $('transparency-note').textContent=selected().some(f=>outputFor(f)==='JPG')?'JPG outputs use a white background for transparent areas.':'PNG and WebP outputs keep transparency.';
  if(!show)return;
  const candidates=pagedFiles();
  if(!candidates.some(f=>String(f.id)===previewId))previewId=String(candidates[0]?.id || '');
  $('preview-file').innerHTML=candidates.map(f=>`<option value="${f.id}" ${String(f.id)===previewId?'selected':''}>${escapeHtml(f.name)}</option>`).join('');
  $('preview-file').parentElement.hidden=merging();
  $('order-list').innerHTML=candidates.map((f,i)=>`<div class="order-row"><span class="order-number">${String(i+1).padStart(2,'0')}</span><span class="order-name">${escapeHtml(f.name)}</span><button data-move="${f.id}" data-delta="-1" aria-label="Move ${escapeHtml(f.name)} up" ${i===0 || busy?'disabled':''}>↑</button><button data-move="${f.id}" data-delta="1" aria-label="Move ${escapeHtml(f.name)} down" ${i===candidates.length-1 || busy?'disabled':''}>↓</button></div>`).join('');
  const viewing=merging()?candidates:candidates.filter(f=>String(f.id)===previewId);
  const blocks=viewing.flatMap(sampleBlocks);
  if(!blocks.some(b=>b.id===chosenBlock))chosenBlock='';
  const capacity=(orientation==='portrait'?6:4)+($('margins').value==='narrow'?1:$('margins').value==='wide'?-1:0);
  const cost=$('spacing').value==='spacious'?2:$('spacing').value==='compact'?1:1.5;
  const pages=[];let page=[],used=0;
  blocks.forEach(b=>{const weight=b.kind==='heading'?1:cost;if(page.length && (breaks.has(b.id)||used+weight>capacity)){pages.push(page);page=[];used=0;}page.push(b);used+=weight;});
  if(page.length)pages.push(page);
  $('page-count').textContent=`${pages.length} page${pages.length===1?'':'s'} · A4 ${orientation}`;
  $('pages').className=`pages ${orientation} margin-${$('margins').value} spacing-${$('spacing').value}`;
  $('pages').innerHTML=pages.length?pages.map((group,i)=>`<div class="page-wrap"><div class="page-sheet"><div class="page-running">DOC CONVERTER <span>LAYOUT SAMPLE</span></div>${group.map(b=>`<button class="content-block ${b.kind || ''} ${chosenBlock===b.id?'selected':''}" data-block="${b.id}" aria-pressed="${chosenBlock===b.id}" ${busy?'disabled':''}>${breaks.has(b.id)?'<span class="break-marker">PAGE BREAK</span>':''}<strong>${escapeHtml(b.title)}</strong><span>${escapeHtml(b.text)}</span></button>`).join('')}<div class="page-folio">${i+1}<span>PRIVATE DOCUMENT</span></div></div><p class="page-caption">Page ${i+1}</p></div>`).join(''):'<p class="empty">Select a document with PDF or Word output to preview its page layout.</p>';
  const current=blocks.find(b=>b.id===chosenBlock);
  $('block-selection').textContent=current?`Selected: ${current.title}`:'Select a content block on the page.';
  $('page-break').disabled=busy || !current || blocks[0]?.id===chosenBlock || breaks.has(chosenBlock);
  $('reset-breaks').disabled=busy || !breaks.size;
};
$('merge').onchange=refreshLayout;
$('merge-name').oninput=validate;
document.querySelectorAll('[data-orientation]').forEach(b=>b.onclick=()=>{orientation=b.dataset.orientation;document.querySelectorAll('[data-orientation]').forEach(button=>{const active=button===b;button.classList.toggle('active',active);button.setAttribute('aria-pressed',String(active));});refreshLayout();});
$('preview-file').onchange=e=>{previewId=e.target.value;chosenBlock='';renderLayout();};
['margins','spacing'].forEach(id=>$(id).onchange=refreshLayout);
$('pages').onclick=e=>{const b=e.target.closest('[data-block]');if(!b || busy)return;chosenBlock=b.dataset.block;renderLayout();const replacement=$('pages').querySelector(`[data-block="${chosenBlock}"]`);replacement?.focus({preventScroll:true});};
$('page-break').onclick=()=>{if(!chosenBlock || busy)return;breaks.add(chosenBlock);refreshLayout();};
$('reset-breaks').onclick=()=>{breaks.clear();refreshLayout();};
$('order-list').onclick=e=>{const b=e.target.closest('[data-move]');if(!b || busy)return;const list=pagedFiles(),index=list.findIndex(f=>f.id===Number(b.dataset.move)),other=list[index+Number(b.dataset.delta)];if(!other)return;const a=files.indexOf(list[index]),z=files.indexOf(other);[files[a],files[z]]=[files[z],files[a]];refreshLayout();};
$('image-format').onchange=e=>{selected().forEach(f=>f.imageTarget=e.target.value);refreshLayout();};
$('image-quality').oninput=e=>{$('quality-value').value=e.target.value+'%';clearResult();};
$('image-size').onchange=refreshLayout;
renderLayout();
