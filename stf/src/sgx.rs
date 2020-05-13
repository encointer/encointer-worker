use sgx_tstd as std;
use std::collections::HashMap;
use std::prelude::v1::*;

use codec::{Compact, Decode, Encode};
use log_sgx::*;
use metadata::StorageHasher;
use sgx_runtime::{Balance, Runtime};
use sp_core::crypto::AccountId32;
use sp_io::SgxExternalitiesTrait;
use sp_runtime::traits::Dispatchable;
use encointer_scheduler::{CeremonyIndexType, CeremonyPhaseType};
use encointer_balances::BalanceType;
use encointer_currencies::CurrencyIdentifier;
use sgx_runtime::Moment;

use crate::{
    AccountId, BalanceTransferFn, State, Stf, TrustedCall, TrustedGetter, BALANCE_MODULE,
    BALANCE_TRANSFER,
};

type Index = u32;
type AccountData = ();//balances::AccountData<Balance>;
type AccountInfo = system::AccountInfo<Index, AccountData>;

impl Stf {
    pub fn init_state() -> State {
        debug!("initializing stf state");
        let mut ext = State::new();
        ext.execute_with(|| {
            // do not set genesis for pallets that are meant to be on-chain
            // use get_storage_hashes_to_update instead
            sp_io::storage::set(
                &storage_value_key("EncointerCeremonies", "CeremonyReward"),
                &BalanceType::from_num(1).encode(),
            );
            sp_io::storage::set(
                &storage_value_key("EncointerCeremonies", "TimeTolerance"),
                &Moment::from(600_000u32).encode(), // +-10min
            );
            sp_io::storage::set(
                &storage_value_key("EncointerCeremonies", "LocationTolerance"),
                &1_000u32.encode(),// [m] 
            );
        });
        ext
    }

    pub fn update_storage(ext: &mut State, map_update: HashMap<Vec<u8>, Vec<u8>>) {
        ext.execute_with(|| {
            map_update
                .iter()
                .for_each(|(k, v)| sp_io::storage::set(k, v))
        });
    }

    pub fn execute(
        ext: &mut State,
        call: TrustedCall,
        nonce: u32,
        calls: &mut Vec<BalanceTransferFn>,
    ) {
        ext.execute_with(|| {
            // TODO: enclave should not panic here.
            assert_eq!(
                nonce,
                u32::decode(
                    &mut sp_io::storage::get(&nonce_key_hash(call.account()))
                        .unwrap_or_else(|| 0u32.encode())
                        .as_slice()
                )
                .unwrap()
            );

            sp_io::storage::set(
                &nonce_key_hash(call.account()),
                (nonce + 1).encode().as_slice(),
            );

            let _result = match call {
                TrustedCall::balance_transfer(from, to, cid, value) => {
                    let origin = sgx_runtime::Origin::signed(AccountId32::from(from));
                    sgx_runtime::EncointerBalancesCall::<Runtime>::transfer(AccountId32::from(to), cid, value)
                        .dispatch(origin)
                }
                TrustedCall::ceremonies_register_participant(from, cid, proof) => {
                    let origin = sgx_runtime::Origin::signed(AccountId32::from(from));
                    sgx_runtime::EncointerCeremoniesCall::<Runtime>::register_participant(cid, proof)
                        .dispatch(origin)
                }
            };
        });
    }

    pub fn get_state(ext: &mut State, getter: TrustedGetter) -> Option<Vec<u8>> {
        ext.execute_with(|| match getter {
            TrustedGetter::balance(who, cid) => {
                Some(get_encointer_balance(&who, &cid).encode())
            }
        })
    }

    pub fn get_storage_hashes_to_update(call: &TrustedCall) -> Vec<Vec<u8>> {
        let mut key_hashes = Vec::new();
        match call {
            TrustedCall::balance_transfer(account, _, _, _) => {
                key_hashes.push(nonce_key_hash(account))
            },
            TrustedCall::ceremonies_register_participant(account, _, _) => {
                key_hashes.push(storage_value_key("EncointerScheduler", "CurrentPhase"));
                key_hashes.push(storage_value_key("EncointerScheduler", "CurrentCeremonyIndex"));
                key_hashes.push(storage_value_key("EncointerCurrencies", "CurrencyIdentifiers"));
            }
        };
        key_hashes
    }
}

