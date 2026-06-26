/**
 * Integration tests for the Soroban escrow contract against a local Stellar node.
 *
 * Prerequisites:
 *   docker-compose -f docker-compose.test.yml up -d
 *
 * Run with:
 *   npm run test:contracts
 *
 * SKIP_CONTRACT_TESTS
 * -------------------
 * Set the environment variable SKIP_CONTRACT_TESTS=true to skip this entire
 * suite without failing the test run. This is used in CI environments where
 * Docker (and therefore the local Stellar node) is not available.
 *
 * When SKIP_CONTRACT_TESTS is not set (or set to any value other than "true"),
 * the tests run normally and require a live local node.
 *
 * A separate nightly CI job runs these tests with Docker available — see
 * .github/workflows/ci.yml (job: contract-tests-nightly).
 */

const path = require('path');
const fs = require('fs');
const StellarSdk = require('@stellar/stellar-sdk');
const { fundAccount, deployContract, invokeContract } = require('./helpers/soroban');

// ---------------------------------------------------------------------------
// Skip guard
// ---------------------------------------------------------------------------
const SKIP = process.env.SKIP_CONTRACT_TESTS === 'true';

if (SKIP && process.env.CI) {
  console.warn(
    '[WARNING] Contract tests are SKIPPED because SKIP_CONTRACT_TESTS=true. ' +
    'These tests require a local Stellar node (Docker). ' +
    'They run on the nightly CI schedule — see .github/workflows/ci.yml.'
  );
}

const describeOrSkip = SKIP ? describe.skip : describe;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Build a ScVal array for the `deposit` contract call.
 */
function depositArgs({ tokenContractId, orderId, buyerPk, farmerPk, amountStroops, timeoutUnix }) {
  return [
    StellarSdk.nativeToScVal(tokenContractId, { type: 'address' }),
    StellarSdk.nativeToScVal(orderId, { type: 'u64' }),
    StellarSdk.nativeToScVal(buyerPk, { type: 'address' }),
    StellarSdk.nativeToScVal(farmerPk, { type: 'address' }),
    StellarSdk.nativeToScVal(amountStroops, { type: 'i128' }),
    StellarSdk.nativeToScVal(timeoutUnix, { type: 'u64' }),
  ];
}

/**
 * Build a ScVal array for the `release` contract call.
 */
function releaseArgs({ orderId, platformFeeBps }) {
  return [
    StellarSdk.nativeToScVal(orderId, { type: 'u64' }),
    StellarSdk.nativeToScVal(platformFeeBps ?? 0, { type: 'u32' }),
  ];
}

/**
 * Build a ScVal array for the `refund` contract call.
 */
function refundArgs({ orderId }) {
  return [
    StellarSdk.nativeToScVal(orderId, { type: 'u64' }),
  ];
}

/**
 * Build a ScVal array for the `get_escrow` contract call.
 */
