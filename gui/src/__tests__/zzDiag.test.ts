import { it } from 'vitest';
import { parser } from '../editor/reifyParser.js';

const cases = [
  'structure def F { let m = map { 1 => 2 }}',
  'structure def F { let a = 1mm }}',
  'structure def F { let s = "a" let t = "b" }',
  'structure def F { let s = "a {b}" let t = "c" }',
  'structure def F { let s = "" }',
  'import "foo.ri"',
  'structure def F { let s = "{{braces}}" }',
  'structure def F { let s = "a\\"b" }',
  'structure def F { let s = "50%" }',
];

it('diag', () => {
  for (const src of cases) {
    const c = parser.parse(src).cursor();
    const names: string[] = [];
    let errors = 0;
    do {
      if (c.type.isError) errors++;
      if (['String', 'InterpolatedString', 'StringChunk', 'Interpolation'].includes(c.type.name)) {
        names.push(`${c.type.name}[${c.from},${c.to}]`);
      }
    } while (c.next());
    console.log(`err=${errors} ${JSON.stringify(src)} :: ${names.join(' ')}`);
  }
});
