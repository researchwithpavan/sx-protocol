const assert = require('assert');

try {
  const { sxVersion } = require('../src/index');
  const version = sxVersion();
  assert.ok(version.includes('SX Protocol'));
  console.log('ok');
} catch (err) {
  if (!process.env.SX_FFI_LIB) {
    console.log('skipped: set SX_FFI_LIB to run JS FFI smoke test');
    process.exit(0);
  }
  throw err;
}
