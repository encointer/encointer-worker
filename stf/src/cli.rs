/*
    Copyright 2019 Supercomputing Systems AG

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.

*/

use crate::{AccountId, ShardIdentifier, TrustedCall, TrustedGetter, PublicGetter, TrustedOperation, Attestation};
use base58::{FromBase58, ToBase58};
use clap::{Arg, ArgMatches, AppSettings};
use clap_nested::{Command, Commander, MultiCommand};
use codec::{Decode, Encode};
use log::*;
use sp_application_crypto::{ed25519, sr25519};
use sp_core::{crypto::Ss58Codec, sr25519 as sr25519_core, Pair};
use sp_runtime::traits::IdentifyAccount;
use std::path::PathBuf;
use fixed::traits::LossyInto;
use fixed::transcendental::exp;
use my_node_runtime::{BlockNumber, Header, ONE_DAY, Signature};
use encointer_balances::{BalanceType, BalanceEntry};
use encointer_currencies::{Location, CurrencyIdentifier, CurrencyPropertiesType};
use encointer_ceremonies::{MeetupIndexType, ClaimOfAttendance, ParticipantIndexType, ProofOfAttendance};
use encointer_scheduler::{CeremonyPhaseType, CeremonyIndexType};
use hex;
use substrate_api_client::Api;
use sp_runtime::{MultiSignature, AccountId32};
use substrate_client_keystore::LocalKeystore;

type Moment = u64;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const KEYSTORE_PATH: &str = "my_trusted_keystore";

