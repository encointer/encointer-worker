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

use std::sync::mpsc::channel;
use std::thread;

use sgx_crypto_helper::rsa3072::Rsa3072PubKey;

use codec::Decode;
use log::*;
use ws::connect;

use client::WsClient;
use requests::*;
use substratee_stf::{Getter, ShardIdentifier};

pub mod client;
pub mod requests;

#[derive(Clone)]
pub struct Api {
    url: String,
}

impl Api {
    pub fn new(url: String) -> Self {
        Self { url }
    }

    pub fn get_mu_ra_port(&self) -> Result<String, ()> {
        Self::get(&self, ClientRequest::MuRaPortWorker)
    }

    pub fn get_rsa_pubkey(&self) -> Result<Rsa3072PubKey, ()> {
        let keystr = Self::get(&self, ClientRequest::PubKeyWorker)?;

        let rsa_pubkey: Rsa3072PubKey = serde_json::from_str(&keystr).unwrap();
        info!("[+] Got RSA public key of enclave");
        debug!("  enclave RSA pubkey = {:?}", rsa_pubkey);
        Ok(rsa_pubkey)
    }

    pub fn get_stf_state(&self, getter: Getter, shard: &ShardIdentifier) -> Result<Vec<u8>, ()> {
        let req = ClientRequest::StfState(getter, shard.to_owned());
        match Self::get(&self, req) {
            Ok(res) => {
                let value_slice = if let Ok(v) = hex::decode(&res) {
                    v
                } else {
                    error!("worker api returned a value that can't be hex decoded: {}", res);
                    return Err(())
                };
                let value: Option<Vec<u8>> = Decode::decode(&mut &value_slice[..]).unwrap();
                match value {
                    Some(val) => Ok(val), // val is still an encoded option! can be None.encode() if storage doesn't exist
                    None => Err(()),      // there must've been an SgxResult::Err inside enclave
                }
            }
            Err(_) => Err(()), // ws error
        }
    }

    fn get(&self, request: ClientRequest) -> Result<String, ()> {
        let url = self.url.clone();
        let (port_in, port_out) = channel();

        info!("[Worker Api]: Sending request: {:?}", request);
        let client = thread::spawn(move || {
            match connect(url, |out| WsClient {
                out,
                request: request.clone(),
                result: port_in.clone(),
            }) {
                Ok(c) => c,
                Err(_) => {
                    error!("Could not connect to worker");
                }
            }
        });
        client.join().unwrap();

        match port_out.recv() {
            Ok(p) => Ok(p),
            Err(_) => {
                error!("[-] [WorkerApi]: error while handling request, returning");
                Err(())
            }
        }
    }
}
