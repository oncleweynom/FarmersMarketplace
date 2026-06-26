const StellarSdk = require('@stellar/stellar-sdk');

const STELLAR_NETWORK = (process.env.STELLAR_NETWORK || 'testnet').toLowerCase();

if (!['testnet', 'mainnet'].includes(STELLAR_NETWORK)) {
  throw new Error(`Invalid STELLAR_NETWORK "${STELLAR_NETWORK}". Must be "testnet" or "mainnet".`);
}

if (STELLAR_NETWORK === 'mainnet' && process.env.STELLAR_MAINNET_CONFIRMED !== 'true') {
  throw new Error(
    'Mainnet use requires STELLAR_MAINNET_CONFIRMED=true in your environment. ' +
      'This guard prevents accidental real-fund transactions.'
  );
}

const isTestnet = STELLAR_NETWORK === 'testnet';

const sorobanRpcUrl =
  process.env.SOROBAN_RPC_URL ||
  (isTestnet ? 'https://soroban-testnet.stellar.org' : 'https://soroban.stellar.org');
const sorobanServer = new StellarSdk.SorobanRpc.Server(sorobanRpcUrl);

const horizonUrl =
  process.env.STELLAR_HORIZON_URL ||
  (isTestnet ? 'https://horizon-testnet.stellar.org' : 'https://horizon.stellar.org');

const server = new StellarSdk.Horizon.Server(horizonUrl);
const networkPassphrase = isTestnet ? StellarSdk.Networks.TESTNET : StellarSdk.Networks.PUBLIC;

module.exports = { StellarSdk, isTestnet, server, sorobanServer, networkPassphrase };
