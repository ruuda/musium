// Mindec -- Music metadata indexer
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! This mod deals with Chromecast interaction.

use std::net::IpAddr;

use mdns;
use mdns::RecordKind;

/// A Chromecast, as discovered using mDNS.
#[derive(Debug)]
pub struct CastAddr {
    pub addr: IpAddr,
    pub port: u16,
    pub name: String,
}

/// Returns the name of the Chromecast device as present in the TXT record.
fn get_name_from_txt_record(txts: &[String]) -> Option<String> {
    // The TXT record has the following format:
    //
    // rs=
    // nf=1
    // bs=<something uppercase hex>
    // st=0
    // ca=2052
    // fn=<user-chosen chromecast name>
    // ic=/setup/icon.png
    // md=Chromecast Audio
    // ve=05
    // rm=<something uppercase hex>
    // cd=<something uppercase hex>
    // id=<something lowercase hex>
    //
    // We take the device name from the fn record.
    for txt in txts {
        if txt.starts_with("fn=") {
            return Some(String::from(&txt[3..]))
        }
    }
    None
}

pub fn enumerate_casts_devices() -> Vec<CastAddr> {
    let mut addrs = Vec::new();

    for response in mdns::discover::all("_googlecast._tcp.local").unwrap() {
        let mut response = response.unwrap();
        let mut addr: Option<IpAddr> = None;
        let mut name: Option<String> = None;
        let mut port: Option<u16> = None;

        for record in response.records() {
            match record.kind {
                RecordKind::A(addr_v4) => addr = Some(addr_v4.into()),
                RecordKind::AAAA(addr_v6) => addr = Some(addr_v6.into()),
                RecordKind::TXT(ref txts) => name = get_name_from_txt_record(&txts[..]),
                RecordKind::SRV { port: p, .. } => port = Some(p),
                _ => {}
            }
        }
        match (addr, port, name) {
            (Some(addr), Some(port), Some(name)) => {
                let cast_addr = CastAddr {
                    addr: addr,
                    port: port,
                    name: name,
                };
                addrs.push(cast_addr);

                // TODO: This is a workaround for the following bug:
                // https://github.com/dylanmckay/mdns/issues/4
                break
            }
            _ => continue,
        }
    }

    addrs
}
