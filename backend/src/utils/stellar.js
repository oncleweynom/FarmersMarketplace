// Backward-compatible barrel — all callers continue to require('./utils/stellar').
// Internals are split into domain modules for maintainability.
const config = require('./stellar-config');
const accounts = require('./stellar-accounts');
const payments = require('./stellar-payments');
const contracts = require('./stellar-contracts');

module.exports = { ...config, ...accounts, ...payments, ...contracts };
