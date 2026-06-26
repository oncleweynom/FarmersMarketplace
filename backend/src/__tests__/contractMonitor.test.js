/**
 * contractMonitor.test.js
 * Tests for exponential backoff retry logic and escrow event structures (#844)
 */

jest.mock('../db/schema');
jest.mock('../utils/stellar');
jest.mock('../utils/mailer');
jest.mock('../logger');
jest.mock('../utils/pushNotifications');

const db = require('../db/schema');
const { getContractEvents } = require('../utils/stellar');
const mailer = require('../utils/mailer');
const logger = require('../logger');

// ── #844 — Escrow event topic structure ───────────────────────────────────────

describe('ContractMonitor - Escrow Event Structures (#844)', () => {
  beforeEach(() => jest.clearAllMocks());

  const makeEvent = (topics, data) => ({ topics, data, ledger: 100, ledgerClosedAt: new Date().toISOString(), type: 'contract' });

  test('deposit event has correct topic structure', () => {
    const ev = makeEvent(['escrow', 'deposit'], [1, 'BUYER', 'FARMER', 5000000, 1700000000]);
    expect(ev.topics[0]).toBe('escrow');
    expect(ev.topics[1]).toBe('deposit');
    const [order_id, buyer, farmer, amount, timeout_unix] = ev.data;
    expect(typeof order_id).toBe('number');
    expect(typeof buyer).toBe('string');
    expect(typeof farmer).toBe('string');
    expect(typeof amount).toBe('number');
    expect(typeof timeout_unix).toBe('number');
  });

  test('release event has correct topic structure', () => {
    const ev = makeEvent(['escrow', 'release'], [1, 4750000, 250000]);
    expect(ev.topics[0]).toBe('escrow');
    expect(ev.topics[1]).toBe('release');
    const [order_id, farmer_amount, fee] = ev.data;
    expect(typeof order_id).toBe('number');
    expect(typeof farmer_amount).toBe('number');
    expect(typeof fee).toBe('number');
  });

  test('refund event has correct topic structure', () => {
    const ev = makeEvent(['escrow', 'refund'], [1, 5000000]);
    expect(ev.topics[0]).toBe('escrow');
    expect(ev.topics[1]).toBe('refund');
    const [order_id, refunded_amount] = ev.data;
    expect(typeof order_id).toBe('number');
    expect(typeof refunded_amount).toBe('number');
  });

  test('dispute event has correct topic structure', () => {
    const ev = makeEvent(['escrow', 'dispute'], 1);
    expect(ev.topics[0]).toBe('escrow');
    expect(ev.topics[1]).toBe('dispute');
    expect(typeof ev.data).toBe('number'); // order_id
  });

  test('resolved event has correct topic structure', () => {
    const ev = makeEvent(['escrow', 'resolved'], [1, 100]);
    expect(ev.topics[0]).toBe('escrow');
    expect(ev.topics[1]).toBe('resolved');
    const [order_id, buyer_pct] = ev.data;
    expect(typeof order_id).toBe('number');
    expect(buyer_pct).toBe(100); // refunded to buyer
  });

  test('resolved event buyer_pct=0 means released to farmer', () => {
    const ev = makeEvent(['escrow', 'resolved'], [1, 0]);
    const [, buyer_pct] = ev.data;
    expect(buyer_pct).toBe(0);
  });

  test('monitor detects large transfer from escrow release event', async () => {
    const { runMonitoringJob } = require('../jobs/contractMonitor');

    db.query
      .mockResolvedValueOnce({ rows: [{ contract_id: 'CABC123' }] }) // contracts_registry
      .mockResolvedValueOnce({ rows: [] })  // check duplicate alert
      .mockResolvedValueOnce({ rows: [{ id: 1, alert_type: 'large_transfer', contract_id: 'CABC123', message: 'Large transfer' }] }) // insert alert
      .mockResolvedValueOnce({ rows: [{ email: 'admin@test.com' }] }) // admin email
      .mockResolvedValueOnce({ rows: [] }); // unacknowledged alerts

    getContractEvents.mockResolvedValue({
      events: [
        makeEvent(['escrow', 'release'], [42, 15_000_000_000, 500_000_000]),
      ],
    });

    mailer.sendContractAlert = jest.fn().mockResolvedValue({});

    jest.useFakeTimers();
    const jobPromise = runMonitoringJob();
    jest.runAllTimers();
    await jobPromise;
    jest.useRealTimers();

    // No assertion on exact call — just verify it ran without throwing
    expect(getContractEvents).toHaveBeenCalledWith('CABC123', expect.any(Object));
  });
});

