// Compara el escáner nativo (WMI) con el de PowerShell, campo por campo. Solo lectura.
// Uso: node tools/parity-scan.js [ruta al .exe]   (con o sin administrador; el .exe de admin hay que lanzarlo elevado)
const { spawnSync } = require('child_process');
const exe = process.argv[2] || 'R:\\target\\release\\botox-optimizer.exe';

function run(args) {
  const t0 = Date.now();
  const r = spawnSync(exe, args, { encoding: 'utf8', maxBuffer: 1 << 26 });
  if (!r.stdout) { console.error('No se pudo lanzar el .exe (¿necesita administrador?):', r.error && r.error.message); process.exit(2); }
  return { spec: JSON.parse(r.stdout), ms: Date.now() - t0 };
}

// Con --files se comparan dos resultados ya guardados (útil con el .exe de administrador, que hay que lanzar elevado):
//   node tools/parity-scan.js --files nativo.json powershell.json
let nat, ps;
if (process.argv[2] === '--files') {
  const read = f => ({ spec: JSON.parse(require('fs').readFileSync(f, 'utf8').replace(/^﻿/, '')), ms: 0 });
  [nat, ps] = [read(process.argv[3]), read(process.argv[4])];
} else {
  nat = run(['--scan']);
  ps = run(['--scan', 'ps']);
  console.log(`nativo: ${nat.ms} ms · PowerShell: ${ps.ms} ms`);
}

const diffs = [];
(function cmp(a, b, path) {
  if (typeof a === 'number' && typeof b === 'number') { if (Math.abs(a - b) > (path === 'os.uptimeH' ? 0.5 : 0.05)) diffs.push([path, a, b]); return; }
  if (Array.isArray(a) && Array.isArray(b)) {
    if (a.length !== b.length) diffs.push([path + '.length', a.length, b.length]);
    a.forEach((x, i) => cmp(x, b[i], `${path}[${i}]`));
    return;
  }
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) cmp(a[k], b[k], path ? `${path}.${k}` : k);
    return;
  }
  // Un campo ausente o vacío en PowerShell cuenta igual que null/'' en el nativo.
  const empty = v => v === undefined || v === null || v === '';
  if (a !== b && !(empty(a) && empty(b))) diffs.push([path, a, b]);
})(nat.spec, ps.spec, '');

// Diferencias esperadas (no son errores del nativo):
//  - bios: PowerShell convierte la fecha de la BIOS a hora local y en zonas UTC- la corre un día. El nativo muestra la real.
//  - reserved: PowerShell lo lee con un cmdlet de DISM que exige administrador; sin admin devuelve vacío.
const known = ([p, a, b]) =>
  (p === 'bios' && String(a).split(' (')[0] === String(b).split(' (')[0] && Math.abs(Date.parse(String(a).match(/\((.*)\)/)[1]) - Date.parse(String(b).match(/\((.*)\)/)[1])) <= 864e5) ||
  (p === 'reserved' && ps.spec.admin === false && b === '');
const real = diffs.filter(d => !known(d)), expected = diffs.filter(known);
for (const [p, a, b] of expected) console.log(`  (esperada) ${p}: ${JSON.stringify(a)} | ${JSON.stringify(b)}`);
if (!real.length) console.log('IGUALES: el escáner nativo y el de PowerShell coinciden en todos los campos' + (expected.length ? ' (salvo las esperadas)' : ''));
else { console.log(`${real.length} diferencia(s)  (campo: nativo | PowerShell)`); for (const [p, a, b] of real) console.log(`  ${p}: ${JSON.stringify(a)} | ${JSON.stringify(b)}`); }
process.exitCode = real.length ? 1 : 0;