pub fn cmd<'a>(
    perform_operation: &'a dyn Fn(&ArgMatches<'_>, &TrustedOperation) -> Option<Vec<u8>>,
) -> MultiCommand<'a, str, str> {
    Commander::new()
        .options(|app| {
            app.setting(AppSettings::ColoredHelp)
                .arg(
                    Arg::with_name("mrenclave")
                        .short("m")
                        .long("mrenclave")
                        .global(true)
                        .takes_value(true)
                        .value_name("STRING")
                        .help("targeted worker MRENCLAVE"),
                )
                .arg(
                    Arg::with_name("shard")
                        .short("s")
                        .long("shard")
                        .global(true)
                        .takes_value(true)
                        .value_name("STRING")
                        .help("shard identifier"),
                )
                .arg(
                    Arg::with_name("xt-signer")
                        .short("a")
                        .long("xt-signer")
                        .global(true)
                        .takes_value(true)
                        .value_name("AccountId")
                        .default_value("//Alice")
                        .help("signer for publicly observable extrinsic"),
                )
                .name("encointer-client-teeproxy")
                .version(VERSION)
                .author("Supercomputing Systems AG <info@scs.ch>")
                .about("trusted calls to worker enclave")
                .after_help("stf subcommands depend on the stf crate this has been built against")
        })
        .add_cmd(
            Command::new("new-account")
                .description("generates a new incognito account for the given substraTEE shard")
                .runner(|_args: &str, matches: &ArgMatches<'_>| {
                    let store = LocalKeystore::open(get_keystore_path(matches), None).unwrap();
                    let key: sr25519::AppPair = store.generate().unwrap();
                    drop(store);
                    println!("{}", key.public().to_ss58check());
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("list-accounts")
                .description("lists all accounts in keystore for the substraTEE chain")
                .runner(|_args: &str, matches: &ArgMatches<'_>| {
                    let store = LocalKeystore::open(get_keystore_path(matches), None).unwrap();
                    info!("sr25519 keys:");
                    for pubkey in store
                        .public_keys::<sr25519::AppPublic>()
                        .unwrap()
                        .into_iter()
                    {
                        println!("{}", pubkey.to_ss58check());
                    }
                    info!("ed25519 keys:");
                    for pubkey in store
                        .public_keys::<ed25519::AppPublic>()
                        .unwrap()
                        .into_iter()
                    {
                        println!("{}", pubkey.to_ss58check());
                    }
                    drop(store);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("transfer")
                .description("send funds from one incognito account to another")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("from")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("sender's AccountId in ss58check format"),
                        )
                        .arg(
                            Arg::with_name("to")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("recipient's AccountId in ss58check format"),
                        )
                        .arg(
                            Arg::with_name("amount")
                                .takes_value(true)
                                .required(true)
                                .value_name("U128")
                                .help("amount to be transferred"),
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_from = matches.value_of("from").unwrap();
                    let arg_to = matches.value_of("to").unwrap();
                    let amount = u128::from_str_radix(matches.value_of("amount").unwrap(), 10)
                        .expect("amount can be converted to u128");
                    let from = get_pair_from_str(matches, arg_from);
                    let to = get_accountid_from_str(arg_to);
                    info!("from ss58 is {}", from.public().to_ss58check());
                    info!("to ss58 is {}", to.to_ss58check());

                    println!(
                        "send trusted call transfer from {} to {}: {}",
                        from.public(),
                        to,
                        amount
                    );
                    let (mrenclave, shard) = get_identifiers(matches);
                    let nonce = 0; // FIXME: hard coded for now
                    let top: TrustedOperation = TrustedCall::balance_transfer(
                        sr25519_core::Public::from(from.public()),
                        to,
                        shard, // for encointer we assume that every currency has its own shard. so shard == cid
                        BalanceType::from_num(amount))
                        .sign(&sr25519_core::Pair::from(from), nonce, &mrenclave, &shard)
                        .into();
                    let _ = perform_operation(matches, &top);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("balance")
                .description("query balance for incognito account in keystore")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_who = matches.value_of("accountid").unwrap();
                    let who = get_pair_from_str(matches, arg_who);
                    let (_mrenclave, shard) = get_identifiers(matches);
                    let top: TrustedOperation = TrustedGetter::balance(sr25519_core::Public::from(who.public()), shard)
                        .sign(&sr25519_core::Pair::from(who))
                        .into();
                    let res = perform_operation(matches, &top);
                    let bal = if let Some(v) = res {
                        if let Ok(vd) = <BalanceEntry<BlockNumber>>::decode(&mut v.as_slice()) {
                            let api = get_chain_api(matches);
                            let bn = get_block_number(&api);
                            let dr = get_demurrage_per_block(&api, shard);
                            debug!("will apply demurrage to {:?}. blocknumber {}, demurrage rate {}", vd, bn, dr);
                            apply_demurrage(vd, bn, dr)
                        } else {
                            info!("could not decode value. maybe hasn't been set? {:x?}", v);
                            BalanceType::from_num(0)
                        }
                    } else {
                        BalanceType::from_num(0)
                    };
                    println!("{}", bal);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("info")
                .description("query various statistics and settings for a currency (public information)")
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let (_mrenclave, shard) = get_identifiers(matches);
                    println!("Public information about currency {}", shard.encode().to_base58());
                    let top: TrustedOperation = PublicGetter::total_issuance(shard)
                        .into();
                    let res = perform_operation(matches, &top);
                    let bal = if let Some(v) = res {
                        if let Ok(vd) = <BalanceEntry<BlockNumber>>::decode(&mut v.as_slice()) {
                            let api = get_chain_api(matches);
                            let bn = get_block_number(&api);
                            let dr = get_demurrage_per_block(&api, shard);
                            debug!("will apply demurrage to {:?}. blocknumber {}, demurrage rate {}", vd, bn, dr);
                            apply_demurrage(vd, bn, dr)
                        } else {
                            info!("could not decode value. maybe hasn't been set? {:x?}", v);
                            BalanceType::from_num(0)
                        }
                    } else {
                        BalanceType::from_num(0)
                    };
                    println!("  total issuance: {}", bal);

                    let top: TrustedOperation = PublicGetter::participant_count(shard)
                        .into();
                    if let Some(v) = perform_operation(matches, &top) {
                        if let Ok(vd) = ParticipantIndexType::decode(&mut v.as_slice()) {
                            println!("  participant count: {}", vd);
                        } else { println!("  participant count: error decoding"); }
                    } else { println!("  participant count: undisclosed (might be REGISTERING phase?)"); };

                    let top: TrustedOperation = PublicGetter::meetup_count(shard)
                        .into();
                    if let Some(v) = perform_operation(matches, &top) {
                        if let Ok(vd) = MeetupIndexType::decode(&mut v.as_slice()) {
                            println!("  meetup count: {}", vd);
                        } else { println!("  meetup count: error decoding"); }
                    } else { println!("  meetup count: unknown"); };

                    let top: TrustedOperation = PublicGetter::ceremony_reward(shard)
                        .into();
                    if let Some(v) = perform_operation(matches, &top) {
                        if let Ok(vd) = BalanceType::decode(&mut v.as_slice()) {
                            println!("  ceremony reward: {}", vd);
                        } else { println!("  ceremony reward: error decoding"); }
                    } else { println!("  ceremony reward: unknown"); };

                    let top: TrustedOperation = PublicGetter::location_tolerance(shard)
                        .into();
                    if let Some(v) = perform_operation(matches, &top) {
                        if let Ok(vd) = u32::decode(&mut v.as_slice()) {
                            println!("  location tolerance: {}m", vd);
                        } else { println!("  location tolerance: error decoding"); }
                    } else { println!("  location tolerance: unknown"); };

                    let top: TrustedOperation = PublicGetter::time_tolerance(shard)
                        .into();
                    if let Some(v) = perform_operation(matches, &top) {
                        if let Ok(vd) = Moment::decode(&mut v.as_slice()) {
                            println!("  time tolerance: {}ms", vd);
                        } else { println!("  time tolerance: unknown nodecode"); }
                    } else { println!("  time tolerance: unknown"); };

                    let top: TrustedOperation = PublicGetter::scheduler_state(shard)
                        .into();
                    if let Some(v) = perform_operation(matches, &top) {
                        type SchedulerState = (CeremonyIndexType, CeremonyPhaseType, BlockNumber);
                        if let Ok(vd) = SchedulerState::decode(&mut v.as_slice()) {
                            println!("  ceremony index: {}", vd.0);
                            println!("  ceremony phase: {:?}", vd.1);
                            println!("  block number (sync height): {:?}", vd.2);
                        } else { println!("  scheduler state: decoding error"); }
                    } else { println!("  scheduler state: unknown"); };

                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("register-participant")
                .description("register participant for next encointer ceremony")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                        .arg(
                            Arg::with_name("reputation")
                                .short("r")
                                .long("reputation")
                                .help("prove attendance reputation for last ceremony"),
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_who = matches.value_of("accountid").unwrap();
                    let accountid = get_accountid_from_str(arg_who);
                    let who = get_pair_from_str(matches, arg_who);
                    let (mrenclave, shard) = get_identifiers(matches);
                    let api = get_chain_api(matches);
                    let cindex = get_ceremony_index(&api);
                    let nonce = 0; // FIXME: hard coded for now
                    println!(
                        "send TrustedCall::register_participant for {}",
                        who.public(),
                    );
                    let proof = if matches.is_present("reputation") {
                        Some(prove_attendance(&accountid, shard, cindex - 1, &who))
                    } else { None };
                    println!("reputation: {:?}", proof);
                    let top: TrustedOperation = TrustedCall::ceremonies_register_participant(
                        sr25519_core::Public::from(who.public()),
                        shard, // for encointer we assume that every currency has its own shard. so shard == cid
                        proof)
                        .sign(&sr25519_core::Pair::from(who), nonce, &mrenclave, &shard)
                        .into();
                    perform_operation(matches, &top);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("get-registration")
                .description("get participant registration index for next encointer ceremony")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_who = matches.value_of("accountid").unwrap();
                    let who = get_pair_from_str(matches, arg_who);
                    let (_mrenclave, shard) = get_identifiers(matches);
                    debug!(
                        "send TrustedGetter::get_registration for {}",
                        who.public()
                    );
                    let top: TrustedOperation = TrustedGetter::participant_index(
                        sr25519_core::Public::from(who.public()),
                        shard, // for encointer we assume that every currency has its own shard. so shard == cid
                    )
                        .sign(&sr25519_core::Pair::from(who))
                        .into();
                    let part = perform_operation(matches, &top).unwrap();
                    let participant: ParticipantIndexType = Decode::decode(&mut part.as_slice()).unwrap();
                    println!("{}", participant);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("get-meetup")
                .description("query meetup index assigned to account")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    match get_meetup_index_and_location(perform_operation, matches) {
                        Ok((index, _location)) => println!("{}", index),
                        Err(e) => panic!(e),
                    }
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("register-attestations")
                .description("register encointer ceremony attestations")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                        .arg(
                            Arg::with_name("attestations")
                                .takes_value(true)
                                .required(true)
                                .multiple(true)
                                .min_values(2)
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_who = matches.value_of("accountid").unwrap();
                    let who = get_pair_from_str(matches, arg_who);
                    let (mrenclave, shard) = get_identifiers(matches);
                    let nonce = 0; // FIXME: hard coded for now
                    let attestation_args: Vec<_> = matches.values_of("attestations").unwrap().collect();
                    let mut attestations: Vec<Attestation<MultiSignature, AccountId32, Moment>> = vec![];
                    for arg in attestation_args.iter() {
                        let w = Attestation::decode(&mut &hex::decode(arg).unwrap()[..]).unwrap();
                        attestations.push(w);
                    }
                    debug!("attestations: {:?}", attestations);
                    println!(
                        "send TrustedCall::register_attestations for {}",
                        who.public()
                    );
                    let top: TrustedOperation = TrustedCall::ceremonies_register_attestations(
                        sr25519_core::Public::from(who.public()),
                        attestations,
                    )
                        .sign(&sr25519_core::Pair::from(who), nonce, &mrenclave, &shard)
                        .into();
                    perform_operation(matches, &top);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("get-attestations")
                .description("get attestations registration index for this encointer ceremony")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_who = matches.value_of("accountid").unwrap();
                    let who = get_pair_from_str(matches, arg_who);
                    let (_mrenclave, shard) = get_identifiers(matches);
                    println!(
                        "send TrustedGetter::get_attestations for {}",
                        who.public(),
                    );
                    let top: TrustedOperation = TrustedGetter::attestations(
                        sr25519_core::Public::from(who.public()),
                        shard, // for encointer we assume that every currency has its own shard. so shard == cid
                    )
                        .sign(&sr25519_core::Pair::from(who))
                        .into();
                    let att_enc = perform_operation(matches, &top).unwrap();
                    let attestations: Vec<AccountId> = Decode::decode(&mut att_enc.as_slice()).unwrap();
                    println!("Attestations: {:?}", attestations);
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("new-claim")
                .description("create a fresh claim of attendance for account")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("accountid")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                        .arg(
                            Arg::with_name("n-participants")
                                .takes_value(true)
                                .required(true)
                        )
                })
                .runner(move |_args: &str, matches: &ArgMatches<'_>| {
                    let arg_who = matches.value_of("accountid").unwrap();
                    // println!("arg_who = {:?}", arg_who);
                    let who = get_pair_from_str(matches, arg_who);

                    let n_participants = matches
                        .value_of("n-participants")
                        .unwrap()
                        .parse::<u32>()
                        .unwrap();

                    let (_mrenclave, shard) = get_identifiers(matches);
                    let (mindex, mlocation) = match get_meetup_index_and_location(
                        perform_operation,
                        matches,
                    ) {
                        Ok((m, l)) => (m, l),
                        Err(e) => panic!(e),
                    };

                    let api = get_chain_api(matches);
                    let mtime = get_meetup_time(&api, mlocation);
                    info!("meetup time: {:?}", mtime);
                    let cindex = api.get_storage_value("EncointerScheduler", "CurrentCeremonyIndex", None)
                        .unwrap();

                    let claim = ClaimOfAttendance::<AccountId, Moment> {
                        claimant_public: who.public().into(),
                        currency_identifier: shard,
                        ceremony_index: cindex,
                        // ceremony_index: Default::default(),
                        meetup_index: mindex,
                        location: mlocation,
                        timestamp: mtime.unwrap(),
                        number_of_participants_confirmed: n_participants,
                    };
                    debug!("claim: {:?}", claim);
                    println!("{}", hex::encode(claim.encode()));
                    Ok(())
                }),
        )
        .add_cmd(
            Command::new("sign-claim")
                .description("sign someone's claim to attest personhood")
                .options(|app| {
                    app.setting(AppSettings::ColoredHelp)
                        .arg(
                            Arg::with_name("signer")
                                .takes_value(true)
                                .required(true)
                                .value_name("SS58")
                                .help("AccountId in ss58check format"),
                        )
                        .arg(
                            Arg::with_name("claim")
                                .takes_value(true)
                                .required(true)
                        )
                })
                .runner(|_args: &str, matches: &ArgMatches<'_>| {
                    let signer_arg = matches.value_of("signer").unwrap();
                    let claim = ClaimOfAttendance::decode(
                        &mut &hex::decode(matches.value_of("claim").unwrap()).unwrap()[..],
                    )
                        .unwrap();
                    let attestation = sign_claim(matches, claim, signer_arg);
                    println!("{}", hex::encode(attestation.encode()));
                    Ok(())
                }),
        )

        .into_cmd("trusted")
}

