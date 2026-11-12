//! Nucleo sem dependencias: tudo aqui e logica pura (sem sockets, sem audio),
//! para poder ser testado em qualquer maquina.
pub mod matrix;
pub mod ptp;
pub mod resample;
pub mod rtp;
pub mod sap;
pub mod sdp;
pub mod state;
pub mod ui;

pub const RATE: u32 = 48_000;
/// Quadros por pacote com ptime de 1 ms a 48 kHz.
pub const FRAMES_PER_PACKET: usize = 48;
/// Dante aceita no maximo 8 canais por fluxo AES67.
pub const MAX_FLOW_CHANNELS: usize = 8;
