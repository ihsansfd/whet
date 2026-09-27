import fs from 'node:fs';
import path from 'node:path';
import { paths } from '../core/config.ts';

/**
 * Points a harness's rules file at whet's rules. An existing file is imported
 * when whet has no rules yet, and backed up either way.
 */
export function linkRules(target: string): string[] {
  const log: string[] = [];
  fs.mkdirSync(path.dirname(target), { recursive: true });

  const stat = fs.lstatSync(target, { throwIfNoEntry: false });
  if (stat?.isSymbolicLink() && path.resolve(path.dirname(target), fs.readlinkSync(target)) === paths.rules) {
    return [`${target} already points at ${paths.rules}`];
  }
  if (stat) {
    const content = fs.readFileSync(target, 'utf8');
    const whetHasRules = fs.existsSync(paths.rules) && fs.readFileSync(paths.rules, 'utf8').trim().length > 0;
    if (content.trim() && !whetHasRules) {
      fs.writeFileSync(paths.rules, content);
      log.push(`imported ${target} into ${paths.rules}`);
    }
    if (!stat.isSymbolicLink() && content.trim()) {
      const backup = `${target}.bak-${Date.now()}`;
      fs.copyFileSync(target, backup);
      log.push(`backed up ${target} to ${backup}`);
    }
    fs.rmSync(target);
  }
  fs.symlinkSync(paths.rules, target);
  log.push(`linked ${target} → ${paths.rules}`);
  return log;
}
