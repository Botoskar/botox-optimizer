// Exporta el catálogo de la app Electron (../../tweaks.js) a assets/catalog.json para la app en Rust.
// Uso: node tools/export-catalog.js
// "native" = el tweak solo tiene registro fijo, servicios y/o energía: Rust lo maneja sin PowerShell.
// El resto conserva sus scripts de PowerShell ya generados (apply / revert / check).
const fs = require('fs');
const path = require('path');
const T = require('../../tweaks');

const src = f => (f ? f.toString().replace(/\s+/g, ' ') : null);
const tweaks = T.TWEAKS.map(t => {
  const native = !(t.L || t.nic || t.apply || t.revert || t.check || t.pre);
  return {
    id: t.id, cat: t.cat, name: t.name, desc: t.desc, risk: t.risk || 'low', reboot: !!t.reboot, debloat: !!t.debloat,
    uses_reg: !!(t.reg || t.L), rec: src(t.rec), needs: src(t.needs), why: src(t.why),
    native, ac: !!t.ac,
    reg: native ? (t.reg || []).map(([p, n, v, ty, def, svc]) => ({ p, n, v, t: ty, def: def ?? null, svc: !!svc })) : [],
    pwr: native ? (t.pwr || []).map(([sub, set, v, def]) => ({ sub, set, v, def })) : [],
    ps: native ? null : { apply: T.applyPS(t), revert: T.revertPS(t), check: T.checkPS(t) },
    details: T.INFO[t.id]?.d ?? null, source: T.INFO[t.id]?.s ?? null,
  };
});
const sinTweak = Object.keys(T.INFO).filter(id => !T.TWEAKS.some(t => t.id === id));
if (sinTweak.length) throw new Error('INFO tiene ids que no existen: ' + sinTweak.join(', '));
const out = { prelude: T.PRELUDE, scan: require('../../scan'), cats: T.CATS, tweaks, boosts: T.BOOSTS };
fs.writeFileSync(path.join(__dirname, '..', 'assets', 'catalog.json'), JSON.stringify(out, null, 1));

console.log(`con detalle: ${tweaks.filter(t => t.details).length} · con fuente: ${tweaks.filter(t => t.source).length}`);
console.log(`tweaks: ${tweaks.length} · nativos: ${tweaks.filter(t => t.native).length} · PowerShell: ${tweaks.filter(t => !t.native).length} · boosts: ${T.BOOSTS.length}`);
for (const k of ['rec', 'needs', 'why']) console.log(k + ':\n  ' + [...new Set(tweaks.map(t => t[k]).filter(Boolean))].join('\n  '));
console.log('tipos de registro:', [...new Set(tweaks.flatMap(t => t.reg.map(r => r.t)))].join(', '));