fn get_keystore_path(matches: &ArgMatches<'_>) -> PathBuf {
    let (_mrenclave, shard) = get_identifiers(matches);
    PathBuf::from(&format!("{}/{}", KEYSTORE_PATH, shard.encode().to_base58()))
}

pub fn get_identifiers(matches: &ArgMatches<'_>) -> ([u8; 32], ShardIdentifier) {
    let mut mrenclave = [0u8; 32];
    if !matches.is_present("mrenclave") {
        panic!("--mrenclave must be provided");
    };
    mrenclave.copy_from_slice(
        &matches
            .value_of("mrenclave")
            .unwrap()
            .from_base58()
            .expect("mrenclave has to be base58 encoded"),
    );
    let shard = match matches.value_of("shard") {
        Some(val) => ShardIdentifier::from_slice(
            &val.from_base58()
                .expect("mrenclave has to be base58 encoded"),
        ),
        None => ShardIdentifier::from_slice(&mrenclave),
    };
    (mrenclave, shard)
}

// TODO this function is redundant with client::main
fn get_accountid_from_str(account: &str) -> AccountId {
    match &account[..2] {
        "//" => sr25519::Pair::from_string(account, None)
            .unwrap()
            .public()
            .into_account(),
        _ => sr25519::Public::from_ss58check(account)
            .unwrap()
            .into_account(),
    }
}