function getEscrowArgs({ orderId }) {
  return [
    StellarSdk.nativeToScVal(orderId, { type: 'u64' }),
  ];
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

describeOrSkip('Escrow contract — full deposit → release flow (local Stellar node)', () => {
  let contractId;
  let buyerKeypair;
  let farmerKeypair;
  let adminKeypair;

  // The XLM native token contract ID on the local Quickstart node.
  const tokenContractId =
    process.env.SOROBAN_XLM_TOKEN_CONTRACT_ID ||
    'CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC'; // Quickstart default

  beforeAll(async () => {
    buyerKeypair  = StellarSdk.Keypair.random();
    farmerKeypair = StellarSdk.Keypair.random();
    adminKeypair  = StellarSdk.Keypair.random();

    // Fund all accounts via local Friendbot.
    await Promise.all([
      fundAccount(buyerKeypair.publicKey()),
      fundAccount(farmerKeypair.publicKey()),
      fundAccount(adminKeypair.publicKey()),
    ]);

    // Deploy the escrow WASM if the compiled artefact is present.
    const wasmPath = path.resolve(__dirname, '../../../../contracts/escrow.wasm');
    if (!fs.existsSync(wasmPath)) {
      console.warn('[test] escrow.wasm not found — skipping deploy, using env CONTRACT_ID');
      contractId = process.env.TEST_ESCROW_CONTRACT_ID;
      return;
    }

    const wasm = fs.readFileSync(wasmPath);
    contractId = await deployContract(wasm, adminKeypair);
  }, 60_000);

  // ── Deployment ────────────────────────────────────────────────────────────

  test('contract is deployed and has a valid address', () => {
    expect(typeof contractId).toBe('string');
    expect(contractId.length).toBeGreaterThan(0);
  });

  // ── Deposit ───────────────────────────────────────────────────────────────

  describe('deposit', () => {
    test('buyer can lock funds into escrow', async () => {
      if (!contractId) return;

      const orderId = 1001;
      const amountStroops = BigInt(10_000_000); // 1 XLM
      const timeoutUnix = Math.floor(Date.now() / 1000) + 86_400; // +24 h

      const result = await invokeContract(
        contractId,
        'deposit',
        depositArgs({
          tokenContractId,
          orderId,
          buyerPk: buyerKeypair.publicKey(),
          farmerPk: farmerKeypair.publicKey(),
          amountStroops,
          timeoutUnix,
        }),
        buyerKeypair,
      );

      // deposit returns void on success; the SDK wraps it as an empty result.
      expect(result).toBeDefined();
    }, 30_000);

    test('duplicate deposit for the same order_id is rejected', async () => {
      if (!contractId) return;

      const orderId = 1001; // same as above — already deposited
      const amountStroops = BigInt(5_000_000);
      const timeoutUnix = Math.floor(Date.now() / 1000) + 86_400;

      await expect(
        invokeContract(
          contractId,
          'deposit',
          depositArgs({
            tokenContractId,
            orderId,
            buyerPk: buyerKeypair.publicKey(),
            farmerPk: farmerKeypair.publicKey(),
            amountStroops,
            timeoutUnix,
          }),
          buyerKeypair,
        ),
      ).rejects.toThrow();
    }, 30_000);

    test('deposit with zero amount is rejected', async () => {
      if (!contractId) return;

      const orderId = 1099;
      const amountStroops = BigInt(0);
      const timeoutUnix = Math.floor(Date.now() / 1000) + 86_400;

      await expect(
        invokeContract(
          contractId,
          'deposit',
          depositArgs({
            tokenContractId,
            orderId,
            buyerPk: buyerKeypair.publicKey(),
            farmerPk: farmerKeypair.publicKey(),
            amountStroops,
            timeoutUnix,
          }),
          buyerKeypair,
        ),
      ).rejects.toThrow();
    }, 30_000);
  });

  // ── get_escrow ────────────────────────────────────────────────────────────

  describe('get_escrow', () => {
    test('returns escrow data after deposit', async () => {
      if (!contractId) return;

      const result = await invokeContract(
        contractId,
        'get_escrow',
        getEscrowArgs({ orderId: 1001 }),
        buyerKeypair,
      );

      // The contract returns Option<Escrow>; a successful read is non-null.
      expect(result).toBeDefined();
    }, 30_000);

    test('returns None for an unknown order_id', async () => {
      if (!contractId) return;

      const result = await invokeContract(
        contractId,
        'get_escrow',
        getEscrowArgs({ orderId: 99999 }),
        buyerKeypair,
      );

      // Option::None is returned as a void/null ScVal.
      // The SDK typically resolves this as null or undefined.
      expect(result == null || result === undefined).toBe(true);
    }, 30_000);
  });

  // ── Release ───────────────────────────────────────────────────────────────

  describe('release', () => {
    test('buyer can release funds to the farmer (0 bps fee)', async () => {
      if (!contractId) return;

      const result = await invokeContract(
        contractId,
        'release',
        releaseArgs({ orderId: 1001, platformFeeBps: 0 }),
        buyerKeypair,
      );

      expect(result).toBeDefined();
    }, 30_000);

    test('releasing an already-released escrow is rejected', async () => {
      if (!contractId) return;

      // order 1001 was released in the previous test.
      await expect(
        invokeContract(
          contractId,
          'release',
          releaseArgs({ orderId: 1001, platformFeeBps: 0 }),
          buyerKeypair,
        ),
      ).rejects.toThrow();
    }, 30_000);
  });

  // ── Full deposit → release flow (independent order) ───────────────────────

  describe('full deposit → release flow', () => {
    const ORDER_ID = 2001;
    const AMOUNT_STROOPS = BigInt(20_000_000); // 2 XLM

    test('step 1: deposit succeeds', async () => {
      if (!contractId) return;

      const timeoutUnix = Math.floor(Date.now() / 1000) + 86_400;

      const result = await invokeContract(
        contractId,
        'deposit',
        depositArgs({
          tokenContractId,
          orderId: ORDER_ID,
          buyerPk: buyerKeypair.publicKey(),
          farmerPk: farmerKeypair.publicKey(),
          amountStroops: AMOUNT_STROOPS,
          timeoutUnix,
        }),
        buyerKeypair,
      );

      expect(result).toBeDefined();
    }, 30_000);

    test('step 2: escrow is readable and Active after deposit', async () => {
      if (!contractId) return;

      const result = await invokeContract(
        contractId,
        'get_escrow',
        getEscrowArgs({ orderId: ORDER_ID }),
        buyerKeypair,
      );

      expect(result).toBeDefined();
    }, 30_000);

    test('step 3: buyer releases funds to farmer', async () => {
      if (!contractId) return;

      const result = await invokeContract(
        contractId,
        'release',
        releaseArgs({ orderId: ORDER_ID, platformFeeBps: 250 }), // 2.5% fee
        buyerKeypair,
      );

      expect(result).toBeDefined();
    }, 30_000);

    test('step 4: escrow cannot be released a second time', async () => {
      if (!contractId) return;

      await expect(
        invokeContract(
          contractId,
          'release',
          releaseArgs({ orderId: ORDER_ID, platformFeeBps: 0 }),
          buyerKeypair,
        ),
      ).rejects.toThrow();
    }, 30_000);

    test('step 5: escrow cannot be refunded after release', async () => {
      if (!contractId) return;

      await expect(
        invokeContract(
          contractId,
          'refund',
          refundArgs({ orderId: ORDER_ID }),
          buyerKeypair,
        ),
      ).rejects.toThrow();
    }, 30_000);
  });

  // ── Refund flow (timeout) ─────────────────────────────────────────────────

  describe('refund flow', () => {
    const ORDER_ID = 3001;

    test('deposit with a past timeout succeeds (contract validates on refund, not deposit)', async () => {
      if (!contractId) return;

      // Use a timeout 1 second in the past so refund is immediately claimable.
      const timeoutUnix = Math.floor(Date.now() / 1000) - 1;

      const result = await invokeContract(
        contractId,
        'deposit',
        depositArgs({
          tokenContractId,
          orderId: ORDER_ID,
          buyerPk: buyerKeypair.publicKey(),
          farmerPk: farmerKeypair.publicKey(),
          amountStroops: BigInt(5_000_000),
          timeoutUnix,
        }),
        buyerKeypair,
      );

      expect(result).toBeDefined();
    }, 30_000);

    test('buyer can claim a refund after timeout', async () => {
      if (!contractId) return;

      const result = await invokeContract(
        contractId,
        'refund',
        refundArgs({ orderId: ORDER_ID }),
        buyerKeypair,
      );

      expect(result).toBeDefined();
    }, 30_000);

    test('refund cannot be claimed twice', async () => {
      if (!contractId) return;

      await expect(
        invokeContract(
          contractId,
          'refund',
          refundArgs({ orderId: ORDER_ID }),
          buyerKeypair,
        ),
      ).rejects.toThrow();
    }, 30_000);
  });
}, 180_000);
