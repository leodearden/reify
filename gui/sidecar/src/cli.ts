import { main } from './index.js';

main().catch((err: unknown) => {
  console.error('Sidecar fatal error:', err);
  process.exit(1);
});