// TODO this function is redundant with client::main
// get a pair either form keyring (well known keys) or from the store
fn get_pair_from_str(matches: &ArgMatches<'_>, account: &str) -> sr25519::AppPair {
    info!("getting pair for {}", account);
    match &account[..2] {
        "//" => sr25519::AppPair::from_string(account, None).unwrap(),
        _ => {
            info!("fetching from keystore at {}", &KEYSTORE_PATH);
            // open store without password protection
            let store = LocalKeystore::open(get_keystore_path(matches), None).expect("store should exist");
            info!("store opened");
            let _pair = store
                .key_pair::<sr25519::AppPair>(
                    &sr25519::Public::from_ss58check(account).unwrap().into(),
                )
                .unwrap();
            info!("key pair fetched");
            drop(store);
            _pair
        }
    }
}

fn get_chain_api(matches: &ArgMatches<'_>) -> Api<sr25519::Pair> {
    let url = format!(
        "{}:{}",
        matches.value_of("node-url").unwrap(),
        matches.value_of("node-port").unwrap()
    );
    info!("connecting to {}", url);
    Api::<sr25519::Pair>::new(url)
}

fn get_block_number(api: &Api<sr25519::Pair>) -> BlockNumber {
    let hdr: Header = api.get_header(None).unwrap();
    debug!("decoded: {:?}", hdr);
    //let hdr: Header= Decode::decode(&mut .as_bytes()).unwrap();
    hdr.number
}

