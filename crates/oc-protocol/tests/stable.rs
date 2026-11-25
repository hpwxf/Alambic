//! The bytes that must read the same in every version of the protocol from
//! 10 on: a tool opens with `Hello`, reads a module's refusal of its version,
//! and restarts a module into its bootloader, whatever the version on either
//! side. New variants of `Request`, `Reply` and `Error` therefore go last;
//! these ranks never move (`oc-protocol`'s `lib.rs`).

use oc_protocol::{Error, Reply, Request};

fn bytes<T: serde::Serialize>(value: &T) -> Vec<u8> {
    let mut buffer = [0u8; 16];
    postcard::to_slice(value, &mut buffer).unwrap().to_vec()
}

#[test]
fn a_hello_reads_the_same_in_every_version() {
    assert_eq!(bytes(&Request::Hello { protocol: 7 }), [0, 7]);
}

#[test]
fn a_version_refusal_reads_the_same_in_every_version() {
    assert_eq!(bytes(&Reply::Error(Error::Version(9))), [14, 2, 9]);
}

#[test]
fn a_restart_into_the_bootloader_reads_the_same_in_every_version() {
    assert_eq!(bytes(&Request::RebootToLoader), [6]);
}
