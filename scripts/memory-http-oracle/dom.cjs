// Actual shipped/native scripts run in an owned DOM against a real native owner.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const {JSDOM, ResourceLoader, VirtualConsole} = require(process.env.MEMORY_DOM_MODULE);
const [base, token, output, legacyScript] = process.argv.slice(2);
const records = [], errors = [], checks = [];
class OwnedResources extends ResourceLoader {
  fetch(url, options) {
    assert.equal(new URL(url).origin, base, 'external DOM resource');
    if (legacyScript && url.endsWith('/app.js')) return Promise.resolve(fs.readFileSync(legacyScript));
    return super.fetch(url, options);
  }
}
process.on('unhandledRejection', error => errors.push(String(error)));
const console = new VirtualConsole();
console.on('jsdomError', error => errors.push(String(error)));
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(condition, label) {
  const end = Date.now() + 5000;
  while (Date.now() < end) { if (condition()) { checks.push(label); return; } await pause(20); }
  throw new Error('DOM condition missing: '+label+' errors='+errors);
}
(async () => {
  const dom = await JSDOM.fromURL(base+'/', {
    runScripts:'dangerously', resources:new OwnedResources(), virtualConsole:console,
    beforeParse(window) {
      window.confirm=()=>true;
      window.fetch=async (url, options) => {
        assert.equal(new URL(url).origin, base, 'external API request');
        const auth_before=window.document.querySelector("#auth-status")?.textContent;
        const response=await fetch(url,options);
        records.push({url,options,auth_before,status:response.status,body_hex:Buffer.from(await response.clone().arrayBuffer()).toString('hex')});
        return response;
      };
    }
  });
  const d=dom.window.document, element=s=>d.querySelector(s), click=s=>element(s).click();
  await until(()=>d.readyState==='complete','native HTML and external assets loaded');
  element('#token-input').value='invalid';click('#auth-btn');
  if (legacyScript) {
    await until(()=>records.some(r=>r.url.endsWith('/api/list')&&r.auth_before==='Authenticated'),'original public-status authentication defect reproduced');
    await until(()=>records.some(r=>r.url.endsWith('/api/list')&&r.status===401),'original falsely-authenticated protected read failed');
  } else {
    await until(()=>element('#auth-status').textContent==='Unauthorized','invalid token correctly denied');
    assert.equal(element('#memories-body').children.length,0);
    assert(records.some(r=>r.url.endsWith('/api/list?limit=1')&&r.status===401));
    assert(!records.some(r=>r.url.endsWith('/api/status')));
  }
  element('#token-input').value=token;click('#auth-btn');
  await until(()=>element('#memories-body').textContent.includes('Blue fox sample'),'valid token loaded actual memory row');
  element('#search-query').value='Blue fox sample';click('#search-btn');
  if (legacyScript) {
    await until(()=>errors.some(e=>e.includes('substring')),'original nested search shape defect reproduced');
    assert.equal(element('#search-body').children.length,0);
  } else {
    await until(()=>element('#search-body').textContent.includes('Blue fox sample'),'nested search memory rendered');
    assert.equal(element('#search-body .score-cell').textContent,'0.6000');checks.push('actual nonempty similarity score rendered');
    click('[data-tab="rules"]');await until(()=>element('#rules-body').textContent.includes('Blue fox rule'),'rules row rendered');
    click('[data-tab="entities"]');await until(()=>element('#entities-body').textContent.includes('Blue fox'),'entity row rendered');
    element('#scope-filter').value='project';element('#scope-filter').dispatchEvent(new dom.window.Event('change'));
    await until(()=>element('#memories-empty').classList.contains('visible'),'scope filter showed actual empty list');
    element('#scope-filter').value='';element('#scope-filter').dispatchEvent(new dom.window.Event('change'));
    await until(()=>element('#memories-body').children.length===1,'all scopes restored');
    element('#memory-content').value='Azure fox specimen';click('#add-memory-btn');
    await until(()=>element('#memories-body').textContent.includes('Azure fox specimen'),'actual HTTP memory write rendered');
    const row=Array.from(element('#memories-body').children).find(r=>r.textContent.includes('Azure fox specimen'));
    row.querySelector('.delete-btn').click();
    await until(()=>!element('#memories-body').textContent.includes('Azure fox specimen'),'actual deletion reflected in list');
    assert.equal(errors.length,0,'native DOM errors');
  }
  fs.writeFileSync(output,JSON.stringify({mode:legacyScript?'frozen-original-defects':'corrected-native-assets',runtime:process.version,checks,records,errors},null,2)+'\n');
  dom.window.close();
})().catch(error=>{fs.writeFileSync(output,JSON.stringify({checks,records,errors,failure:String(error)},null,2)+'\n');process.exitCode=1;});
