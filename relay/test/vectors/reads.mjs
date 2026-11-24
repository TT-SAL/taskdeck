// "Rows read, at most" of relay.md §16.1, as formulas. The platform counts every
// row a statement touches, the rows a write changes included (measured under
// `wrangler dev`), so these are bounds on that count.
//   n   rows in `nonces` when the request arrives (at most 64)
//   D   rows in `devices`     B   rows in `blobs`     t   tickets (at most 4)
//   RT  rows in `read_tokens`
const A = (n = 1) => 8 + 2 * n; // every admin request before its own checks: the token's row three
// times, the board row twice, the signer's row twice, the nonces twice, the nonce it deletes
export const READS = {
  create: 3,
  createRepeat: 4,
  nonce: (n) => 3 + 3 * n,
  login: (n) => 8 + 2 * n,
  hello: 3,
  read: (k) => 4 + k,
  append: ({ frames, others }) => 5 + 3 * (others + 1) + 2 * frames, // 5 + 3a + 2 x frames, a the authors
  blobBegin: (B) => 4 + B,
  blobPut: (B) => 7 + B,
  blobPutSame: 4,
  blobList: (B) => 2 + B,
  blobMeta: 3,
  blobGet: 4,
  blobDelete: (P) => 2 + P,
  pairOpen: (t) => 3 + t,
  slotGet: 3,
  slotPost: 4,
  register: (D, frame) => A() + D + (frame ? 6 : 3),
  registerRepeat: (D) => A() + D + 5,
  adminRefused: (D) => A() + D + 3,
  mint: (RT) => A() + 3 + RT,
  revoke: (D, RT) => A() + D + 1 + RT,
  revokeRead: A() + 2,
  fence: (frame) => A() + 1 + (frame ? 5 : 0),
  restore: (f, a) => A() + 1 + 3 * a + f,
  purge: (RT, d, gone) => A() + 6 + gone + RT + 3 * d + 3 * gone,
  compact: (B, d, gone) => A() + 8 + B + d + 3 * gone,
  recovery: (D) => A() + 2 * D + 3,
  recoveryRepeat: (D) => A() + D + 2,
  seed: (entries) => 4 + 2 + entries,
  seedTail: (f, a) => 5 + 3 * a + 2 * f,
};
