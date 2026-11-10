// Compara el estado de todos los tweaks según la app Rust (--check) y según el motor de Electron (PowerShell).
// Solo lectura: no aplica nada. Uso: node tools/parity.js [ruta al .exe]
const { execFileSync, spawnSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const T = require('../../tweaks');

const exe = process.argv[2] || 'M:\\Github Things\\botox-optimizer-rust\\target\\debug\\botox-optimizer.exe';
let t0 = Date.now();
const r = spawnSync(exe, ['--check'], { encoding: 'utf8' });
const rust = JSON.parse(r.stdout);
console.log(`Rust     (${Date.now() - t0} ms)  ${r.stderr.trim()}`);

t0 = Date.now();
const f = path.join(os.tmpdir(), 'bx-parity.ps1');
fs.writeFileSync(f, '\ufeff' + T.PRELUDE + '\n' + T.checkAllPS(), 'utf8');
const out = execFileSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', f], { encoding: 'utf8', maxBuffer: 1 << 26 });
fs.unlinkSync(f);
const electron = JSON.parse(out.slice(out.indexOf('{')));
console.log(`Electron (${Date.now() - t0} ms)`);

const ids = T.TWEAKS.map(t => t.id);
const missing = ids.filter(id => !(id in rust));
const diff = ids.filter(id => id in rust && rust[id] !== electron[id]);
console.log(`Tweaks: ${ids.length} · activos según Rust: ${ids.filter(id => rust[id]).length} · según Electron: ${ids.filter(id => electron[id]).length}`);
if (missing.length) console.log('FALTAN en Rust:', missing.join(', '));
if (diff.length) diff.forEach(id => console.log(`DISTINTO ${id}: rust=${rust[id]} electron=${electron[id]}`));
console.log(!missing.length && !diff.length ? 'PARIDAD OK: los 2 motores coinciden en todos los tweaks' : 'HAY DIFERENCIAS');
process.exit(missing.length || diff.length ? 1 : 0);
