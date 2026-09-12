(function(){
var _lang=localStorage.getItem('lang')||'zh-CN';
window.__T={};
window.__=function(k){return window.__T[k]||k};
window._sl=function(l){_lang=l;localStorage.setItem('lang',l);fetch('/webui/lang/'+l+'.json').then(function(r){return r.json()}).then(function(t){window.__T=t;document.querySelectorAll('[data-i]').forEach(function(e){var k=e.getAttribute('data-i');if(window.__T[k])e.textContent=window.__T[k]});document.querySelectorAll('.lang-btn').forEach(function(b){b.classList.toggle('active',b.getAttribute('data-lang')===l)})})};
window._ir=function(f){var c=function(){if(Object.keys(window.__T).length)f();else setTimeout(c,10)};c()};
window._sl(_lang);
})();