fn get_demurrage_per_block(api: &Api<sr25519::Pair>, cid: CurrencyIdentifier) -> BalanceType {
    let cp: CurrencyPropertiesType = api
        .get_storage_map("EncointerCurrencies", "CurrencyProperties", cid, None)
        .expect("unknown currency");
    debug!("CurrencyProperties are {:?}", cp);
    cp.demurrage_per_block
}

fn apply_demurrage(entry: BalanceEntry<BlockNumber>, current_block: BlockNumber, demurrage_per_block: BalanceType) -> BalanceType {
    let elapsed_time_block_number = current_block.checked_sub(entry.last_update).unwrap();
    let elapsed_time_u32: u32 = elapsed_time_block_number.into();
    let elapsed_time = BalanceType::from_num(elapsed_time_u32);
    let exponent: BalanceType = -demurrage_per_block * elapsed_time;
    debug!("demurrage per block {}, current_block {}, last {}, elapsed_blocks {}", demurrage_per_block, current_block, entry.last_update, elapsed_time);
    let exp_result: BalanceType = exp(exponent).unwrap();
    entry.principal.checked_mul(exp_result).unwrap()
}

fn get_current_phase(api: &Api<sr25519::Pair>) -> CeremonyPhaseType {
    api.get_storage_value("EncointerScheduler", "CurrentPhase", None)
        .or(Some(CeremonyPhaseType::default()))
        .unwrap()
}

