// Mindec -- Music metadata indexer
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! This mod deals with Chromecast interaction.

use std::mem;
use std::net::IpAddr;
use std::thread::JoinHandle;
use std::thread;

use mdns;
use mdns::RecordKind;
use rust_cast::channels::heartbeat::HeartbeatResponse;
use rust_cast::channels::receiver::{Application, CastDeviceApp};
use rust_cast::{CastDevice, ChannelMessage};

/// A Chromecast, as discovered using mDNS.
#[derive(Debug)]
pub struct CastAddr {
    pub host: IpAddr,
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
        let mut host: Option<IpAddr> = None;
        let mut name: Option<String> = None;
        let mut port: Option<u16> = None;

        for record in response.records() {
            match record.kind {
                RecordKind::A(addr_v4) => host = Some(addr_v4.into()),
                RecordKind::AAAA(addr_v6) => host = Some(addr_v6.into()),
                RecordKind::TXT(ref txts) => name = get_name_from_txt_record(&txts[..]),
                RecordKind::SRV { port: p, .. } => port = Some(p),
                _ => {}
            }
        }
        match (host, port, name) {
            (Some(host), Some(port), Some(name)) => {
                let cast_addr = CastAddr {
                    host: host,
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

enum CastImplState<'a> {
    Disconnected,
    Discovered(CastAddr),
    Connected(CastDevice<'a>),
    Launched(CastDevice<'a>, Application),
}

struct CastImpl<'a> {
    state: CastImplState<'a>,
}

impl<'a> CastImpl<'a> {
    pub fn new() -> CastImpl<'a> {
        CastImpl {
            state: CastImplState::Disconnected,
        }
    }

    fn discover() -> Option<CastAddr> {
        enumerate_casts_devices().pop()
    }

    fn connect(addr: &CastAddr) -> Option<CastDevice<'a>> {
        // This is a bit unfortunate ... CastDevice::connect takes a string,
        // while we have a perfectly parsed IP address lying around.
        let host_str = format!("{}", addr.host);
        println!("Connecting to {}.", addr.name);
        CastDevice::connect_without_host_verification(host_str, addr.port).ok()
    }

    fn launch(device: &mut CastDevice) -> Option<Application> {
        // Launch the default media player.
        device.connection.connect("receiver-0").ok()?;
        let def = CastDeviceApp::DefaultMediaReceiver;
        let app = device.receiver.launch_app(&def).ok()?;
        device.connection.connect(app.transport_id.as_ref()).ok()?;
        let status = device.receiver.get_status().ok()?;
        println!("Status {:?}", status);

        Some(app)
    }

    fn observe(device: &mut CastDevice, app: &Application) {
        loop {
            match device.receive() {
                Ok(ChannelMessage::Heartbeat(HeartbeatResponse::Ping)) => {
                    println!("-> ping");
                    device.heartbeat.pong().unwrap();
                }
                Ok(ChannelMessage::Heartbeat(..)) => {
                    println!("-> pong or something else");
                }
                Ok(ChannelMessage::Connection(resp)) => {
                    println!("-> connection {:?}", resp);
                }
                Ok(ChannelMessage::Media(resp)) => {
                    println!("-> media {:?}", resp);
                }
                Ok(ChannelMessage::Receiver(resp)) => {
                    println!("-> receiver {:?}", resp);
                }
                Ok(ChannelMessage::Raw(..)) => {
                    println!("-> raw");
                }
                Err(_) => break,
            }
        }
    }

    fn run(&mut self) {
        loop {
            let mut old_state = mem::replace(&mut self.state, CastImplState::Disconnected);
            self.state = match old_state {
                CastImplState::Disconnected => {
                    match CastImpl::discover() {
                        Some(addr) => CastImplState::Discovered(addr),
                        None => {
                            thread::yield_now();
                            CastImplState::Disconnected
                        }
                    }
                }
                CastImplState::Discovered(ref addr) => {
                    match CastImpl::connect(addr) {
                        Some(device) => CastImplState::Connected(device),
                        None => {
                            thread::yield_now();
                            CastImplState::Disconnected
                        }
                    }
                }
                CastImplState::Connected(mut device) => {
                    match CastImpl::launch(&mut device) {
                        Some(app) => CastImplState::Launched(device, app),
                        None => {
                            thread::yield_now();
                            CastImplState::Disconnected
                        }
                    }
                }
                CastImplState::Launched(ref mut device, ref app) => {
                    CastImpl::observe(device, app);
                    // When the connection is terminated, restart from the
                    // disconnected state.
                    thread::yield_now();
                    CastImplState::Disconnected
                }
            }
        }
    }
}

/// Handles connecting and controlling a cast device from a background thread.
///
/// The thread is alive as long as the `CastSession` is. It will open a
/// connection to a cast device, and keep it open.
pub struct CastSession {
    join_handle: JoinHandle<()>,
}

impl CastSession {
    pub fn new() -> CastSession {
        let join_handle = thread::spawn(|| {
            let mut cast_impl = CastImpl::new();
            cast_impl.run();
        });
        CastSession {
            join_handle: join_handle,
        }
    }

    pub fn join(self) {
        self.join_handle.join();
    }
}
