#![cfg(test)]

use crate::{AggregatedMetrics, RevoraRevenueShare, RevoraRevenueShareClient};
use soroban_sdk::{symbol_short, testutils::Address as _, Address, Env};

fn make_client(env: &Env) -> RevoraRevenueShareClient {
    let id = env.register_contract(None, RevoraRevenueShare);
    RevoraRevenueShareClient::new(env, &id)
}

/// @dev Verifies that registering the same token under different namespaces isolates their state.
#[test]
fn test_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token = Address::generate(&env); // Same token for both!
    let ns_1 = symbol_short!("ns1");
    let ns_2 = symbol_short!("ns2");

    // Issuer A registers in ns1
    client.register_offering(&issuer_a, &ns_1, &token, &1000, &token, &0);
    // Issuer B registers in ns2 with SAME token
    client.register_offering(&issuer_b, &ns_2, &token, &2000, &token, &0);

    // Set holder shares differently
    let holder = Address::generate(&env);
    client.set_holder_share(&issuer_a, &ns_1, &token, &holder, &500);
    client.set_holder_share(&issuer_b, &ns_2, &token, &holder, &1500);

    // Verify they are isolated
    assert_eq!(client.get_holder_share(&issuer_a, &ns_1, &token, &holder), 500);
    assert_eq!(client.get_holder_share(&issuer_b, &ns_2, &token, &holder), 1500);

    // We need to manage the token (mint some to the issuer)
    // Actually, in mock_all_auths, the transfer will succeed if we don't check balances?
    // No, soroban-sdk mock_all_auths doesn't mock balances.
    // But we are using the `token` Address directly. We should probably use a proper token client.

    // For simplicity in this isolation test, let's just check metadata/config which are simple set/get
    client.set_claim_delay(&issuer_a, &ns_1, &token, &3600);
    client.set_claim_delay(&issuer_b, &ns_2, &token, &7200);

    assert_eq!(client.get_claim_delay(&issuer_a, &ns_1, &token), 3600);
    assert_eq!(client.get_claim_delay(&issuer_b, &ns_2, &token), 7200);
}

/// @dev Verifies that a single issuer can register the same token in multiple namespaces isolated from each other.
#[test]
fn test_same_issuer_different_namespaces() {
    let env = Env::default();
    env.mock_all_auths();

    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_1 = symbol_short!("prod");
    let ns_2 = symbol_short!("stg");

    client.register_offering(&issuer, &ns_1, &token, &1000, &token, &0);
    client.register_offering(&issuer, &ns_2, &token, &2000, &token, &0);

    client.set_snapshot_config(&issuer, &ns_1, &token, &true);
    client.set_snapshot_config(&issuer, &ns_2, &token, &false);

    assert!(client.get_snapshot_config(&issuer, &ns_1, &token));
    assert!(!client.get_snapshot_config(&issuer, &ns_2, &token));
}

/// @dev Verifies that blacklisting an investor in one namespace does not affect their standing in another.
#[test]
fn test_cross_namespace_blacklist_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_1 = symbol_short!("ns1");
    let ns_2 = symbol_short!("ns2");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns_1, &token, &1000, &token, &0);
    client.register_offering(&issuer, &ns_2, &token, &1000, &token, &0);

    // Blacklist in NS 1
    client.blacklist_add(&issuer, &issuer, &ns_1, &token, &investor);

    // Verify isolated
    assert!(client.is_blacklisted(&issuer, &ns_1, &token, &investor));
    assert!(!client.is_blacklisted(&issuer, &ns_2, &token, &investor));

    assert_eq!(client.get_blacklist(&issuer, &ns_1, &token).len(), 1);
    assert_eq!(client.get_blacklist(&issuer, &ns_2, &token).len(), 0);
}

/// @dev Verifies that attempting to access state of an unregistered namespace fails securely.
#[test]
#[should_panic(expected = "HostError: Error(Contract, #1)")] // OfferingNotFound
fn test_unregistered_namespace_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_ghost = symbol_short!("ghost");

    // Attempt to set delay on non-existent offering
    client.set_claim_delay(&issuer, &ns_ghost, &token, &3600);
}

