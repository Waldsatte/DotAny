// The update signature: an Ed25519 signature on SHA256SUMS.txt in minisign's format, so
// the hub (minisign-verify, hub/src-tauri/src/update.rs) can tell a release we made from one someone swapped. Node's own
// crypto only, nothing downloaded.
//
//   node sign-update.mjs keygen <folder>                   a new key pair: <folder>/update-signing-key.txt (secret), .pub
//   node sign-update.mjs sign <file> <key file> [comment]  signs <file> with the secret in <key file>; writes <file>.sig
//   node sign-update.mjs verify <file> <pub>               checks <file>.sig against a public key (base64, minisign form)
//
// The secret is one line, `DOTANYKEY1.<key id, 16 hex>.<seed, base64url>.<public key, base64url>`, in a file of its own.
// It is kept off the repository and off its build service on purpose: anyone who could push to the repository or run
// its workflows could otherwise have a malicious update signed. So a release's SHA256SUMS.txt is signed after the
// build, on the computer that holds the key, and this script refuses to make or read a key inside a git checkout. It
// never prints the secret. The public key is the minisign form, base64("Ed" + key id + key), and is written into the
// hub (TRUSTED_KEYS in update.rs). minisign itself checks the same signature:
//   minisign -Vm SHA256SUMS.txt -x SHA256SUMS.txt.sig -P <public key>
//
// The signature is minisign's "ED" kind: Ed25519 over BLAKE2b-512 of the file, then a second signature over the first
// plus the trusted comment, which therefore can't be changed either.
import {
  createHash,
  createPrivateKey,
  createPublicKey,
  generateKeyPairSync,
  randomBytes,
  sign,
  verify,
} from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const b64u = (b) => Buffer.from(b).toString('base64url');
const fail = (m) => {
  console.error(m);
  process.exit(1);
};

function parseSecret(line) {
  const [tag, id, seed, pub] = String(line || '')
    .trim()
    .split('.');
  if (tag !== 'DOTANYKEY1' || !/^[0-9a-f]{16}$/.test(id || '') || !seed || !pub)
    fail('the key is missing or not a DOTANYKEY1 line.');
  const key = createPrivateKey({ key: { kty: 'OKP', crv: 'Ed25519', d: seed, x: pub }, format: 'jwk' });
  return { id: Buffer.from(id, 'hex'), key, pub: Buffer.from(pub, 'base64url') };
}
const publicB64 = (id, pub) => Buffer.concat([Buffer.from('Ed'), id, pub]).toString('base64');

/** The public key (minisign form) of a secret line. */
export function publicOf(secretLine) {
  const { id, pub } = parseSecret(secretLine);
  return publicB64(id, pub);
}

/** The git checkout `path` is in (the nearest folder up the way that has a .git), or null. */
export function checkoutOf(path) {
  for (let d = resolve(path); ; d = dirname(d)) {
    if (existsSync(join(d, '.git'))) return d;
    if (dirname(d) === d) return null;
  }
}

/** The secret line in a key file. A key inside a git checkout is refused: it is one commit away from being published. */
export function readSecretFile(path) {
  if (!path) fail('no key file given.');
  const repo = checkoutOf(dirname(resolve(path)));
  if (repo)
    fail(`the key file is inside the git checkout ${repo}: keep it in a folder of its own, outside any checkout.`);
  let line;
  try {
    line = readFileSync(path, 'utf8').trim();
  } catch {
    fail(`cannot read the key file ${path}.`);
  }
  parseSecret(line);
  return line;
}

export function signText(data, secretLine, trusted) {
  const { id, key } = parseSecret(secretLine);
  const hash = createHash('blake2b512').update(data).digest();
  const sig = sign(null, hash, key);
  const comment = trusted || `timestamp:${Math.floor(Date.now() / 1000)}`;
  const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), key);
  return `untrusted comment: DotAny update signature\n${Buffer.concat([Buffer.from('ED'), id, sig]).toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}\n`;
}

export function verifyText(data, sigText, pubB64) {
  const pk = Buffer.from(pubB64, 'base64'),
    lines = sigText.split(/\r?\n/);
  const bin = Buffer.from(lines[1] || '', 'base64'),
    global = Buffer.from(lines[3] || '', 'base64');
  if (
    pk.length !== 42
    || bin.length !== 74
    || global.length !== 64
    || !(lines[2] || '').startsWith('trusted comment: ')
  )
    return false;
  if (!bin.subarray(2, 10).equals(pk.subarray(2, 10)) || bin.subarray(0, 2).toString() !== 'ED') return false;
  const key = createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: b64u(pk.subarray(10)) }, format: 'jwk' });
  const sig = bin.subarray(10);
  return (
    verify(null, createHash('blake2b512').update(data).digest(), key, sig)
    && verify(null, Buffer.concat([sig, Buffer.from(lines[2].slice(17))]), key, global)
  );
}

export function keygen() {
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const jwk = privateKey.export({ format: 'jwk' }),
    id = randomBytes(8);
  const pub = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url');
  return { secret: `DOTANYKEY1.${id.toString('hex')}.${jwk.d}.${jwk.x}`, public: publicB64(id, pub) };
}

if (process.argv[1] && import.meta.url.endsWith(process.argv[1].replace(/\\/g, '/').split('/').pop())) {
  const [cmd, a, b, c] = process.argv.slice(2);
  if (cmd === 'keygen') {
    if (!a) fail('keygen <folder>');
    const repo = checkoutOf(a);
    if (repo)
      fail(`${a} is inside the git checkout ${repo}: make the key in a folder of its own, outside any checkout.`);
    const secretPath = join(a, 'update-signing-key.txt');
    if (existsSync(secretPath)) fail(`${secretPath} already exists; not overwriting a key.`);
    mkdirSync(a, { recursive: true });
    const k = keygen();
    writeFileSync(secretPath, k.secret + '\n');
    writeFileSync(join(a, 'update-signing-key.pub'), `untrusted comment: DotAny update public key\n${k.public}\n`);
    console.log(k.public);
  } else if (cmd === 'sign') {
    if (!a || !b) fail('sign <file> <key file> [trusted comment]');
    const secret = readSecretFile(b);
    const data = readFileSync(a);
    const text = signText(data, secret, c);
    // a key that can't check its own signature is the wrong key, or a broken one: never publish that
    if (!verifyText(data, text, publicOf(secret))) fail('the signature does not verify');
    writeFileSync(a + '.sig', text);
    console.log(`signed ${a} -> ${a}.sig`);
  } else if (cmd === 'verify') {
    if (!a || !b) fail('verify <file> <public key>');
    const good = verifyText(readFileSync(a), readFileSync(a + '.sig', 'utf8'), b);
    console.log(good ? 'good' : 'BAD');
    process.exit(good ? 0 : 1);
  } else fail('keygen <folder> | sign <file> <key file> [comment] | verify <file> <public key>');
}