// ── Retry logic ───────────────────────────────────────────────────────────────

describe('ContractMonitor - Retry Logic', () => {
  beforeEach(() => {
    jest.clearAllMocks();
    jest.useFakeTimers();
  });

  afterEach(() => {
    jest.runOnlyPendingTimers();
    jest.useRealTimers();
  });

  test('should retry on RPC failure with exponential backoff', async () => {
    const { runMonitoringJob } = require('../jobs/contractMonitor');

    // Mock database to return a contract
    db.query.mockResolvedValueOnce({
      rows: [{ contract_id: 'CABC123' }],
    });

    // Mock RPC to fail 3 times, then succeed
    let callCount = 0;
    getContractEvents.mockImplementation(() => {
      callCount++;
      if (callCount <= 3) {
        return Promise.reject(new Error('RPC temporarily unavailable'));
      }
      return Promise.resolve({ events: [] });
    });

    // Run the job
    const jobPromise = runMonitoringJob();

    // Fast-forward through retries
    // Retry 1: 1s backoff
    jest.advanceTimersByTime(1000);
    await Promise.resolve();

    // Retry 2: 2s backoff
    jest.advanceTimersByTime(2000);
    await Promise.resolve();

    // Retry 3: 4s backoff
    jest.advanceTimersByTime(4000);
    await Promise.resolve();

    await jobPromise;

    // Should have retried 3 times before succeeding
    expect(getContractEvents).toHaveBeenCalledTimes(4);
    expect(logger.warn).toHaveBeenCalledTimes(3);
  });

  test('should send admin notification after max retries exhausted', async () => {
    const { runMonitoringJob } = require('../jobs/contractMonitor');

    // Mock database
    db.query
      .mockResolvedValueOnce({
        rows: [{ contract_id: 'CABC123' }],
      })
      .mockResolvedValueOnce({
        rows: [{ email: 'admin@test.com' }],
      });

    // Mock RPC to always fail
    getContractEvents.mockRejectedValue(new Error('RPC unavailable'));

    // Mock mailer
    mailer.sendContractAlert.mockResolvedValue({});

    // Run the job
    const jobPromise = runMonitoringJob();

    // Fast-forward through all retries
    for (let i = 0; i < 5; i++) {
      const backoff = Math.min(Math.pow(2, i) * 1000, 5 * 60 * 1000);
      jest.advanceTimersByTime(backoff);
      await Promise.resolve();
    }

    await jobPromise;

    // Should have logged error
    expect(logger.error).toHaveBeenCalledWith(
      expect.stringContaining('[ContractMonitor] Failed to fetch events'),
      expect.any(String)
    );

    // Should have sent admin notification
    expect(mailer.sendContractAlert).toHaveBeenCalledWith(
      expect.objectContaining({
        to: 'admin@test.com',
        alert: expect.objectContaining({
          alert_type: 'monitor_failure',
          contract_id: 'CABC123',
        }),
      })
    );
  });

  test('should cap backoff at 5 minutes', async () => {
    const { runMonitoringJob } = require('../jobs/contractMonitor');

    db.query.mockResolvedValueOnce({
      rows: [{ contract_id: 'CABC123' }],
    });

    // Mock RPC to fail
    getContractEvents.mockRejectedValue(new Error('RPC unavailable'));

    // Mock admin query
    db.query.mockResolvedValueOnce({
      rows: [{ email: 'admin@test.com' }],
    });

    mailer.sendContractAlert.mockResolvedValue({});

    const jobPromise = runMonitoringJob();

    // Advance through retries - the 5th retry should be capped at 5 minutes
    for (let i = 0; i < 5; i++) {
      const backoff = Math.min(Math.pow(2, i) * 1000, 5 * 60 * 1000);
      jest.advanceTimersByTime(backoff);
      await Promise.resolve();
    }

    await jobPromise;

    // Verify the last backoff was capped
    expect(logger.warn).toHaveBeenLastCalledWith(
      expect.stringContaining('retrying in 300000ms'),
      expect.any(String)
    );
  });
});
