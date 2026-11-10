// Mide cuánto tarda el chequeo de cada tweak que usa PowerShell, todos en una misma sesión (solo lectura).
// Uso: node tools/time-checks.js
const { execFileSync } = require('child_process');
const fs = require('fs'), os = require('os'), path = require('path');
const cat = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'assets', 'catalog.json'), 'utf8'));

let script = cat.prelude + '\n$sw=[Diagnostics.Stopwatch]::StartNew(); "inicio|0"\n';
for (const t of cat.tweaks.filter(t => !t.native)) {
  script += `$t0=$sw.ElapsedMilliseconds; try { $null = & { ${t.ps.check} } } catch {}; "${t.id}|" + ($sw.ElapsedMilliseconds-$t0)\n`;
}
const f = path.join(os.tmpdir(), 'bx-time.ps1');
fs.writeFileSync(f, '﻿' + script, 'utf8');
const t0 = Date.now();
const out = execFileSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', f], { encoding: 'utf8' });
fs.unlinkSync(f);
const rows = out.trim().split(/\r?\n/).map(l => l.split('|')).filter(r => r.length === 2).map(([id, ms]) => [id, +ms]).sort((a, b) => b[1] - a[1]);
console.log(`total del proceso: ${Date.now() - t0} ms · suma de chequeos: ${rows.reduce((a, r) => a + r[1], 0)} ms`);
rows.slice(0, 12).forEach(([id, ms]) => console.log(String(ms).padStart(6) + ' ms  ' + id));