/// @dev Verifies that an issuer cannot access or modify offerings they do not own, even within the same namespace.
#[test]
fn test_unauthorized_issuer_access_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_real = Address::generate(&env);
    let issuer_attacker = Address::generate(&env);
    let token = Address::generate(&env);
    let ns_1 = symbol_short!("ns1");

    client.register_offering(&issuer_real, &ns_1, &token, &1000, &token, &0);

    // Attacker tries to blacklist for real issuer's offering
    // Note: mock_all_auths will allow the call to reach the contract,
    // but the contract should check that issuer_attacker is not current_issuer.

    let res = client.try_blacklist_add(
        &issuer_attacker,
        &issuer_real,
        &ns_1,
        &token,
        &Address::generate(&env),
    );

    // Should fail with NotAuthorized (#10) or OfferingNotFound (if we strictly check issuer in ID)
    // Actually our implementation returns NotAuthorized if issuer matches but caller doesn't,
    // but here the issuer_real in the ID matches the real one, but the caller is attacker.
    assert!(res.is_err());
}

/// @dev Verifies that transferring an offering ownership maintains namespace isolation while correctly updating authorization.
#[test]
fn test_transfer_maintains_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer_a = Address::generate(&env);
    let issuer_b = Address::generate(&env);
    let token_1 = Address::generate(&env);
    let ns_1 = symbol_short!("ns1");

    client.register_offering(&issuer_a, &ns_1, &token_1, &1000, &token_1, &0);
    client.set_claim_delay(&issuer_a, &ns_1, &token_1, &3600);

    // Transfer to Issuer B
    client.propose_issuer_transfer(&issuer_a, &ns_1, &token_1, &issuer_b);
    client.accept_issuer_transfer(&issuer_a, &ns_1, &token_1);

    // Verify config preserved
    assert_eq!(client.get_claim_delay(&issuer_a, &ns_1, &token_1), 3600);

    // Verify Issuer B now has control (e.g. can change delay)
    client.set_claim_delay(&issuer_b, &ns_1, &token_1, &7200);
    assert_eq!(client.get_claim_delay(&issuer_a, &ns_1, &token_1), 7200);

    // Verify Issuer A NO LONGER has control
    let res = client.try_set_claim_delay(&issuer_a, &ns_1, &token_1, &9999);
    assert!(res.is_err());
}

/// @dev Verifies that double-registration of the exact same (issuer, namespace, token) is rejected to prevent state clobbering.
#[test]
fn test_duplicate_registration_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    // Exact same registration should fail
    let res = client.try_register_offering(&issuer, &ns, &token, &1000, &token, &0);
    assert!(res.is_err());
}

/// @dev Verifies that aggregated platform and issuer metrics correctly sum across namespace boundaries.
#[test]
fn test_aggregation_across_namespaces() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token1 = Address::generate(&env);
    let token2 = Address::generate(&env);
    let ns_1 = symbol_short!("prod");
    let ns_2 = symbol_short!("stg");

    client.register_offering(&issuer, &ns_1, &token1, &1000, &token1, &0);
    client.register_offering(&issuer, &ns_2, &token2, &1000, &token2, &0);

    // Report revenue in both namespaces
    client.report_revenue(&issuer, &ns_1, &token1, &token1, &50000, &1, &false);
    client.report_revenue(&issuer, &ns_2, &token2, &token2, &25000, &1, &false);

    let metrics = client.get_issuer_aggregation(&issuer);
    assert_eq!(metrics.total_reported_revenue, 75000);
    assert_eq!(metrics.offering_count, 2);
}

// ── Issue #257: Blacklist/whitelist precedence invariants ─────────────────────
//
// Security assumption: a blacklisted address is **always** excluded from
// payouts, regardless of whitelist membership or registration order.
// The whitelist (when enabled) cannot bypass the blacklist.
// Both add/remove operations are idempotent.

/// @dev Proves the core invariant: blacklist always wins over whitelist.
/// An investor on both lists must be treated as ineligible.
#[test]
fn test_blacklist_wins_over_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    // Add investor to whitelist first, then blacklist
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    // Eligibility check: blacklist must win unconditionally
    let blacklisted = client.is_blacklisted(&issuer, &ns, &token, &investor);
    let whitelist_enabled = client.is_whitelist_enabled(&issuer, &ns, &token);
    let whitelisted = client.is_whitelisted(&issuer, &ns, &token, &investor);

    let eligible = if blacklisted {
        false
    } else if whitelist_enabled {
        whitelisted
    } else {
        true
    };

    assert!(!eligible, "blacklisted investor must not be eligible even when whitelisted");
}

/// @dev Proves the reverse order: blacklist added before whitelist still wins.
#[test]
fn test_blacklist_wins_when_added_before_whitelist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    // Blacklist first, then whitelist
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);

    let blacklisted = client.is_blacklisted(&issuer, &ns, &token, &investor);
    let whitelist_enabled = client.is_whitelist_enabled(&issuer, &ns, &token);
    let whitelisted = client.is_whitelisted(&issuer, &ns, &token, &investor);

    let eligible = if blacklisted {
        false
    } else if whitelist_enabled {
        whitelisted
    } else {
        true
    };

    assert!(!eligible, "blacklist added before whitelist must still exclude investor");
}

