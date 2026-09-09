import { test } from 'node:test';
import assert from 'node:assert';
import { validateArgs, checkExternalUrl } from './check-broken-links.mjs';
import http from 'node:http';

test('test_check_broken_links_validateArgs_valid', () => {
  assert.deepStrictEqual(validateArgs([]), { scanAll: false, checkExternal: false });
  assert.deepStrictEqual(validateArgs(['--all']), { scanAll: true, checkExternal: false });
  assert.deepStrictEqual(validateArgs(['--external']), { scanAll: false, checkExternal: true });
  assert.deepStrictEqual(validateArgs(['--all', '--external']), { scanAll: true, checkExternal: true });
});

test('test_check_broken_links_validateArgs_invalid', () => {
  assert.deepStrictEqual(validateArgs('not-an-array'), { error: 'args-not-array' });
  assert.deepStrictEqual(validateArgs(['--invalid']), { error: 'invalid-arg=--invalid' });
  assert.deepStrictEqual(validateArgs([undefined]), { error: 'invalid-arg=undefined' });
});

test('test_check_broken_links_checkExternalUrl_success', async () => {
  const server = http.createServer((req, res) => {
    res.writeHead(200);
    res.end();
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`);
  assert.deepStrictEqual(result, { ok: true });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_head_fail_get_success', async () => {
  const server = http.createServer((req, res) => {
    if (req.method === 'HEAD') {
      res.writeHead(405);
      res.end();
    } else {
      res.writeHead(200);
      res.end();
    }
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`);
  assert.deepStrictEqual(result, { ok: true });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_retry_success', async () => {
  let attempt = 0;
  const server = http.createServer((req, res) => {
    attempt++;
    if (attempt < 2) {
      res.writeHead(503);
    } else {
      res.writeHead(200);
    }
    res.end();
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`, 3, 5000, 10);
  assert.deepStrictEqual(result, { ok: true });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_retry_failure', async () => {
  const server = http.createServer((req, res) => {
    res.writeHead(503);
    res.end();
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`, 1, 5000, 10);
  assert.deepStrictEqual(result, { ok: false, status: 503 });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_timeout', async () => {
  const server = http.createServer((req, res) => {
    // Hangs forever
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`, 1, 50, 10);
  assert.strictEqual(result.ok, false);
  assert.strictEqual(result.error, 'TimeoutError');

  server.close();
});


test('test_check_broken_links_checkExternalUrl_head_fail_get_retry', async () => {
  let getAttempt = 0;
  const server = http.createServer((req, res) => {
    if (req.method === 'HEAD') {
      res.writeHead(405);
      res.end();
    } else {
      getAttempt++;
      if (getAttempt < 2) {
        res.writeHead(503);
      } else {
        res.writeHead(200);
      }
      res.end();
    }
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`, 2, 5000, 10);
  assert.deepStrictEqual(result, { ok: true });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_head_fail_get_fail', async () => {
  const server = http.createServer((req, res) => {
    if (req.method === 'HEAD') {
      res.writeHead(405);
    } else {
      res.writeHead(404);
    }
    res.end();
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`, 1, 5000, 10);
  assert.deepStrictEqual(result, { ok: false, status: 404 });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_head_fail_get_retry_fail', async () => {
  const server = http.createServer((req, res) => {
    if (req.method === 'HEAD') {
      res.writeHead(405);
    } else {
      res.writeHead(503);
    }
    res.end();
  });

  await new Promise(resolve => server.listen(0, resolve));
  const port = server.address().port;

  const result = await checkExternalUrl(`http://localhost:${port}`, 1, 5000, 10);
  assert.deepStrictEqual(result, { ok: false, status: 503 });

  server.close();
});

test('test_check_broken_links_checkExternalUrl_network_error', async () => {
  const result = await checkExternalUrl(`http://localhost:1`, 1, 5000, 10);
  assert.strictEqual(result.ok, false);
  assert.strictEqual(typeof result.error, 'string');
});

test('test_check_broken_links_checkExternalUrl_network_error_retry', async () => {
  const result = await checkExternalUrl(`http://localhost:1`, 2, 5000, 10);
  assert.strictEqual(result.ok, false);
  assert.strictEqual(typeof result.error, 'string');
});
