# Smart-Contract Capsules (v2.8)

Solidity and Vyper capsules (`smart_contract` risk class) deploy code to a
blockchain, where mistakes are irreversible and secret keys are catastrophic
to leak. v2.8 reviews them with compiler pins, EVM target metadata, a
required ABI hash, gas evidence, and a hard secret-key safety gate. Contract
capsules are always at least `review` and never auto-approved.

## Metadata

- **Compiler pin**: `version` carries the compiler version (`0.8.24` for
  Solidity, `0.3.10` for Vyper).
- **EVM target**: `env` carries the EVM target or chain (`evm-shanghai`).
- **ABI hash**: declared as a sandbox rule `abi.hash <algo:digest>` — the
  fingerprint of the contract interface.
- **Gas evidence**: declared as a sandbox rule `gas.limit <integer>`.
- **Chain effects**: `chain.read` / `chain.write`, covered by a `chain.*`
  sandbox grant (v2.4).

## Validation (blocking at `fgx check`)

- `contract_capsule_no_abi_hash` — a contract capsule must declare an ABI
  hash (`sandbox "abi.hash sha256:<digest>"`).
- `contract_capsule_secret_key` — a contract capsule must not request
  secret-key or wallet access. Any effect or sandbox grant mentioning
  `secret`, `wallet`, `privatekey`, `keystore`, `crypto.sign`, or `signer`
  is denied.

Missing gas evidence is a review note, not a hard rejection.

## Review report

```text
fgx contract-review <file> [--json] [--strict]
```

`forgentex.contract_review.v1` lists each contract capsule with compiler,
EVM target, ABI hash, gas limit, chain effects, any (forbidden) secret
access, and a `review`/`reject` decision. `--strict` exits non-zero on
`reject`.

## Example

`examples/capsule/price_oracle_solidity.fgx` — a Chainlink-style price
oracle pinned to Solidity 0.8.24 on `evm-shanghai`, with an ABI hash, a
`gas.limit`, and `chain.read` only. It reviews as `review`. A contract with
no ABI hash, or one requesting `crypto.sign` / `wallet.access`, is rejected
at `fgx check`.

## Acceptance gates (v2.8)

- Solidity/Vyper capsules classify as `smart_contract`.
- A contract without an ABI hash reports `contract_capsule_no_abi_hash` and
  reviews as `reject`.
- A contract requesting secret-key/wallet access reports
  `contract_capsule_secret_key`.
- `fgx contract-review` reports compiler, EVM target, ABI hash, and gas
  limit, and `--strict` exits non-zero on rejection.