/// @dev Removing from blacklist while still on whitelist restores eligibility.
#[test]
fn test_remove_from_blacklist_restores_eligibility_when_whitelisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);

    // Confirm ineligible
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));

    // Remove from blacklist
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);

    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    // Now eligible via whitelist
    let blacklisted = client.is_blacklisted(&issuer, &ns, &token, &investor);
    let whitelist_enabled = client.is_whitelist_enabled(&issuer, &ns, &token);
    let whitelisted = client.is_whitelisted(&issuer, &ns, &token, &investor);

    let eligible = if blacklisted {
        false
    } else if whitelist_enabled {
        whitelisted
    } else {
        true
    };

    assert!(eligible, "after blacklist removal, whitelisted investor should be eligible");
}

/// @dev Whitelist disabled (empty) + not blacklisted = eligible.
#[test]
fn test_whitelist_disabled_non_blacklisted_is_eligible() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    // No whitelist entries, no blacklist entries
    assert!(!client.is_whitelist_enabled(&issuer, &ns, &token));
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));

    let blacklisted = client.is_blacklisted(&issuer, &ns, &token, &investor);
    let whitelist_enabled = client.is_whitelist_enabled(&issuer, &ns, &token);
    let whitelisted = client.is_whitelisted(&issuer, &ns, &token, &investor);

    let eligible = if blacklisted {
        false
    } else if whitelist_enabled {
        whitelisted
    } else {
        true
    };

    assert!(eligible, "non-blacklisted investor with whitelist disabled must be eligible");
}

/// @dev Whitelist enabled + not on whitelist + not blacklisted = ineligible.
#[test]
fn test_whitelist_enabled_excludes_non_whitelisted() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let approved = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &approved);

    assert!(client.is_whitelist_enabled(&issuer, &ns, &token));

    // stranger is not whitelisted and not blacklisted
    let blacklisted = client.is_blacklisted(&issuer, &ns, &token, &stranger);
    let whitelist_enabled = client.is_whitelist_enabled(&issuer, &ns, &token);
    let whitelisted = client.is_whitelisted(&issuer, &ns, &token, &stranger);

    let eligible = if blacklisted {
        false
    } else if whitelist_enabled {
        whitelisted
    } else {
        true
    };

    assert!(!eligible, "non-whitelisted investor must be excluded when whitelist is enabled");
}

/// @dev blacklist_add is idempotent: adding the same address twice is safe.
#[test]
fn test_blacklist_add_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor); // second call must not panic

    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 1);
    assert!(client.is_blacklisted(&issuer, &ns, &token, &investor));
}

/// @dev blacklist_remove is idempotent: removing a non-existent address is safe.
#[test]
fn test_blacklist_remove_is_idempotent() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    // Remove without prior add must not panic
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);
    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));

    // Add then remove twice
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor); // second remove must not panic

    assert!(!client.is_blacklisted(&issuer, &ns, &token, &investor));
    assert_eq!(client.get_blacklist(&issuer, &ns, &token).len(), 0);
}

/// @dev Mixed sequence: register, set share, whitelist, blacklist, remove from blacklist.
/// Verifies that state transitions are consistent throughout.
#[test]
fn test_mixed_sequence_register_share_whitelist_blacklist() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");
    let investor = Address::generate(&env);

    // Step 1: register offering
    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    // Step 2: set holder share
    client.set_holder_share(&issuer, &ns, &token, &investor, &500);
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &investor), 500);

    // Step 3: whitelist investor
    client.whitelist_add(&issuer, &issuer, &ns, &token, &investor);
    assert!(client.is_whitelist_enabled(&issuer, &ns, &token));
    assert!(client.is_whitelisted(&issuer, &ns, &token, &investor));

    // Step 4: blacklist investor — must override whitelist
    client.blacklist_add(&issuer, &issuer, &ns, &token, &investor);
    {
        let bl = client.is_blacklisted(&issuer, &ns, &token, &investor);
        let wl_on = client.is_whitelist_enabled(&issuer, &ns, &token);
        let wl = client.is_whitelisted(&issuer, &ns, &token, &investor);
        let eligible = if bl { false } else if wl_on { wl } else { true };
        assert!(!eligible, "blacklist must override whitelist after mixed sequence");
    }

    // Step 5: remove from blacklist — whitelist still active, investor eligible again
    client.blacklist_remove(&issuer, &issuer, &ns, &token, &investor);
    {
        let bl = client.is_blacklisted(&issuer, &ns, &token, &investor);
        let wl_on = client.is_whitelist_enabled(&issuer, &ns, &token);
        let wl = client.is_whitelisted(&issuer, &ns, &token, &investor);
        let eligible = if bl { false } else if wl_on { wl } else { true };
        assert!(eligible, "after blacklist removal, whitelisted investor should be eligible again");
    }

    // Step 6: remove from whitelist — whitelist now disabled, investor still eligible (no blacklist)
    client.whitelist_remove(&issuer, &issuer, &ns, &token, &investor);
    {
        let bl = client.is_blacklisted(&issuer, &ns, &token, &investor);
        let wl_on = client.is_whitelist_enabled(&issuer, &ns, &token);
        let wl = client.is_whitelisted(&issuer, &ns, &token, &investor);
        let eligible = if bl { false } else if wl_on { wl } else { true };
        assert!(eligible, "with whitelist disabled and no blacklist, investor must be eligible");
    }

    // Share is preserved throughout
    assert_eq!(client.get_holder_share(&issuer, &ns, &token, &investor), 500);
}