// get the AccountInfo key where the nonce is stored
pub fn nonce_key_hash(account: &AccountId) -> Vec<u8> {
    storage_map_key(
        "System",
        "Account",
        account,
        &StorageHasher::Blake2_128Concat,
    )
}

fn get_account_info(who: &AccountId) -> Option<AccountInfo> {
    if let Some(infovec) = sp_io::storage::get(&storage_map_key(
        "System",
        "Account",
        who,
        &StorageHasher::Blake2_128Concat,
    )) {
        if let Ok(info) = AccountInfo::decode(&mut infovec.as_slice()) {
            Some(info)
        } else {
            None
        }
    } else {
        None
    }
}

fn get_encointer_balance(who: &AccountId, cid: &CurrencyIdentifier) -> BalanceType {
    if let Some(balvec) = sp_io::storage::get(&storage_double_map_key(
        "EncointerBalances",
        "Balance",
        cid, 
        &StorageHasher::Blake2_128Concat,
        who,
        &StorageHasher::Blake2_128Concat,
    )) {
        if let Ok(bal) = BalanceType::decode(&mut balvec.as_slice()) {
            bal
        } else {
            BalanceType::from_num(0)
        }
    } else {
        BalanceType::from_num(0)
    }
}

pub fn storage_value_key(module_prefix: &str, storage_prefix: &str) -> Vec<u8> {
    let mut bytes = sp_core::twox_128(module_prefix.as_bytes()).to_vec();
    bytes.extend(&sp_core::twox_128(storage_prefix.as_bytes())[..]);
    bytes
}

pub fn storage_map_key<K: Encode>(
    module_prefix: &str,
    storage_prefix: &str,
    mapkey1: &K,
    hasher1: &StorageHasher,
) -> Vec<u8> {
    let mut bytes = sp_core::twox_128(module_prefix.as_bytes()).to_vec();
    bytes.extend(&sp_core::twox_128(storage_prefix.as_bytes())[..]);
    bytes.extend(key_hash(mapkey1, hasher1));
    bytes
}

pub fn storage_double_map_key<K: Encode, Q: Encode>(
    module_prefix: &str,
    storage_prefix: &str,
    mapkey1: &K,
    hasher1: &StorageHasher,
    mapkey2: &Q,
    hasher2: &StorageHasher,
) -> Vec<u8> {
    let mut bytes = sp_core::twox_128(module_prefix.as_bytes()).to_vec();
    bytes.extend(&sp_core::twox_128(storage_prefix.as_bytes())[..]);
    bytes.extend(key_hash(mapkey1, hasher1));
    bytes.extend(key_hash(mapkey2, hasher2));
    bytes
}

/// generates the key's hash depending on the StorageHasher selected
fn key_hash<K: Encode>(key: &K, hasher: &StorageHasher) -> Vec<u8> {
    let encoded_key = key.encode();
    match hasher {
        StorageHasher::Identity => encoded_key.to_vec(),
        StorageHasher::Blake2_128 => sp_core::blake2_128(&encoded_key).to_vec(),
        StorageHasher::Blake2_128Concat => {
            // copied from substrate Blake2_128Concat::hash since StorageHasher is not public
            let x: &[u8] = encoded_key.as_slice();
            sp_core::blake2_128(x)
                .iter()
                .chain(x.iter())
                .cloned()
                .collect::<Vec<_>>()
        }
        StorageHasher::Blake2_256 => sp_core::blake2_256(&encoded_key).to_vec(),
        StorageHasher::Twox128 => sp_core::twox_128(&encoded_key).to_vec(),
        StorageHasher::Twox256 => sp_core::twox_256(&encoded_key).to_vec(),
        StorageHasher::Twox64Concat => sp_core::twox_64(&encoded_key).to_vec(),
    }
}
