//! ramshared-broker — protocolo (JSON-lines) + modelo + política do árbitro do Memory Broker.
//!
//! SPEC: `docs/memory-broker/SPECv2.md` ITEM-3/ITEM-4 (RF-B1, RF-B2, RF-B3, RF-L1; DT-1).
//!
//! Lib **pura, testável sem rede/root/GPU**: tipos do modelo ([`model`]), codec JSON-lines
//! ([`protocol`], DT-1), e — nos ITEM-4 — o mapa de slices e o árbitro (clock injetado). A
//! fiação (sockets, worker, IO) vive no daemon `ramshared-wsl2d` (ITEM-8).
#![forbid(unsafe_code)]

pub mod arbiter;
pub mod model;
pub mod protocol;
pub mod slices;