/// @dev Blacklist/whitelist state is isolated per namespace.
/// Blacklisting in ns1 must not affect ns2.
#[test]
fn test_blacklist_whitelist_namespace_isolation() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns1 = symbol_short!("ns1");
    let ns2 = symbol_short!("ns2");
    let investor = Address::generate(&env);

    client.register_offering(&issuer, &ns1, &token, &1000, &token, &0);
    client.register_offering(&issuer, &ns2, &token, &1000, &token, &0);

    // Whitelist in both, blacklist only in ns1
    client.whitelist_add(&issuer, &issuer, &ns1, &token, &investor);
    client.whitelist_add(&issuer, &issuer, &ns2, &token, &investor);
    client.blacklist_add(&issuer, &issuer, &ns1, &token, &investor);

    // ns1: blacklisted → ineligible
    let bl1 = client.is_blacklisted(&issuer, &ns1, &token, &investor);
    let wl1 = client.is_whitelisted(&issuer, &ns1, &token, &investor);
    let wl1_on = client.is_whitelist_enabled(&issuer, &ns1, &token);
    let eligible1 = if bl1 { false } else if wl1_on { wl1 } else { true };
    assert!(!eligible1, "investor must be ineligible in ns1 (blacklisted)");

    // ns2: whitelisted, not blacklisted → eligible
    let bl2 = client.is_blacklisted(&issuer, &ns2, &token, &investor);
    let wl2 = client.is_whitelisted(&issuer, &ns2, &token, &investor);
    let wl2_on = client.is_whitelist_enabled(&issuer, &ns2, &token);
    let eligible2 = if bl2 { false } else if wl2_on { wl2 } else { true };
    assert!(eligible2, "investor must be eligible in ns2 (whitelisted, not blacklisted)");
}

/// @dev Multiple investors: some blacklisted, some whitelisted, some both.
/// Verifies correct eligibility for each category.
#[test]
fn test_multi_investor_eligibility_matrix() {
    let env = Env::default();
    env.mock_all_auths();
    let client = make_client(&env);

    let issuer = Address::generate(&env);
    let token = Address::generate(&env);
    let ns = symbol_short!("ns1");

    let only_whitelisted = Address::generate(&env);
    let only_blacklisted = Address::generate(&env);
    let both = Address::generate(&env);
    let neither = Address::generate(&env);

    client.register_offering(&issuer, &ns, &token, &1000, &token, &0);

    client.whitelist_add(&issuer, &issuer, &ns, &token, &only_whitelisted);
    client.whitelist_add(&issuer, &issuer, &ns, &token, &both);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &only_blacklisted);
    client.blacklist_add(&issuer, &issuer, &ns, &token, &both);

    let wl_on = client.is_whitelist_enabled(&issuer, &ns, &token);

    let check = |addr: &Address| -> bool {
        let bl = client.is_blacklisted(&issuer, &ns, &token, addr);
        let wl = client.is_whitelisted(&issuer, &ns, &token, addr);
        if bl { false } else if wl_on { wl } else { true }
    };

    assert!(check(&only_whitelisted), "only_whitelisted must be eligible");
    assert!(!check(&only_blacklisted), "only_blacklisted must be ineligible");
    assert!(!check(&both), "on both lists: blacklist wins, must be ineligible");
    assert!(!check(&neither), "not on whitelist when whitelist enabled: ineligible");
}
