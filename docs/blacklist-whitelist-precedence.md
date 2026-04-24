# Blacklist / Whitelist Precedence — Security & Risk Note

> Related issue: #257  
> Branch: `feature/soroban-blacklist-precedence`

---

## Rule

**Blacklist always wins.**  
A blacklisted address is unconditionally excluded from revenue payouts,
regardless of whitelist membership, holder share registration, or any
other on-chain state.

```
eligibility(addr) =
  if is_blacklisted(addr)  → INELIGIBLE  (unconditional)
  else if whitelist_enabled && !is_whitelisted(addr) → INELIGIBLE
  else → ELIGIBLE
```

This rule is enforced in `claim()` (src/lib.rs) before any storage
mutation or token transfer occurs.

---

## Security Assumptions

| Assumption | Rationale |
|---|---|
| Blacklist check is pre-transfer | Prevents any payout to a blocked address even if other state is inconsistent |
| Only issuer or admin can modify blacklist | Prevents unauthorized actors from re-enabling blocked investors |
| Blacklist is per-offering (issuer, namespace, token) | Namespace isolation prevents cross-offering leakage |
| Whitelist is optional; empty = disabled | When disabled, all non-blacklisted holders are eligible |
| Both operations are idempotent | Adding/removing an already-listed address is safe and emits an event |
| Blacklist capped at `MAX_BLACKLIST_SIZE` (200) | Prevents unbounded storage growth and gas DoS |

---

## Risk Note

- **Issuer key compromise**: If the issuer key is compromised, an attacker
  could remove addresses from the blacklist. Mitigation: use the admin
  role as a secondary guard; admin can also manage the blacklist.
- **Whitelist bypass attempt**: No combination of whitelist membership
  can restore eligibility for a blacklisted address. The blacklist check
  is evaluated first and short-circuits.
- **Namespace confusion**: Each offering is keyed by `(issuer, namespace,
  token)`. Blacklisting in one namespace has no effect on another.

---

## Test Coverage (issue #257)

All tests live in `src/test_namespaces.rs`:

| Test | What it proves |
|---|---|
| `test_blacklist_wins_over_whitelist` | BL added after WL still wins |
| `test_blacklist_wins_when_added_before_whitelist` | BL added before WL still wins |
| `test_remove_from_blacklist_restores_eligibility_when_whitelisted` | Removing from BL restores WL eligibility |
| `test_whitelist_disabled_non_blacklisted_is_eligible` | WL disabled + no BL = eligible |
| `test_whitelist_enabled_excludes_non_whitelisted` | WL enabled excludes non-members |
| `test_blacklist_add_is_idempotent` | Double-add is safe, count stays 1 |
| `test_blacklist_remove_is_idempotent` | Double-remove is safe, no panic |
| `test_mixed_sequence_register_share_whitelist_blacklist` | Full lifecycle with share updates |
| `test_blacklist_whitelist_namespace_isolation` | BL in ns1 does not affect ns2 |
| `test_multi_investor_eligibility_matrix` | 4-category matrix: WL-only, BL-only, both, neither |
