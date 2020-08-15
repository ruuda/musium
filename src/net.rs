// Musium -- Music playback daemon with web-based library browser
// Copyright 2018 Ruud van Asseldonk

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// A copy of the License has been included in the root of the repository.

//! Network utilities.

use std::mem;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr;

use libc;

/// Returns the IP (v4 and v6) addresses of all available network interfaces.
pub fn getifaddrs() -> Vec<IpAddr> {
    let mut addrs = Vec::new();

    unsafe {
        let mut ifaddrs: *mut libc::ifaddrs = ptr::null_mut();
        if libc::getifaddrs(&mut ifaddrs) != 0 {
            // Return an empty vector in the case of an error. I don't feel like
            // handling it properly at this point, and if this call fails, then
            // there are probably worse issues anyway.
            return addrs
        }
        let mut current = ifaddrs;
        while !current.is_null() {
            if !(*current).ifa_addr.is_null() {
                match (*(*current).ifa_addr).sa_family as i32 {
                    libc::AF_INET => {
                        let sa: *mut libc::sockaddr_in = mem::transmute((*current).ifa_addr);
                        // Note the to_be (big endian) conversion. Rust's From
                        // implementation assumes big-endian input, not machine
                        // endianness.
                        let addr = Ipv4Addr::from((*sa).sin_addr.s_addr.to_be());
                        addrs.push(IpAddr::from(addr));
                    }
                    libc::AF_INET6 => {
                        let sa: *mut libc::sockaddr_in6 = mem::transmute((*current).ifa_addr);
                        let addr = Ipv6Addr::from((*sa).sin6_addr.s6_addr);
                        addrs.push(IpAddr::from(addr));
                    }
                    _ => {},
                }
            }
            current = (*current).ifa_next;
        }
        libc::freeifaddrs(ifaddrs);
    }

    addrs
}
