(function(){
var pn=null,pa=null,_em=null;
function $(i){return document.getElementById(i)}
function sm(t,y){var m=$('msg');m.textContent=t;m.className='msg msg-'+y;m.style.display='block';setTimeout(function(){m.style.display='none'},3000)}
function sp(i){document.querySelectorAll('.page').forEach(function(p){p.classList.remove('active')});$(i).classList.add('active')}
function gt(){var m=document.cookie.match(/csrf-token=([^;]+)/);return m?m[1]:''}

window.login=function(){
var u=$('username').value.trim();var p=$('password').value;var e=$('loginError');var b=$('loginBtn');if(!u||!p){e.textContent=__('login_required');return}
e.textContent='';b.disabled=true;b.textContent=__('logging_in');
fetch('/webui/api/login',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({username:u,password:p})}).then(function(r){return r.json()}).then(function(d){if(d.code===200){sp('dashboardPage');$('uDisplay').textContent=d.data.username;ld()}else{e.textContent=d.message||__('login_failed')}
b.disabled=false;b.textContent=__('login')}).catch(function(){e.textContent=__('network_error');b.disabled=false;b.textContent=__('login')})};

window.logout=function(){fetch('/webui/api/logout',{method:'POST',headers:{'Content-Type':'application/json','X-CSRF-Token':gt()}}).then(function(){$('serviceGrid').innerHTML='';$('serverStats').innerHTML='';sp('loginPage');$('loginError').textContent=''})};

window.ca=function(n,a){pn=n;pa=a;
var ls={start:__('start'),stop:__('stop'),restart:__('restart'),'force-stop':__('force_stop'),delete:__('delete_service')};
$('cTitle').textContent=__('confirm')+' '+ls[a];
$('cMsg').textContent=(a==='delete'?__('delete_confirm')+' "'+n+'"?':__('confirm_action')+' '+ls[a]+' '+__('confirm_service')+' "'+n+'"? '+__('confirm_undo'));
var cb=$('cBtn');cb.className='btn'+(a==='delete'||a==='force-stop'?' btn-danger':' btn-primary');cb.textContent=ls[a];
$('cModal').classList.add('active')};

window.cc=function(){pn=null;pa=null;$('cModal').classList.remove('active')};

window.ec=function(){if(!pn||!pa){cc();return}var n=pn,a=pa;cc();
if(a==='stop_first')return;
if(a==='delete'){dc(n);return}
sa(n,a)};

function sa(n,a){var g=$('serviceGrid');var ab=g.querySelectorAll('.service-card-actions button');ab.forEach(function(b){b.disabled=true})
fetch('/webui/api/service/'+encodeURIComponent(n)+'/'+a,{method:'POST',headers:{'Content-Type':'application/json','X-CSRF-Token':gt()}}).then(function(r){return r.json()}).then(function(d){sm(n+': '+(d.message||(d.code===200?'OK':__('network_error'))),d.code===200?'success':'error');ls()}).catch(function(){sm(__('network_error'),'error');ab.forEach(function(b){b.disabled=false})})}

function dc(n){fetch('/webui/api/service/'+encodeURIComponent(n)+'/delete',{method:'POST',headers:{'Content-Type':'application/json','X-CSRF-Token':gt()}}).then(function(r){return r.json()}).then(function(d){if(d.code===409){sm(__('running_stop_first'),'error');return}sm(n+': '+(d.message||(d.code===200?'OK':__('network_error'))),d.code===200?'success':'error');if(d.code===200)ls()}).catch(function(){sm(__('network_error'),'error')})}

window.sr=function(n,a){pn=n;pa='stop_first';
$('cTitle').textContent=__('confirm');
$('cMsg').textContent=__('running_stop_first')+' "'+n+'"';
$('cBtn').textContent='OK';
$('cBtn').className='btn btn-primary';
$('cModal').classList.add('active')};

function fd(s){if(s===null||s===undefined)return'-';if(s<60)return s+'s';if(s<3600){var m=Math.floor(s/60);return m+'m '+(s%60)+'s'}var h=Math.floor(s/3600);s=s%3600;m=Math.floor(s/60);return h+'h '+m+'m'}
function fb(b){if(!b)return'0 B';var u=['B','KB','MB','GB'];for(var i=0;i<u.length-1&&b>=1024;i++){b/=1026}return b.toFixed(1)+' '+u[i]}
function fp(v){return v!==undefined&&v!==null?v.toFixed(1)+'%':'-'}

function usc(c,d,n){var b=c.querySelector('.status-badge'),st=b.querySelector('.s-text'),de=c.querySelector('.dur'),re=c.querySelector('.retry'),ce=c.querySelector('.cmd'),ac=c.querySelector('.service-card-actions')
if(d.operation){b.className='status-badge status-pending';st.textContent=__('op_in_progress')}else if(d.running){b.className='status-badge status-running';st.textContent=__('running')}else{b.className='status-badge status-stopped';st.textContent=__('stopped')}
de.textContent=fd(d.duration);re.textContent=d.abnormalExitCount;ce.textContent=d.command||'-'
ac.innerHTML='';if(d.operation){ac.innerHTML='<span style="font-size:12px;color:#666">'+__('op_in_progress')+'</span>';return}
function mkBtn(b){var btn=document.createElement('button');btn.className='btn btn-sm '+b.c;btn.textContent=b.t;
if(b.a==='edit'){btn.onclick=function(){if(d.running){sr(n,'edit')}else{ecf(n)}}}else if(b.a==='delete'){btn.onclick=function(){if(d.running){sr(n,'delete')}else{ca(n,'delete')}}}else{btn.onclick=function(){ca(n,b.a)}}
return btn}
var opBt=[],mgBt=[]
if(!d.running){opBt.push({t:__('start'),c:'btn-primary',a:'start'})}else{opBt.push({t:__('stop'),c:'',a:'stop'});opBt.push({t:__('restart'),c:'',a:'restart'});opBt.push({t:__('force_stop'),c:'btn-danger',a:'force-stop'})}
mgBt.push({t:__('edit'),c:'',a:'edit'});mgBt.push({t:__('delete'),c:'btn-danger',a:'delete'})
opBt.forEach(function(b){ac.appendChild(mkBtn(b))})
if(opBt.length&&mgBt.length){var sp=document.createElement('span');sp.style.cssText='display:inline-block;width:1px;height:20px;background:#ccc;margin:0 6px;vertical-align:middle';ac.appendChild(sp)}
mgBt.forEach(function(b){ac.appendChild(mkBtn(b))})}

function ls(){fetch('/webui/api/services',{method:'GET',headers:{'X-CSRF-Token':gt()}}).then(function(r){if(r.status===401){sp('loginPage');return null}return r.json()}).then(function(d){if(!d)return;if(d.code!==200)return;var g=$('serviceGrid');var ns=Object.keys(d.data);if(ns.length===0){g.innerHTML='<p style="padding:20px;border:1px solid #ddd;background:#fff">'+__('no_services')+'</p>';return}
var ex={};g.querySelectorAll('.service-card').forEach(function(c){var n=c.getAttribute('data-n');if(n)ex[n]=c})
ns.forEach(function(n){var dt=d.data[n];var c=ex[n];if(c){usc(c,dt,n)}else{c=document.createElement('div');c.className='service-card';c.setAttribute('data-n',n);c.innerHTML='<div class="service-card-header"><h3>'+n+'</h3><span class="status-badge status-stopped"><span class="s-text">-</span></span></div><div class="service-card-body"><div class="row"><span data-i="command">'+__('command')+'</span><span class="cmd">-</span></div><div class="row"><span data-i="duration">'+__('duration')+'</span><span class="dur">-</span></div><div class="row"><span data-i="abnormal_exits">'+__('abnormal_exits')+'</span><span class="retry">0</span></div></div><div class="service-card-actions"></div>';g.appendChild(c);usc(c,dt,n)}})
for(var n in ex){if(!d.data[n]){ex[n].remove()}}})}

function lss(){fetch('/webui/api/server-status',{method:'GET',headers:{'X-CSRF-Token':gt()}}).then(function(r){if(r.status===401){sp('loginPage');return null}return r.json()}).then(function(d){if(!d||d.code!==200)return;var s=d.data;var st=$('serverStats');st.innerHTML='<div class="section-title" data-i="server_status">'+__('server_status')+'</div><div class="stats-grid">'+
'<div class="stat-card"><div class="stat-value">'+fp(s.cpu)+'</div><div class="stat-label" data-i="cpu_usage">'+__('cpu_usage')+'</div></div>'+
'<div class="stat-card"><div class="stat-value">'+fp(s.memory.usage)+'</div><div class="stat-label" data-i="memory_usage">'+__('memory_usage')+'</div></div>'+
'<div class="stat-card"><div class="stat-value">'+fb(s.memory.used)+' / '+fb(s.memory.total)+'</div><div class="stat-label" data-i="memory">'+__('memory')+'</div></div>'+
'<div class="stat-card"><div class="stat-value">'+fd(s.uptime)+'</div><div class="stat-label" data-i="system_uptime">'+__('system_uptime')+'</div></div>'+
'</div>'})}

function ld(){ls();lss()}

window._sl=function(l){localStorage.setItem('lang',l);fetch('/webui/lang/'+l+'.json').then(function(r){return r.json()}).then(function(t){window.__T=t;document.querySelectorAll('[data-i]').forEach(function(e){var k=e.getAttribute('data-i');if(window.__T[k])e.textContent=window.__T[k]});document.querySelectorAll('.lang-btn').forEach(function(b){b.classList.toggle('active',b.getAttribute('data-lang')===l)});$('loginError').textContent='';ld()})};

window.ncf=function(){_em=null;$('cfTitle').textContent=__('create_service');$('cfId').value='';$('cfId').readOnly=false;$('cfCmd').value='';$('cfCwd').value='';$('cfPri').value='999';$('cfRet').value='';$('cfAutoStart').checked=false;$('cfRetryOnExit').checked=true;$('cfLogEnabled').checked=false;$('cfLogDays').value='';$('cfErr').textContent='';$('cfBtn').textContent=__('create');$('cfModal').classList.add('active');setTimeout(function(){$('cfId').focus()},100)};

window.ecf=function(n){_em=n;fetch('/webui/api/services',{method:'GET',headers:{'X-CSRF-Token':gt()}}).then(function(r){return r.json()}).then(function(d){if(!d||d.code!==200)return;var dt=d.data[n];if(!dt)return;$('cfTitle').textContent=__('edit_service')+' - '+n;$('cfId').value=n;$('cfId').readOnly=true;$('cfCmd').value=dt.command||'';$('cfCwd').value=dt.cwd||'';$('cfPri').value=dt.priority!==undefined?dt.priority:999;$('cfRet').value=dt.maxRetries!==undefined?dt.maxRetries:'';$('cfAutoStart').checked=!!dt.autoStart;$('cfRetryOnExit').checked=dt.retryOnAbnormalExit!==false;$('cfLogEnabled').checked=!!(dt.log&&dt.log.enabled);$('cfLogDays').value=(dt.log&&dt.log.retentionDays)?dt.log.retentionDays:'';$('cfErr').textContent='';$('cfBtn').textContent=__('save');$('cfModal').classList.add('active')})};

window.hcf=function(){$('cfModal').classList.remove('active');_em=null};

window.scf=function(){var id=$('cfId').value.trim();var cmd=$('cfCmd').value.trim();var cwd=$('cfCwd').value.trim();if(!cmd){$('cfErr').textContent=__('command_required');return}
if(!cwd){$('cfErr').textContent=__('cwd_required');return}
if(!id||!/^[a-zA-Z0-9_\-]+$/.test(id)){$('cfErr').textContent=__('invalid_id');return}
var roe=$('cfRetryOnExit').checked,le=$('cfLogEnabled').checked;
var rv=$('cfRet').value.trim(),lv=$('cfLogDays').value.trim();
if(roe&&!rv){$('cfErr').textContent=__('retry_required');return}
if(le&&!lv){$('cfErr').textContent=__('log_days_required');return}
var body={id:id,command:cmd,cwd:cwd,priority:parseInt($('cfPri').value,10)||999,maxRetries:roe?parseInt(rv,10)||3:0,retryOnAbnormalExit:roe,autoStart:$('cfAutoStart').checked,log:{enabled:le,retentionDays:le?parseInt(lv,10)||7:7}};
if(_em){fetch('/webui/api/service/'+encodeURIComponent(id)+'/edit',{method:'POST',headers:{'Content-Type':'application/json','X-CSRF-Token':gt()},body:JSON.stringify(body)}).then(function(r){return r.json()}).then(function(d){$('cfErr').textContent='';if(d.code===200){hcf();sm(__('service_updated')+': '+id,'success');ls()}else{$('cfErr').textContent=d.message||__('network_error')}}).catch(function(){$('cfErr').textContent=__('network_error')})}else{
fetch('/webui/api/service/new',{method:'POST',headers:{'Content-Type':'application/json','X-CSRF-Token':gt()},body:JSON.stringify(body)}).then(function(r){return r.json()}).then(function(d){$('cfErr').textContent='';if(d.code===200){hcf();sm(__('service_created')+': '+id,'success');ls()}else{if(d.code===409){$('cfErr').textContent=__('id_exists')}else{$('cfErr').textContent=d.message||__('network_error')}}}).catch(function(){$('cfErr').textContent=__('network_error')})}};

_ir(function(){
$('password').addEventListener('keydown',function(e){if(e.key==='Enter')login()});
fetch('/webui/api/services',{method:'GET',headers:{'X-CSRF-Token':gt()}}).then(function(r){if(r.status===401){sp('loginPage')}else{sp('dashboardPage');ld()}}).catch(function(){sp('loginPage')});
setInterval(function(){if($('dashboardPage').classList.contains('active')){ls();lss()}},5000)
});
})();