fn get_meetup_index_and_location<'a>(
    perform_operation: &'a dyn Fn(&ArgMatches<'_>, &TrustedOperation) -> Option<Vec<u8>>,
    matches: &ArgMatches<'_>) -> Result<(MeetupIndexType, Location), String> {
    let arg_who = matches.value_of("accountid").unwrap();
    // println!("arg_who = {:?}", arg_who);
    let who = get_pair_from_str(matches, arg_who);

    let (_mrenclave, shard) = get_identifiers(matches);
    let top: TrustedOperation = TrustedGetter::meetup_index(who.public().into(), shard)
        .sign(&sr25519_core::Pair::from(who.clone()))
        .into();

    let res = perform_operation(matches, &top).unwrap();
    let m_index: MeetupIndexType = Decode::decode(&mut res.as_slice()).unwrap();
    if m_index == 0 {
        return Err(format!("participant {} has not been assigned to a meetup. Meetup Index is 0", arg_who));
    }

    let api = get_chain_api(matches);
    let m_location = get_meetup_location(&api, m_index, shard);

    if m_location.is_none() {
        return Err(format!("participant {} has not been assigned to a meetup. Location is None", arg_who));
    };
    info!("got index {}", m_index);
    info!("got location: {:?}", m_location);
    return Ok((m_index, m_location.unwrap()));
}

fn get_meetup_location(api: &Api<sr25519::Pair>, m_index: MeetupIndexType, cid: CurrencyIdentifier) -> Option<Location> {
    return api.get_storage_map(
        "EncointerCurrencies",
        "Locations",
        cid,
        None,
    ).map(|locs: Vec<Location>| locs[m_index as usize]);
}

fn get_meetup_time(api: &Api<sr25519::Pair>, mlocation: Location) -> Option<Moment> {
    let mlon: f64 = mlocation.lon.lossy_into();
    // as long as the runtime pallet is rounding lon, we should do so too
    let mlon = mlon.round();
    debug!("meetup longitude: {}", mlon);
    let next_phase_timestamp: Moment = api.get_storage_value(
        "EncointerScheduler",
        "NextPhaseTimestamp",
        None,
    ).unwrap();
    debug!("next phase timestamp: {}", next_phase_timestamp);

    let attesting_start = match get_current_phase(api) {
        CeremonyPhaseType::ASSIGNING => next_phase_timestamp, // - next_phase_timestamp.rem(ONE_DAY),
        CeremonyPhaseType::ATTESTING => {
            let attesting_duration: Moment = api.get_storage_map(
                "EncointerScheduler",
                "PhaseDurations",
                CeremonyPhaseType::ATTESTING,
                None,
            ).unwrap();
            next_phase_timestamp - attesting_duration //- next_phase_timestamp.rem(ONE_DAY)
        }
        CeremonyPhaseType::REGISTERING => panic!("ceremony phase must be ASSIGNING or ATTESTING to request meetup location.")
    };
    debug!("attesting start at: {}", attesting_start);
    let mtime = (
        (attesting_start + ONE_DAY / 2) as i64 - (mlon * (ONE_DAY as f64) / 360.0) as i64
    ) as Moment;
    debug!("meetup time at lon {}: {:?}", mlon, mtime);
    Some(mtime)
}

fn sign_claim(matches: &ArgMatches<'_>, claim: ClaimOfAttendance<AccountId, Moment>, account_str: &str) -> Attestation<Signature, AccountId, Moment> {
    let pair = get_pair_from_str(matches, account_str);
    let accountid = get_accountid_from_str(account_str);
    Attestation {
        claim: claim.clone(),
        signature: Signature::from(sr25519_core::Signature::from(pair.sign(&claim.encode()))),
        public: accountid,
    }
}

fn get_ceremony_index(api: &Api<sr25519::Pair>) -> CeremonyIndexType {
    api.get_storage_value("EncointerScheduler", "CurrentCeremonyIndex", None)
        .unwrap()
}

fn prove_attendance(
    prover: &AccountId,
    cid: CurrencyIdentifier,
    cindex: CeremonyIndexType,
    attendee: &sr25519::AppPair,
) -> ProofOfAttendance<Signature, AccountId32> {
    let msg = (prover.clone(), cindex);
    debug!("generating proof of attendance for {} and cindex: {}", prover, cindex);
    debug!("signature payload is {:x?}", msg.encode());
    ProofOfAttendance {
        prover_public: AccountId32::from(*prover),
        currency_identifier: cid,
        ceremony_index: cindex,
        attendee_public: AccountId32::from(sr25519_core::Public::from(attendee.public())),
        attendee_signature: Signature::from(sr25519_core::Signature::from(
            attendee.sign(&msg.encode()),
        )),
    }
}
