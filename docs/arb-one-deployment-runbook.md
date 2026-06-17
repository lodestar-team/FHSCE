# FHSCE — Arbitrum One deployment runbook

Deploys `FileHostingDataService` (UUPS proxy) to Arbitrum One and proves the full paid loop:
**serve a file → signed TAP v2 receipt → metered RAV → on-chain `collect()` → GRT settled + 1% burned.**

> Spends real GRT (gas + a small escrow). All addresses below are Arbitrum One mainnet, verified against
> `@graphprotocol/horizon` `addresses.json` and confirmed working via an Arbitrum One fork dry-run.

## Real Horizon addresses (Arbitrum One, chainId 42161)

| Contract | Address |
|---|---|
| Controller | `0x0a8491544221dd212964fbb96487467291b2C97e` |
| GraphTallyCollector | `0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e` |
| HorizonStaking | `0x00669A4CF01450B64E8A2A20E9b1FCB71E61eF03` |
| PaymentsEscrow | `0xf6Fcc27aAf1fcD8B254498c9794451d82afC673E` |
| GraphPayments | `0x7Aae8ae011927BC36Cb4d0d3e81f2E6E30daE06D` |
| L2GraphToken (GRT) | `0x9623063377AD1B27544C965cCd7342f7EA7e88C7` |

## 1. Deploy the contract

```bash
export ARB_ONE_RPC="https://arb1.arbitrum.io/rpc"          # or your own node
export DEPLOYER_PK="0x<your funded deployer key>"           # needs ETH for gas
export CONTROLLER=0x0a8491544221dd212964fbb96487467291b2C97e
export GRAPH_TALLY_COLLECTOR=0x8f69F5C07477Ac46FBc491B1E6D91E2bb0111A9e
export OWNER=0x<owner/multisig>                             # contract owner + governance
export PAUSE_GUARDIAN=0x<pause guardian>
export MANIFESTS="<ipfs-manifest-cid>"                      # the firehose-flatfile bundle manifest

forge script script/Deploy.s.sol \
  --rpc-url "$ARB_ONE_RPC" --broadcast \
  --private-key "$DEPLOYER_PK" \
  --verify --etherscan-api-key "$ARBISCAN_KEY"
```

Record the printed **proxy** address — this is the `data_service_address` for the gateway and catalogue.

## 2. Become a provider (stake → provision → register → startService)

```bash
DS=0x<proxy from step 1>
GRT=0x9623063377AD1B27544C965cCd7342f7EA7e88C7
STAKING=0x00669A4CF01450B64E8A2A20E9b1FCB71E61eF03

# Soft launch: DEFAULT_MIN_PROVISION is 0, so any provision works. Provision a token to be safe.
cast send $GRT "approve(address,uint256)" $STAKING 1ether --rpc-url $ARB_ONE_RPC --private-key $DEPLOYER_PK
cast send $STAKING "stake(uint256)" 1ether --rpc-url $ARB_ONE_RPC --private-key $DEPLOYER_PK
# provision(serviceProvider, verifier=DS, tokens, maxVerifierCut, thawingPeriod>=14d)
cast send $STAKING "provision(address,address,uint256,uint32,uint64)" \
  $PROVIDER $DS 1ether 1000000 1209600 --rpc-url $ARB_ONE_RPC --private-key $DEPLOYER_PK

# register(provider, abi.encode(endpoint, geoHash, paymentsDestination))
DATA=$(cast abi-encode "f(string,string,address)" "https://fhsce.89.167.109.4.sslip.io" "u4pruydqqvs" $PROVIDER)
cast send $DS "register(address,bytes)" $PROVIDER $DATA --rpc-url $ARB_ONE_RPC --private-key $DEPLOYER_PK

# startService(provider, abi.encode(manifestId, endpoint))  — manifest must be addManifest'd by owner first
SVC=$(cast abi-encode "f(string,string)" "$MANIFESTS" "https://fhsce.89.167.109.4.sslip.io")
cast send $DS "startService(address,bytes)" $PROVIDER $SVC --rpc-url $ARB_ONE_RPC --private-key $DEPLOYER_PK
```

## 3. Run the provider stack (VPS)

- **file-service** — the upstream FHS data plane, serving the published firehose `.dbin` bundle (publish its
  manifest to IPFS first with the `file-exchange` publisher; allowlist that CID via `addManifest`).
- **fhsce-gateway** — `crates/fhsce-gateway`, configured from `gateway.example.toml`:
  `backend.upstream_url` → the file-service, `tap.data_service_address` → the proxy, `[collector]` enabled with
  the operator key + Arbitrum RPC. Exposed at `https://fhsce.89.167.109.4.sslip.io`.

## 4. Prove the paid loop

1. A consumer funds `PaymentsEscrow` for the provider and authorises a signer on `GraphTallyCollector`.
2. The consumer signs an EIP-712 TAP receipt (`GraphTallyCollector` domain, chainId 42161) per request and
   sends it in the `TAP-Receipt` header. The gateway verifies + meters it and proxies the chunk download.
3. The gateway's aggregator rolls receipts into a signed RAV; the collector submits it to
   `FileHostingDataService.collect()`. Confirm on Arbiscan: GRT settled to the provider, **1% burned**
   (`FeesBurned` event).
